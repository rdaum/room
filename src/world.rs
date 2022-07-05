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
use log::{error, info};
use rmp_serde::Serializer;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tungstenite::Message;
use uuid::Uuid;

use crate::{
    fdb_object::ObjDBTxHandle,
    object::{Oid, Program, Value},
};
use crate::object::{AdminHandle, ObjDBHandle, SlotDef};
use crate::object::Error::{InvalidProgram, NoError, SlotDoesNotExist};
use crate::wasm_vm::WasmVM;

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

pub async fn get_slot(
    world: &Arc<World>,
    oid: Oid,
    key: Oid,
    slot_name: &str,
) -> Result<Value, Error> {
    let v = world
        .fdb_database
        .run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            match odb.get_slot(oid, key, String::from(slot_name)).await {
                Ok(slot) => Ok(slot),
                Err(_err) => Ok(Value::Error(SlotDoesNotExist)),
            }
        })
        .await?;

    Ok(v)
}

pub async fn set_slot(
    world: &Arc<World>,
    oid: Oid,
    key: Oid,
    slot_name: &str,
    value: &Value,
) -> Result<Value, Error> {
    world
        .fdb_database
        .run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            odb.set_slot(oid, key, String::from(slot_name), value);

            Ok(())
        })
        .await?;

    Ok(Value::Error(NoError))
}

pub async fn send_verb_dispatch(
    world: &Arc<World>,
    vm: Arc<WasmVM>,
    destoid: Oid,
    method: &str,
    arguments: &[Value],
) -> Result<Value, Error> {
    let vm = &vm.clone();
    let v = world
        .fdb_database
        .run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            match odb.get_slot(destoid, destoid, String::from(method)).await {
                Ok(sv) => {
                    let message_val = Value::Vector(arguments.to_vec());
                    match sv {
                        Value::Program(p) => Ok(vm
                            .execute(&p, &message_val)
                            .await
                            .expect("Couldn't invoke receive method")),
                        _ => {
                            error!("slot not a Program: {:?}", message_val);
                            Ok(Value::Error(InvalidProgram))
                        }
                    }
                }
                Err(r) => {
                    error!("Verb not found: {:?}", r);
                    Ok(Value::Error(SlotDoesNotExist))
                }
            }
        })
        .await
        .expect("Could not dispatch verb send");
    Ok(v)
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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Dump {
    slot_def: SlotDef,
    value: Value,
}

/// Iterate a directory loading values into slots.
/// Each file contains a MessagePack serialization of:
/// A header defining the slot
/// The value defining the slot contents
pub async fn load(world: Arc<World>, slot_path: &std::path::Path) -> Result<bool, Error> {
    assert!(slot_path.is_dir());

    let mut found = false;

    for entry in std::fs::read_dir(slot_path)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            let payload = std::fs::read(&path)?;
            let dump_result: Result<Dump, rmp_serde::decode::Error> =
                rmp_serde::from_slice(payload.as_slice());
            match dump_result {
                Ok(dump) => {
                    info!("Loading {:}-{:}.{:} from dump",
                      dump.slot_def.location.id.to_hyphenated().to_string(),
                                        dump.slot_def.key.id.to_hyphenated().to_string(),
                                        dump.slot_def.name);
                    set_slot(
                        &world.clone(),
                        dump.slot_def.location,
                        dump.slot_def.location,
                        &dump.slot_def.name,
                        &dump.value,
                    )
                        .await?;
                    found = true;
                }
                Err(e) => {
                    info!("File {:?} is not a valid slot dump: {:?}", entry.path(), e);
                }
            }
        }
    }

    Ok(found)
}

pub async fn save(
    world: Arc<World>,
    slot_path: &std::path::Path,
    oids: &Vec<Oid>,
) -> Result<(), Error> {
    assert!(slot_path.is_dir());
    world.fdb_database.run(|tr| async move {
        let odb = ObjDBTxHandle::new(&tr);
        for oid in oids {
            let slots = odb.dump_slots(*oid).unwrap();
            let collect = slots.collect::<Vec<(SlotDef, Value)>>();
            for slot in collect.await {
                let mut result_buf = Vec::new();
                let dump = Dump {
                    slot_def: slot.0.clone(),
                    value: slot.1.clone(),
                };
                dump
                    .serialize(&mut Serializer::new(&mut result_buf))
                    .unwrap();
                let pathname = format! {"{:}-{:}.{:}",
                                        &slot.0.location.id.to_hyphenated().to_string(),
                                        &slot.0.key.id.to_hyphenated().to_string(),
                                        &slot.0.name};
                let path = slot_path.join(std::path::Path::new(pathname.as_str()));
                info!("Writing slot {:?}", path);
                std::fs::write(path, result_buf).unwrap();
            }
        }
        Ok(())
    }).await?;

    Ok(())
}

pub async fn initialize_world(world: Arc<World>) -> Result<(), Error> {
    let sys_oid = Oid { id: Uuid::nil() };

    let dump_path = std::path::Path::new("dump");

    let dump_found = load(world.clone(), dump_path).await.unwrap();
    if !dump_found {
        info!("No dump found; bootstrapping minimal...");
        let bootstrap_objects = |tr| async move {
            let odb = ObjDBTxHandle::new(&tr);

            // Sys 'log' method.
            odb.set_slot(
                sys_oid,
                sys_oid,
                String::from("syslog"),
                &Value::Program(Program::from(String::from(
                    r#"(module
                            (import "host" "log" (func $host/log (param i32) (result i32 i32)))
                            (memory $mem 1)
                            (export "memory" (memory $mem))
                            (func $log (param $0 i32) (result i32 i32) get_local $0 (call $host/log))
                            (export "invoke" (func $log))
                            )
    "#,
                ))),
            );

            // Connection 'receive' method. Just does an 'echo' for now.
            odb.set_slot(
                sys_oid,
                sys_oid,
                String::from("receive"),
                &Value::Program(Program::from(String::from(
                    r#"(module
                            (import "host" "send" (func $host/send (param i32) (result i32 i32)))
                            (memory $mem 1)
                            (export "memory" (memory $mem))
                            (func $send (param $0 i32) (result i32 i32) get_local $0 (call $host/send))
                            (export "invoke" (func $send))
                            )
    "#,
                ))),
            );
            Ok(())
        };
        world.fdb_database.run(bootstrap_objects).await?;
    }

    save(world.clone(), dump_path, &vec![sys_oid]).await?;
    Ok(())
}
