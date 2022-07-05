use sha2::Digest;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Error};
use futures::executor::block_on;
use futures::lock::Mutex;
use log::{error, info};
use rmp_serde::Serializer;
use serde::Serialize;
use tungstenite::Message;
use wasmtime::{self, Extern, Module, Trap, Val};

use crate::world::{send_connection_message, send_verb_dispatch, World};
use crate::{object::Program, object::Value};

pub struct WasmVM {
    wasm_linker: Arc<Mutex<wasmtime::Linker<VMState>>>,
    wasm_store: Arc<Mutex<wasmtime::Store<VMState>>>,
    module_cache: moka::future::Cache<Vec<u8>, wasmtime::Module>,
}

struct VMState {
    wasi: wasmtime_wasi::WasiCtx,
    world: Arc<World>,
}

// Argument 'stack frame' construction.
// Packs all arguments into the first N bytes of an instance's memory.
fn pack_args(
    mut store: &mut wasmtime::Store<VMState>,
    instance: &wasmtime::Instance,
    args: &Value,
) -> usize {
    // Messagepack the arguments to pass through.
    let mut args_buf = Vec::new();
    args.serialize(&mut Serializer::new(&mut args_buf))
        .expect("Unable to serialize arguments");

    // Fill module's memory offset 0 with the serialized arguments.
    let memory = instance
        .get_memory(store.deref_mut(), "memory")
        .expect("expected memory not found");

    memory
        .write(store.deref_mut(), 0, args_buf.as_slice())
        .expect("Could not write argument memory");

    args_buf.len()
}

fn pack_result(
    mut caller: &mut wasmtime::Caller<VMState>,
    stack_end: usize,
    result: &Value,
) -> Result<usize, Error> {
    let mut result_buf = Vec::new();
    result
        .serialize(&mut Serializer::new(&mut result_buf))
        .unwrap();
    let mem = &caller.get_export("memory").unwrap();
    match mem {
        Extern::Memory(mem) => {
            mem.write(caller.deref_mut(), stack_end, result_buf.as_slice())
                .expect("Could not write result memory");
            Ok(result_buf.len())
        }
        _ => {
            return Err(anyhow!("Invalid export for 'memory'"));
        }
    }
}

// Unpack arguments from a stack frame, used by builtins etc.
fn unpack_args(
    caller: &mut wasmtime::Caller<VMState>,
    params: &[wasmtime::Val],
) -> anyhow::Result<(Vec<Value>, usize)> {
    let mem = caller.get_export("memory").unwrap();
    let stack_end = match &params[0] {
        Val::I32(p) => *p as usize,
        _ => {
            return Err(anyhow!("Invalid stack_end argument"));
        }
    };
    match mem {
        Extern::Memory(mem) => {
            let _world = caller.data().world.clone();
            let mut buffer: Vec<u8> = vec![0; stack_end];
            mem.read(&caller, 0, &mut buffer).unwrap();

            let arguments: Value = rmp_serde::from_slice(buffer.as_slice()).unwrap();
            match arguments {
                Value::Vector(v) => Ok((v, stack_end)),
                _ => {
                    return Err(anyhow!("Invalid method arguments"));
                }
            }
        }
        _ => {
            return Err(anyhow!("Invalid export for 'memory'"));
        }
    }
}

impl WasmVM {
    pub fn new(world: Arc<World>) -> Result<Self, Error> {
        let mut config = wasmtime::Config::new();
        // We need this engine's `Store`s to be async, and consume fuel, so
        // that they can co-operatively yield during execution.
        config.async_support(true);
        config.consume_fuel(true);

        let engine = wasmtime::Engine::new(&config)?;
        let mut linker = wasmtime::Linker::new(&engine);

        wasmtime_wasi::add_to_linker(&mut linker, |state: &mut VMState| &mut state.wasi)?;

        let state = VMState {
            wasi: wasmtime_wasi::WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_args()?
                .build(),
            world,
        };
        let mut store = wasmtime::Store::new(&engine, state);

        // WebAssembly execution will be paused for an async yield every time it
        // consumes 10000 fuel. Fuel will be refilled u64::MAX times.
        store.out_of_fuel_async_yield(u64::MAX, 10000);

        let vm = WasmVM {
            wasm_linker: Arc::new(Mutex::new(linker)),
            wasm_store: Arc::new(Mutex::new(store)),
            module_cache: moka::future::Cache::builder()
                .time_to_live(Duration::from_secs(30 * 60))
                .build(),
        };
        Ok(vm)
    }

    pub fn bind_builtins(self: Arc<Self>) -> anyhow::Result<(), anyhow::Error> {
        let builtin_func_type = wasmtime::FuncType::new(
            Some(wasmtime::ValType::I32),
            vec![wasmtime::ValType::I32, wasmtime::ValType::I32].into_iter(),
        );

        let mut linker = block_on(self.wasm_linker.lock());
        let vm = self.clone();
        linker.func_new_async(
            "host",
            "invoke",
            builtin_func_type.clone(),
            move |mut caller, params, results| {
                let vm = vm.clone();

                Box::new(async move {
                    let vm = vm.clone();

                    let (arguments, stack_end) = unpack_args(&mut caller, params)?;
                    let (dest_oid, verb, arguments) = match &arguments[..] {
                        [oid, verb, args] => {
                            let oid = match oid {
                                Value::IdKey(id) => id,
                                _ => {
                                    return Err(Trap::new("Invalid destination"));
                                }
                            };
                            let verb = match verb {
                                Value::String(str) => str,
                                _ => {
                                    return Err(Trap::new("Invalid verb"));
                                }
                            };
                            let args = match args {
                                Value::Vector(a) => a,
                                _ => {
                                    return Err(Trap::new("Invalid verb arguments"));
                                }
                            };
                            (oid, verb, args)
                        }
                        _ => {
                            error!("Invalid 'invoke' arguments");
                            return Err(Trap::new("Invalid arguments"));
                        }
                    };
                    // How to dispatch... hmph.
                    let world = caller.data().world.clone();

                    // TODO results...
                    send_verb_dispatch(&world.clone(), vm, *dest_oid, verb.as_str(), arguments)
                        .await?;

                    let results_size = pack_result(&mut caller, stack_end, &Value::I32(0)).unwrap();
                    results[0] = Val::I32(stack_end as i32);
                    results[1] = Val::I32(results_size as i32);
                    Ok(())
                })
            },
        )?;

        linker.func_new_async(
            "host",
            "log",
            builtin_func_type.clone(),
            |mut caller, params, results| {
                Box::new(async move {
                    let (arguments, stack_end) = unpack_args(&mut caller, params)?;
                    info!("Log: {:?}", arguments);

                    let results_size = pack_result(&mut caller, stack_end, &Value::I32(0)).unwrap();
                    results[0] = Val::I32(stack_end as i32);
                    results[1] = Val::I32(results_size as i32);
                    Ok(())
                })
            },
        )?;

        linker.func_new_async(
            "host",
            "send",
            builtin_func_type,
            |mut caller, params, results| {
                Box::new(async move {
                    let (arguments, stack_end) = unpack_args(&mut caller, params)?;
                    let (cid, msg) = match &arguments[..] {
                        [oid, message] => {
                            let cid = match &oid {
                                Value::IdKey(oid) => oid,
                                _ => {
                                    error!("Invalid 'send' destination: {:?}", oid);
                                    return Err(Trap::new("Invalid arguments"));
                                }
                            };

                            let msg = match message {
                                Value::String(str) => Message::Text(str.clone()),
                                Value::Binary(bin) => Message::Binary(bin.clone()),
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
                    };
                    let world = caller.data().world.clone();
                    send_connection_message(world, *cid, msg).await?;

                    let results_size = pack_result(&mut caller, stack_end, &Value::I32(0)).unwrap();
                    results[0] = Val::I32(stack_end as i32);
                    results[1] = Val::I32(results_size as i32);
                    Ok(())
                })
            },
        )?;

        Ok(())
    }

    pub async fn execute(&self, method: &Program, args: &Value) -> Result<(), anyhow::Error> {
        // Check to see if we have a cached copy of the compiled Module for this Program, using
        // a Sha512 digest of the program text.
        // (Should probably profile this because perhaps in some cases taking the hash could be
        // costlier than just compiling.)
        let digest = sha2::Sha512::digest(method.as_slice());
        let module = self
            .module_cache
            .get_with(digest.to_vec(), async move {
                let store = self.wasm_store.lock().await;

                Module::new(store.engine(), method).expect("Not able to produce WASM module")
            })
            .await;

        // We'll be holding a lock on the actual 'store' throughout execution.
        // This defacto enforces single-threaded single file access per connection
        // But I think this is ok for our purposes.
        let mut store = self.wasm_store.lock().await;

        // Use the linker to produce an instance from the module.
        let instance = {
            let linker = self.wasm_linker.lock().await;
            linker
                .instantiate_async(store.deref_mut(), &module)
                .await
                .unwrap()
        };

        // Build the 'stack frame'. Pack args into module's memory.
        let args_len = pack_args(store.deref_mut(), &instance, args);

        // Retrieve the linked function from the instance and call it.
        let verb_func = instance
            .get_typed_func::<i32, (i32, i32), _>(store.deref_mut(), "invoke")
            .expect("Didn't create typed func");

        // Invocation argument is the length of the argument buffer in memory.
        let _result = verb_func
            .call_async(store.deref_mut(), args_len as i32)
            .await?;

        Ok(())
    }
}
