use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use anyhow::Error;
use bytes::Bytes;
use fdb::{database::FdbDatabase, transaction::Transaction};
use futures::{channel::mpsc::UnboundedSender, SinkExt};
use log::error;
use tungstenite::Message;
use uuid::Uuid;

use crate::object::ObjDBHandle;
use crate::wasm_vm::WasmVM;
use crate::{
    fdb_object::ObjDBTxHandle,
    object::{Oid, Program, Value},
};

type PeerMap = Arc<Mutex<HashMap<Oid, Connection>>>;

// Owns the database and WASM runtime, and hosts methods for accessing the world.
pub struct World {
    fdb_database: FdbDatabase,
    peer_map: PeerMap,
}

pub struct Connection {
    address: SocketAddr,
    sender: UnboundedSender<Message>,
    vm: Arc<WasmVM>,
}

impl World {
    pub fn new() -> Self {
        unsafe {
            fdb::select_api_version(710);
            fdb::start_network();
        }
        let fdb_cluster_file = env::var("FDB_CLUSTER_FILE").expect("FDB_CLUSTER_FILE not defined!");
        let fdb_database = fdb::open_database(fdb_cluster_file).expect("Could not open database");

        World {
            fdb_database,
            peer_map: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn register_connection(
    world: Arc<World>,
    sender: UnboundedSender<Message>,
    address: SocketAddr,
) -> Result<Oid, Error> {
    let new_oid = Oid { id: Uuid::new_v4() };
    let vm = Arc::new(WasmVM::new(world.clone()).unwrap());
    vm.clone().bind_builtins()?;
    world.peer_map.lock().unwrap().insert(
        new_oid,
        Connection {
            address,
            sender,
            vm,
        },
    );
    Ok(new_oid)
}

pub async fn disconnect(world: Arc<World>, oid: Oid) -> Result<(), Error> {
    world.peer_map.lock().unwrap().remove(&oid);
    world
        .fdb_database
        .run(|tr| async move {
            tr.clear(oid);
            Ok(())
        })
        .await
        .expect("Unable to destroy object");
    Ok(())
}

pub async fn receive_connection_message(
    world: &Arc<World>,
    connection: Oid,
    message: Bytes,
) -> Result<(), Error> {
    let vm = {
        let peer_map = world.peer_map.lock().unwrap();
        let con_record = &peer_map.get(&connection).unwrap();
        &con_record.vm.clone()
    };

    let m = &message.clone();
    world
        .fdb_database
        .run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            let sys_oid = Oid { id: Uuid::nil() };
            match odb
                .get_slot(sys_oid, sys_oid, String::from("receive"))
                .await
            {
                Ok(sv) => {
                    // Invoke "receive" program with connection obj and message as arguments.
                    let message_val =
                        Value::Vector(vec![Value::IdKey(connection), Value::Binary(m.to_vec())]);

                    match sv {
                        Value::Program(p) => {
                            vm.execute(&p, &message_val)
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

pub async fn send_verb_dispatch(
    world: &Arc<World>,
    vm: Arc<WasmVM>,
    destoid: Oid,
    method: &str,
    arguments: &[Value],
) -> Result<(), Error> {
    let vm = &vm.clone();
    world
        .fdb_database
        .run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            match odb.get_slot(destoid, destoid, String::from(method)).await {
                Ok(sv) => {
                    let message_val = Value::Vector(arguments.to_vec());
                    match sv {
                        Value::Program(p) => {
                            vm.execute(&p, &message_val)
                                .await
                                .expect("Couldn't invoke receive method");
                        }
                        _ => {
                            error!("slot not a Program: {:?}", message_val)
                        }
                    }
                }
                Err(r) => {
                    error!("Verb not found: {:?}", r)
                }
            }
            Ok(())
        })
        .await
        .expect("Could not dispatch verb send");
    Ok(())
}

pub async fn send_connection_message(
    world: Arc<World>,
    conoid: Oid,
    message: Message,
) -> Result<(), Error> {
    let mut tx = {
        let peer_map = world.peer_map.lock().unwrap();
        let connection = &peer_map.get(&conoid).unwrap();
        connection.sender.clone()
    };
    tx.send(message)
        .await
        .expect("Could not send message to connection");
    Ok(())
}

pub async fn initialize_world(world: Arc<World>) -> Result<(), Error> {
    let sys_oid = Oid { id: Uuid::nil() };

    let bootstrap_objects = |tr| async move {
        let odb = ObjDBTxHandle::new(&tr);

        // Sys 'log' method.
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
            ))),
        );

        // Connection 'receive' method. Just does an 'echo' fornow.
        odb.set_slot(
            sys_oid,
            sys_oid,
            String::from("receive"),
            &Value::Program(Program::from(String::from(
                r#"(module
                            (import "host" "send" (func $host/send (param i32)))
                            (memory $mem 1)
                            (export "memory" (memory $mem))
                            (func $send (param $0 i32) get_local $0 (call $host/send))
                            (export "invoke" (func $send))
                            )
    "#,
            ))),
        );
        Ok(())
    };
    world.fdb_database.run(bootstrap_objects).await?;

    Ok(())
}
