use std::{
    collections::HashMap,
    env,
    error::Error,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use fdb::{database::FdbDatabase, transaction::Transaction};
use futures::{channel::mpsc::UnboundedSender, future::BoxFuture, FutureExt, SinkExt};
use log::{error};
use tungstenite::Message;
use uuid::Uuid;

use crate::{
    fdb_object::ObjDBTxHandle,
    object::{Method, Object, Oid, Value},
    vm::VM,
    world::World,
};


type PeerMap = Arc<Mutex<HashMap<Oid, (SocketAddr, UnboundedSender<Message>)>>>;

// Owns the database and WASM runtime, and hosts methods for accessing the world.
pub struct FdbWorld<'world_lifetime> {
    vm: Box<dyn VM + 'world_lifetime + Send + Sync>,
    fdb_database: FdbDatabase,
    peer_map: PeerMap,
}

impl<'world_lifetime> FdbWorld<'world_lifetime> {
    pub fn new(
        vm: Box<dyn VM + 'world_lifetime + Send + Sync>,
    ) -> Arc<dyn World + Send + Sync + 'world_lifetime> {
        unsafe {
            fdb::select_api_version(710);
            fdb::start_network();
        }
        let fdb_cluster_file = env::var("FDB_CLUSTER_FILE").expect("FDB_CLUSTER_FILE not defined!");
        let fdb_database = fdb::open_database(fdb_cluster_file).expect("Could not open database");

        Arc::new(FdbWorld {
            vm,
            fdb_database,
            peer_map: Arc::new(Mutex::new(Default::default())),
        })
    }
}

impl<'world_lifetime> World for FdbWorld<'world_lifetime> {
    fn connect(
        &self,
        sender: UnboundedSender<Message>,
        address: SocketAddr,
    ) -> BoxFuture<Result<Oid, Box<dyn Error>>> {
        async move {
            let new_oid = Oid { id: Uuid::new_v4() };
            self.peer_map
                .lock()
                .unwrap()
                .insert(new_oid, (address, sender));
            self.fdb_database
                .run(|tr| async move {
                    let odb = ObjDBTxHandle::new(&tr);
                    let sys_oid = Oid { id: Uuid::nil() };
                    let connection_proto = odb
                        .get_property(sys_oid, sys_oid, String::from("connection"))
                        .await
                        .expect("Unable to get connection proto");

                    match connection_proto {
                        Value::Obj(conn_oid) => {
                            let conn_obj = Object {
                                oid: new_oid,
                                delegates: vec![conn_oid],
                            };
                            odb.put(new_oid, &conn_obj);
                        }
                        v => {
                            panic!("Could not get connection proto OID, got {:?} instead", v);
                        }
                    }
                    Ok(())
                })
                .await
                .expect("Could not create connection object");
            Ok(new_oid)
        }
        .boxed()
    }

    fn disconnect(&self, oid: Oid) -> BoxFuture<Result<(), Box<dyn Error>>> {
        async move {
            self.peer_map.lock().unwrap().remove(&oid);
            self.fdb_database
                .run(|tr| async move {
                    tr.clear(oid);
                    Ok(())
                })
                .await
                .expect("Unable to destroy object");
            Ok(())
        }
        .boxed()
    }

    fn receive(&self, connection: Oid, _message: Message) -> BoxFuture<Result<(), Box<dyn Error>>> {
        async move {
            // Invoke "receive" method on the system object with the connection object
            self.fdb_database
                .run(|tr| async move {
                    let odb = ObjDBTxHandle::new(&tr);

                    let verbval = odb.find_verb(connection, String::from("receive")).await;
                    match verbval {
                        Ok(v) => {
                            self.vm
                                .execute_method(&v, self)
                                .expect("Couldn't invoke receive method");
                        }
                        Err(e) => {
                            error!("Receive verb not found: {:?}", e);
                        }
                    }
                    Ok(())
                })
                .await
                .expect("Could not receive message");
            Ok(())
        }
        .boxed()
    }

    fn send(&self, connection: Oid, message: Message) -> BoxFuture<Result<(), Box<dyn Error>>> {
        let peer_map = self.peer_map.lock().unwrap();
        let (_socket_addr, tx) = &peer_map.get(&connection).unwrap();
        let mut tx = tx.clone();
        async move {
            tx.send(message)
                .await
                .expect("Could not send message to connection");
            Ok(())
        }
        .boxed()
    }

    fn initialize(&self) -> BoxFuture<Result<(), Box<dyn Error>>> {
        async move {
            let sys_oid = Oid { id: Uuid::nil() };

            let bootstrap_objects = |tr| async move {
                // Create root object.
                let root_oid = Oid { id: Uuid::new_v4() };
                let bootstrap_root = Object {
                    oid: root_oid,
                    delegates: vec![],
                };
                let odb = ObjDBTxHandle::new(&tr);

                odb.put(root_oid, &bootstrap_root);

                // Create the sys object as a child of it.
                let bootstrap_sys = Object {
                    oid: sys_oid,
                    delegates: vec![root_oid],
                };
                odb.put(sys_oid, &bootstrap_sys);

                // Attach a reference to 'root' onto the sys object.
                odb.set_property(
                    sys_oid,
                    sys_oid,
                    String::from("root"),
                    &Value::Obj(root_oid),
                );

                // Then create connection prototype.
                let connection_prototype_oid = Oid { id: Uuid::new_v4() };
                let connection_prototype = Object {
                    oid: connection_prototype_oid,
                    delegates: vec![],
                };
                odb.put(connection_prototype_oid, &connection_prototype);
                odb.set_property(
                    sys_oid,
                    sys_oid,
                    String::from("connection"),
                    &Value::Obj(connection_prototype_oid),
                );

                // Sys 'log' method.
                // TODO actually handle proper string arguments etc. here
                odb.put_verb(
                    sys_oid,
                    String::from("syslog"),
                    &Method {
                        method: Bytes::from(
                            r#"(module
                            (import "host" "log" (func $host/log (param i32)))
                            (func $log (param $0 i32) get_local $0 (call $host/log))
                            (export "invoke" (func $log))
                            )
    "#,
                        ),
                    },
                );

                // Connection 'receive' method.
                // TODO actually handle the websocket payload here, etc.
                odb.put_verb(
                    connection_prototype_oid,
                    String::from("receive"),
                    &Method {
                        method: Bytes::from(
                            r#"(module
                            (import "host" "log" (func $host/log (param i32)))
                            (func $log (param $0 i32) get_local $0 (call $host/log))
                            (export "invoke" (func $log))
                            )
    "#,
                        ),
                    },
                );
                Ok(())
            };
            self.fdb_database.run(bootstrap_objects).await?;

            // Just a quick test of some of the functions for now.
            let read_obj = |tr| async move {
                let odb = ObjDBTxHandle::new(&tr);
                let root_obj = odb.get(sys_oid).await;
                println!("Root Object {:?}", root_obj.unwrap());

                let verbval = odb.find_verb(sys_oid, String::from("syslog")).await;
                println!("Verb: {:?}", verbval);
                self.vm
                    .execute_method(&verbval.unwrap(), self)
                    .expect("Could not execute method");

                Ok(())
            };
            self.fdb_database.run(read_obj).await?;

            Ok(())
        }
        .boxed()
    }
}
