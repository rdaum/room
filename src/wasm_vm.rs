use anyhow::Error;
use std::sync::Arc;

use futures::future::{BoxFuture, FutureExt};
use futures::lock::Mutex;
use log::info;
use rmp_serde::{Serializer};
use serde::Serialize;
use wasmtime::{self, Extern, Module};
use wasmtime::Extern::Func;
use wasmtime_wasi::sync::WasiCtxBuilder;

use crate::{object::Program, object::Value, vm::VM};
use crate::world::World;

pub struct WasmVM<'vm_lifetime> {
    wasm_engine: wasmtime::Engine,
    wasm_linker: Arc<wasmtime::Linker<VMState<'vm_lifetime>>>,
}

struct VMState<'vm_lifetime> {
    wasi: wasmtime_wasi::WasiCtx,
    vm: &'vm_lifetime WasmVM<'vm_lifetime>,
    world: Arc<World>
}

impl<'vm_lifetime> VM for WasmVM<'vm_lifetime> {
    fn execute(
        &self,
        method: &Program,
        world:  Arc<World>,
        args: &Value,
    ) -> BoxFuture<Result<(), anyhow::Error>> {
        // Copy the method program before entering the closure.
        let bytes = method.clone();

        // Messagepack the arguments to pass through.
        let mut args_buf = Vec::new();
        args.serialize(&mut Serializer::new(&mut args_buf))
            .expect("Unable to serialize arguments");

        async move {
            let state = VMState {
                wasi:  wasmtime_wasi::WasiCtxBuilder::new()
                    .inherit_stdio()
                    .inherit_args()?
                    .build(),
                vm: self,
                world: world.clone()
            };
            let mut store = wasmtime::Store::new(&self.wasm_engine, state);

            // WebAssembly execution will be paused for an async yield every time it
            // consumes 10000 fuel. Fuel will be refilled u64::MAX times.
            store.out_of_fuel_async_yield(u64::MAX, 10000);

            let module =
                Module::new(&self.wasm_engine, bytes).expect("Not able to produce WASM module");
            let instance = self
                .wasm_linker
                .instantiate_async(&mut store, &module)
                .await?;

            // Fill module's memory offset 0 with the serialized arguments.
            let memory = &instance
                .get_memory(&mut store, "memory")
                .expect("expected memory not found");
            memory
                .write(&mut store, 0, args_buf.as_slice())
                .expect("Could not write argument memory");

            let verb_func = instance
                .get_typed_func::<i32, (), _>(&mut store, "invoke")
                .expect("Didn't create typed func");

            // Invocation argument is the length of the arguments in memory.
            verb_func
                .call_async(&mut store, args_buf.len() as i32)
                .await?;
            Ok(())
        }
        .boxed()
    }
}

impl<'vm_lifetime> WasmVM<'vm_lifetime> {
    pub fn new() -> Result<Self, Error> {
        let mut config = wasmtime::Config::new();
        // We need this engine's `Store`s to be async, and consume fuel, so
        // that they can co-operatively yield during execution.
        config.async_support(true);
        config.consume_fuel(true);

        let engine = wasmtime::Engine::new(&config)?;
        let mut linker = wasmtime::Linker::new(&engine);

        wasmtime_wasi::add_to_linker(&mut linker, |state: &mut VMState| &mut state.wasi)?;

        let into_func = move |mut caller: wasmtime::Caller<'_, VMState>, param: i32| {
            let mem = caller.get_export("memory").unwrap();
            match mem {
                Func(_) => {}
                Extern::Global(_) => {}
                Extern::Table(_) => {}
                Extern::Memory(mem) => {
                    let stack_end = param as usize;
                    let mut buffer: Vec<u8> = vec![0; stack_end as usize];
                    mem.read(&caller, 0, &mut buffer).unwrap();
                    let result : Value = rmp_serde::from_slice(buffer.as_slice()).unwrap();
                    info!("Log: {:?}", result);
                }
            }


            Ok(())
        };


        linker
            .func_wrap("host", "log", into_func)
            .expect("Unable to link externals");

        Ok(WasmVM {
            wasm_engine: engine,
            wasm_linker: Arc::new(linker),
        })
    }
}
