use std::env;
use std::error::Error;

use bytes::Bytes;
use fdb::database::FdbDatabase;
use fdb::transaction::Transaction;
use fdb::tuple::Tuple;
use tokio_stream::StreamExt;
use tungstenite::Message;
use uuid::Uuid;
use wasmtime;
use wasmtime::Module;

use crate::object::{Method, Object, ObjectDB, Oid, PropDef, Value};

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

        let into_func =
            |_caller: wasmtime::Caller<'_, &World<'world_lifetime>>, param: i32, arg: i32| {
                println!("Got {} {} from WebAssembly", param, arg);
                param + 1
            };

        linker
            .func_wrap("host", "console_log", into_func)
            .expect("Unable to link externals");
        World {
            wasm_engine: engine,
            wasm_linker: linker,
            fdb_database: fdb_database,
        }
    }

    pub async fn create_object(&self, delegates: Vec<Oid>) -> Result<Oid, Box<dyn Error>> {
        let new_oid = Oid { id: Uuid::new_v4() };
        let delegates = &delegates.clone();
        self.fdb_database
            .run(|tr| async move {
                let new_obj = Object {
                    oid: new_oid.clone(),
                    delegates: delegates.clone(),
                };
                ObjectDB::put(&tr, new_oid.clone(), &new_obj);
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
                let verbval = ObjectDB::find_verb(&tr, connection, String::from("receive")).await;
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
            // Create the root object.
            let bootstrap_sys = Object {
                oid: sys_oid.clone(),
                delegates: vec![],
            };
            ObjectDB::put(&tr, sys_oid.clone(), &bootstrap_sys);

            // And then the 'root' object as a child of it.
            let bootstrap_root = self
                .create_object(vec![sys_oid.clone()])
                .await
                .expect("Unable to create system object");

            // Attach a reference to 'root' onto the sys object.
            ObjectDB::set_property(
                &tr,
                sys_oid.clone(),
                sys_oid.clone(),
                String::from("root"),
                &Value::Obj(bootstrap_root.clone()),
            );

            // Sys 'log' method.
            ObjectDB::add_verb(
                &tr,
                sys_oid.clone(),
                String::from("syslog"),
                &Method {
                    /*
                    Compiled from:
                        extern void host_log(const char *log_line);

                        void syslog(const char *log_line) {
                          host_log(log_line);
                        }

                     Janky.
                     */
                    method: Bytes::from(
                        r#"(module
  (type (;0;) (func (param i32)))
  (import "env" "__linear_memory" (memory (;0;) 0))
  (import "env" "__stack_pointer" (global (;0;) (mut i32)))
  (import "env" "host_log" (func (;0;) (type 0)))
  (func $syslog (type 0) (param i32)
    (local i32 i32 i32 i32 i32 i32)
    global.get 0
    local.set 1
    i32.const 16
    local.set 2
    local.get 1
    local.get 2
    i32.sub
    local.set 3
    local.get 3
    global.set 0
    local.get 3
    local.get 0
    i32.store offset=12
    local.get 3
    i32.load offset=12
    local.set 4
    local.get 4
    call 0
    i32.const 16
    local.set 5
    local.get 3
    local.get 5
    i32.add
    local.set 6
    local.get 6
    global.set 0
    return))
    "#,
                    ),
                },
            );
            Ok(())
        };
        self.fdb_database.run(bootstrap_objects).await?;

        // Just a quick test of some of the functions for now.
        let read_obj = |tr| async move {
            let root_obj = ObjectDB::get(&tr, sys_oid.clone()).await;
            println!("Root Object {:?}", root_obj.unwrap());

            let verbval = ObjectDB::find_verb(&tr, sys_oid, String::from("syslog")).await;
            println!("Verb: {:?}", verbval);
            self.execute_method(&verbval.unwrap());

            Ok(())
        };
        self.fdb_database.run(read_obj).await?;

        Ok(())
    }

    fn execute_method(&self, method: &Method) -> Result<i32, Box<dyn Error>> {
        let mut store = wasmtime::Store::new(&self.wasm_engine, self);

        // TODO: how to wire this up to __linear_memory? And how to set __stack_pointer, etc?
        // Or don't. Point is to have a runtime environment / ABI for verbs, with available memory,
        // ability to pass arguments etc. Need to think about this.
        let memory_ty = wasmtime::MemoryType::new(1, Option::None);
        let memory = wasmtime::Memory::new(&mut store, memory_ty);

        let module = Module::new(&self.wasm_engine, &method.method.as_ref())
            .expect("Not able to produce WASM module");
        let instance = self
            .wasm_linker
            .instantiate(&mut store, &module)
            .expect("Not able to create instance");
        let verb_func = instance
            .get_typed_func::<i32, i32, _>(&mut store, "invoke")
            .expect("Didn't create typed func");

        Ok(verb_func.call(&mut store, 1).unwrap())
    }
}
