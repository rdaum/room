use std::env;
use std::error::Error;

use bytes::Bytes;
use fdb::database::FdbDatabase;
use fdb::transaction::Transaction;
use fdb::tuple::Tuple;
use tungstenite::Message;
use uuid::Uuid;
use wasmtime;
use wasmtime::Module;

use crate::object::{Method, Object, Oid, Value};
use crate::fdb_object::ObjDBTxHandle;

// Owns the database and WASM runtime, and hosts methods for accessing the world.
pub struct World<'world_lifetime> {
    wasm_engine: wasmtime::Engine,
    wasm_linker: wasmtime::Linker<&'world_lifetime World<'world_lifetime>>,
    fdb_database: FdbDatabase,
}

impl<'world_lifetime> World<'world_lifetime> {
    pub fn new() -> World<'world_lifetime> {
        unsafe {
            fdb::select_api_version(710);
            fdb::start_network();
        }
        let fdb_cluster_file = env::var("FDB_CLUSTER_FILE").expect("FDB_CLUSTER_FILE not defined!");
        let fdb_database = fdb::open_database(fdb_cluster_file).expect("Could not open database");

        let engine = wasmtime::Engine::default();
        let mut linker: wasmtime::Linker<&World<'world_lifetime>> = wasmtime::Linker::new(&engine);

        let into_func = |_caller: wasmtime::Caller<'_, &World<'world_lifetime>>, param: i32| {
            println!("Got {:?} from WebAssembly", param);
            ()
        };

        linker
            .func_wrap("host", "log", into_func)
            .expect("Unable to link externals");

        World {
            wasm_engine: engine,
            wasm_linker: linker,
            fdb_database: fdb_database,
        }
    }

    pub async fn create_connection_object(&self) -> Result<Oid, Box<dyn Error>> {
        let new_oid = Oid { id: Uuid::new_v4() };
        self.fdb_database.run(|tr| async move {
            let odb = ObjDBTxHandle::new(&tr);
            let sys_oid = Oid { id: Uuid::nil() };
            let connection_proto = odb
                .get_property(sys_oid.clone(), sys_oid.clone(), String::from("connection"))
                .await
                .expect("Unable to get connection proto");

            match connection_proto {
                Value::Obj(conn_oid) => {
                    let conn_obj = Object {
                        oid: new_oid.clone(),
                        delegates: vec![conn_oid],
                    };
                    odb.put(conn_oid.clone(), &conn_obj);
                    Ok(())
                }
                v => {
                    panic!("Could not get connection proto OID, got {:?} instead", v);
                }
            }
        }).await.expect("Could not create connection object");
        Ok(new_oid)
    }

    pub async fn create_object(&self, delegates: Vec<Oid>) -> Result<Oid, Box<dyn Error>> {
        let new_oid = Oid { id: Uuid::new_v4() };
        let delegates = &delegates.clone();
        self.fdb_database
            .run(|tr| async move {
                let odb = ObjDBTxHandle::new(&tr);

                let new_obj = Object {
                    oid: new_oid.clone(),
                    delegates: delegates.clone(),
                };
                odb.put(new_oid.clone(), &new_obj);
                Ok(())
            })
            .await
            .expect("Unable to create object");
        Ok(new_oid)
    }

    pub async fn destroy_object(&self, oid: Oid) -> Result<(), Box<dyn Error>> {
        self.fdb_database
            .run(|tr| async move {
                let mut oid_tup = Tuple::new();
                oid_tup.add_uuid(oid.id);
                tr.clear(oid_tup.pack());
                Ok(())
            })
            .await
            .expect("Unable to destroy object");
        Ok(())
    }

    pub async fn receive(&self, connection: Oid, _message: Message) -> Result<(), Box<dyn Error>> {
        // Invoke "receive" method on the system object with the connection object
        self.fdb_database
            .run(|tr| async move {
                let odb = ObjDBTxHandle::new(&tr);

                let verbval = odb.find_verb(connection, String::from("receive")).await;
                println!("Receive verb: {:?}", verbval);
                self.execute_method(&verbval.unwrap())
                    .expect("Couldn't invoke receive method");
                Ok(())
            })
            .await
            .expect("Could not receive message");
        Ok(())
    }

    pub async fn initialize_world(&self) -> Result<(), Box<dyn Error>> {
        let sys_oid = Oid { id: Uuid::nil() };

        let bootstrap_objects = |tr| async move {
            // Create root object.
            let root_oid = Oid { id: Uuid::new_v4() };
            let bootstrap_root = Object {
                oid: root_oid.clone(),
                delegates: vec![],
            };
            let odb = ObjDBTxHandle::new(&tr);

            odb.put(root_oid.clone(), &bootstrap_root);

            // Create the sys object as a child of it.
            let bootstrap_sys = Object {
                oid: sys_oid.clone(),
                delegates: vec![root_oid],
            };
            odb.put(sys_oid.clone(), &bootstrap_sys);

            // Attach a reference to 'root' onto the sys object.
            odb.set_property(
                sys_oid.clone(),
                sys_oid.clone(),
                String::from("root"),
                &Value::Obj(root_oid.clone()),
            );

            // Then create connection prototype.
            let connection_prototype_oid = Oid { id: Uuid::new_v4() };
            let connection_prototype = Object {
                oid: connection_prototype_oid.clone(),
                delegates: vec![],
            };
            odb.put(connection_prototype_oid.clone(), &connection_prototype);
            odb.set_property(
                sys_oid.clone(),
                sys_oid.clone(),
                String::from("connection"),
                &Value::Obj(connection_prototype_oid.clone()),
            );

            // Sys 'log' method.
            // TODO actually handle proper string arguments etc. here
            odb.add_verb(
                sys_oid.clone(),
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
            odb.add_verb(
                connection_prototype_oid.clone(),
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
            let root_obj = odb.get(sys_oid.clone()).await;
            println!("Root Object {:?}", root_obj.unwrap());

            let verbval = odb.find_verb(sys_oid, String::from("syslog")).await;
            println!("Verb: {:?}", verbval);
            self.execute_method(&verbval.unwrap()).expect("Could not execute method");

            Ok(())
        };
        self.fdb_database.run(read_obj).await?;

        Ok(())
    }

    fn execute_method(&self, method: &Method) -> Result<(), Box<dyn Error>> {
        let mut store = wasmtime::Store::new(&self.wasm_engine, self);

        let module = Module::new(&self.wasm_engine, &method.method.as_ref())
            .expect("Not able to produce WASM module");
        let instance = self
            .wasm_linker
            .instantiate(&mut store, &module)
            .expect("Not able to create instance");

        let verb_func = instance
            .get_typed_func::<i32, (), _>(&mut store, "invoke")
            .expect("Didn't create typed func");

        Ok(verb_func.call(&mut store, 1).unwrap())
    }
}
