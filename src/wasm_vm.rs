
use std::ops::DerefMut;
use std::sync::Arc;

use anyhow::Error;

use futures::lock::Mutex;
use log::{error, info};

use rmp_serde::Serializer;
use serde::Serialize;
use tungstenite::Message;
use wasmtime::{self, Extern, Module, Trap, Val};


use crate::{object::Program, object::Value};
use crate::world::{send_connection_message, World};

pub struct WasmVM {
    wasm_linker: Arc<wasmtime::Linker<VMState>>,
    wasm_store: Arc<Mutex<wasmtime::Store<VMState>>>,
}

struct VMState {
    wasi: wasmtime_wasi::WasiCtx,
    world: Arc<World>,
}

impl WasmVM {
    pub fn new(world: Arc<World>,
    ) -> Result<Self, Error> {
        let mut config = wasmtime::Config::new();
        // We need this engine's `Store`s to be async, and consume fuel, so
        // that they can co-operatively yield during execution.
        config.async_support(true);
        config.consume_fuel(true);

        let engine = wasmtime::Engine::new(&config)?;
        let mut linker = wasmtime::Linker::new(&engine);

        wasmtime_wasi::add_to_linker(&mut linker, |state: &mut VMState| &mut state.wasi)?;

        let builtin_func_type = wasmtime::FuncType::new(
            Some(wasmtime::ValType::I32),
            None,
        );

        let state = VMState {
            wasi: wasmtime_wasi::WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_args()?
                .build(),
            world,
        };
        let mut store = wasmtime::Store::new(&engine, state);

        linker.func_new_async("host", "log", builtin_func_type.clone(), |mut caller, params,_results| {Box::new( async move {
            let mem = caller.get_export("memory").unwrap();
            match mem {
                Extern::Func(_) => {}
                Extern::Global(_) => {}
                Extern::Table(_) => {}
                Extern::Memory(mem) => {
                    match &params[0] {
                        Val::I32(param) => {
                            let stack_end = *param as usize;
                            let mut buffer: Vec<u8> = vec![0; stack_end as usize];
                            mem.read(&caller, 0, &mut buffer).unwrap();
                            let result: Value = rmp_serde::from_slice(buffer.as_slice()).unwrap();
                            info!("Log: {:?}", result);
                        }
                        _ => {}
                    }
                }
            }

            Ok(())
        })})?;


        linker.func_new_async("host", "send", builtin_func_type, |mut caller, params,_results| {Box::new( async move {
            let mem = caller.get_export("memory").unwrap();
            let stack_end = match &params[0] {
                Val::I32(p) => {*p as usize}
                _ => {
                    return Err(Trap::new("Invalid arguments"));
                }
            };
            match mem {
                Extern::Func(_) => {}
                Extern::Global(_) => {}
                Extern::Table(_) => {}
                Extern::Memory(mem) => {
                    let world = caller.data().world.clone();
                    let mut buffer: Vec<u8> = vec![0; stack_end as usize];
                    mem.read(&caller, 0, &mut buffer).unwrap();

                    let arguments: Value = rmp_serde::from_slice(buffer.as_slice()).unwrap();

                    let (cid, msg) = match &arguments {
                        Value::Vector(args) => {

                            match &args[..] {
                                [oid, message] => {

                                    let cid = match &oid {
                                        Value::IdKey(oid) => {
                                            oid
                                        }
                                        _ => {
                                            error!("Invalid 'send' destination: {:?}", oid);
                                            return Err(Trap::new("Invalid arguments"));
                                        }
                                    };

                                    let msg = match message {
                                        Value::String(str) => {
                                            Message::Text(str.clone())
                                        }
                                        Value::Binary(bin) => {
                                            Message::Binary(bin.clone())
                                        }
                                        _ => {
                                            error!("Invalid arguments to 'send': {:?}", arguments);
                                            return Err(Trap::new("Invalid arguments"));
                                        }
                                    };

                                    (cid, msg)
                                }
                                _ => {
                                    error!("Invalid arguments to 'send': {:?}", arguments);
                                    return Err(Trap::new("Invalid arguments"));
                                }
                            }
                        }
                        _ => {
                            error!("Invalid arguments to 'send': {:?}", arguments);
                            return Err(Trap::new("Invalid arguments"));
                        }
                    };
                    send_connection_message(world, *cid, msg).await?;
                }
            }
            Ok(())
        })})?;


        // WebAssembly execution will be paused for an async yield every time it
        // consumes 10000 fuel. Fuel will be refilled u64::MAX times.
        store.out_of_fuel_async_yield(u64::MAX, 10000);

        Ok(WasmVM {
            wasm_linker: Arc::new(linker),
            wasm_store: Arc::new(Mutex::new(store)),
        })
    }

    pub async fn execute(
        &self,
        method: &Program,
        args: &Value,
    ) -> Result<(), anyhow::Error> {
        // Copy the method program before entering the closure.
        let bytes = method.clone();

        // Messagepack the arguments to pass through.
        let mut args_buf = Vec::new();
        args.serialize(&mut Serializer::new(&mut args_buf))
            .expect("Unable to serialize arguments");

        let mut store = self.wasm_store.lock().await;
        let module = Module::new(store.engine(), bytes).expect("Not able to produce WASM module");
        let instance = self
            .wasm_linker
            .instantiate_async(store.deref_mut(), &module)
            .await
            .unwrap();

        // Fill module's memory offset 0 with the serialized arguments.
        let memory = &instance
            .get_memory(store.deref_mut(), "memory")
            .expect("expected memory not found");

        memory
            .write(store.deref_mut(), 0, args_buf.as_slice())
            .expect("Could not write argument memory");


        let verb_func = instance
            .get_typed_func::<i32, (), _>(store.deref_mut(), "invoke")
            .expect("Didn't create typed func");

        // Invocation argument is the length of the arguments in memory.
        verb_func
            .call_async(store.deref_mut(), args_buf.len() as i32)
            .await?;

        Ok(())
    }
}

