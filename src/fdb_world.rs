use crate::object::{ObjDBHandle};
use anyhow::Error;
use bytes::Bytes;
use fdb::{database::FdbDatabase, transaction::Transaction};
use futures::{channel::mpsc::UnboundedSender, future::BoxFuture, FutureExt, SinkExt};
use log::error;
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tungstenite::Message;
use uuid::Uuid;


use crate::{
    fdb_object::ObjDBTxHandle,
    object::{Program, Oid, Value},
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
    pub fn new(vm: Box<dyn VM + 'world_lifetime + Send + Sync>) -> Self {
        unsafe {
            fdb::select_api_version(710);
            fdb::start_network();
        }
        let fdb_cluster_file = env::var("FDB_CLUSTER_FILE").expect("FDB_CLUSTER_FILE not defined!");
        let fdb_database = fdb::open_database(fdb_cluster_file).expect("Could not open database");

        FdbWorld {
            vm,
            fdb_database,
            peer_map: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl<'world_lifetime> World for FdbWorld<'world_lifetime> {
    fn connect(
        &self,
        sender: UnboundedSender<Message>,
        address: SocketAddr,
    ) -> BoxFuture<Result<Oid, Error>> {
        async move {
            let new_oid = Oid { id: Uuid::new_v4() };
            self.peer_map
                .lock()
                .unwrap()
                .insert(new_oid, (address, sender));
            Ok(new_oid)
        }
        .boxed()
    }

    fn disconnect(&self, oid: Oid) -> BoxFuture<Result<(), Error>> {
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

    fn receive(&self, connection: Oid, message: Bytes) -> BoxFuture<Result<(), Error>> {
        async move {
            let m = &message.clone();
            self.fdb_database
                .run(|tr| async move {
                    let odb = ObjDBTxHandle::new(&tr);
                    let sys_oid = Oid { id: Uuid::nil() };
                    match odb.get_slot(sys_oid, sys_oid, String::from("receive")).await {
                        Ok(sv) => {
                            // Invoke "receive" program with connection obj and message as arguments.
                            let message_val = Value::Vector(vec![Value::IdKey(connection), Value::Binary(m.to_vec())]);

                            match sv {
                                Value::Program(p) => {
                                    self.vm
                                        .execute(&p, self, &message_val)
                                        .await
                                        .expect("Couldn't invoke receive method");
                                }
                                _ => {
                                    error!("'receive' not a Program: {:?}", message_val)
                                }
                            }
                        }

                        Err(r) => {
                            error!("Receive program not found: {:?}", r)
                        }
                    };
                    Ok(())
                })
                .await
                .expect("Could not receive message");
            Ok(())
        }
        .boxed()
    }

    fn send(&self, connection: Oid, message: Message) -> BoxFuture<Result<(), Error>> {
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

    fn initialize(&self) -> BoxFuture<Result<(), Error>> {
        async move {
            let sys_oid = Oid { id: Uuid::nil() };

            let bootstrap_objects = |tr| async move {
                let odb = ObjDBTxHandle::new(&tr);

                // Sys 'log' method.
                // TODO actually handle proper string arguments etc. here
                odb.set_slot(
                    sys_oid,
                    sys_oid,
                    String::from("syslog"),
                    &Value::Program(Program::from(String::from(
                            r#"(module
                            (import "host" "log" (func $host/log (param i32)))
                            (memory $mem 1)
                            (export "memory" (memory $mem))
                            (func $log (param $0 i32) get_local $0 (call $host/log))
                            (export "invoke" (func $log))
                            )
    "#,
                        ))));

                // Connection 'receive' method.
                // TODO actually handle the websocket payload here, etc.
                odb.set_slot(
                    sys_oid,
                    sys_oid,
                    String::from("receive"),
                    &Value::Program(Program::from(String::from(
                            r#"(module
                            (import "host" "log" (func $host/log (param i32)))
                            (memory $mem 1)
                            (export "memory" (memory $mem))
                            (func $log (param $0 i32) get_local $0 (call $host/log))
                            (export "invoke" (func $log))
                            )
    "#,
                        ))));
                Ok(())
            };
            self.fdb_database.run(bootstrap_objects).await?;

            Ok(())
        }
        .boxed()
    }
}
