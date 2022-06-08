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
};

type PeerMap = Arc<Mutex<HashMap<Oid, (SocketAddr, UnboundedSender<Message>)>>>;

// Owns the database and WASM runtime, and hosts methods for accessing the world.
pub struct World {
    vm: Box<dyn VM + Send + Sync>,
    fdb_database: FdbDatabase,
    peer_map: PeerMap,
}

impl World {
    pub fn new(vm: Box<dyn VM + Send + Sync>) -> Self {
        unsafe {
            fdb::select_api_version(710);
            fdb::start_network();
        }
        let fdb_cluster_file = env::var("FDB_CLUSTER_FILE").expect("FDB_CLUSTER_FILE not defined!");
        let fdb_database = fdb::open_database(fdb_cluster_file).expect("Could not open database");

        World {
            vm,
            fdb_database,
            peer_map: Arc::new(Mutex::new(Default::default())),
        }
    }
}


pub async fn register_connection(
    world: Arc<World>,
    sender: UnboundedSender<Message>,
    address: SocketAddr,
) -> Result<Oid, Error> {
    let new_oid = Oid { id: Uuid::new_v4() };
    world.peer_map
        .lock()
        .unwrap()
        .insert(new_oid, (address, sender));
    Ok(new_oid)
}

pub async fn disconnect(world: Arc<World>, oid: Oid) -> Result<(), Error> {
    world.peer_map.lock().unwrap().remove(&oid);
    world.fdb_database
        .run(|tr| async move {
            tr.clear(oid);
            Ok(())
        })
        .await
        .expect("Unable to destroy object");
    Ok(())
}

pub async fn receive_connection_message(world: &Arc<World>, connection: Oid, message: Bytes) -> Result<(), Error> {
    let vm = &world.clone().vm;

    let m = &message.clone();
    world.fdb_database
        .run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            let sys_oid = Oid { id: Uuid::nil() };
            match odb.get_slot(sys_oid, sys_oid, String::from("receive")).await {
                Ok(sv) => {
                    // Invoke "receive" program with connection obj and message as arguments.
                    let message_val = Value::Vector(vec![Value::IdKey(connection), Value::Binary(m.to_vec())]);

                    match sv {
                        Value::Program(p) => {
                            vm
                                .execute(&p, world.clone(), &message_val)
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

pub async fn send_connection_message(world: Arc<World>, connection: Oid, message: Message) -> Result<(), Error> {
    let peer_map = world.peer_map.lock().unwrap();
    let (_socket_addr, tx) = &peer_map.get(&connection).unwrap();
    let mut tx = tx.clone();
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
    world.fdb_database.run(bootstrap_objects).await?;

    Ok(())
}
