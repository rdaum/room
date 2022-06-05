use anyhow::Error;
use std::sync::Arc;

use futures::future::{BoxFuture, FutureExt};
use wasmtime::{self, Module};

use crate::{object::Method, vm::VM, world::World};

pub struct WasmVM<'vm_lifetime> {
    wasm_engine: wasmtime::Engine,
    wasm_linker: Arc<wasmtime::Linker<&'vm_lifetime WasmVM<'vm_lifetime>>>,
}

impl<'vm_lifetime> VM for WasmVM<'vm_lifetime> {
    fn execute_method(
        &self,
        method: &Method,
        _world: &(dyn World + Send + Sync),
    ) -> BoxFuture<Result<(), anyhow::Error>> {
        let bytes = method.method.clone();
        async move {
            let mut store = wasmtime::Store::new(&self.wasm_engine, self);

            // WebAssembly execution will be paused for an async yield every time it
            // consumes 10000 fuel. Fuel will be refilled u64::MAX times.
            store.out_of_fuel_async_yield(u64::MAX, 10000);

            let module =
                Module::new(&self.wasm_engine, bytes).expect("Not able to produce WASM module");
            let instance = self
                .wasm_linker
                .instantiate_async(&mut store, &module)
                .await?;

            let verb_func = instance
                .get_typed_func::<i32, (), _>(&mut store, "invoke")
                .expect("Didn't create typed func");

            verb_func.call_async(&mut store, 1).await?;
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
        let mut linker: wasmtime::Linker<&WasmVM> = wasmtime::Linker::new(&engine);

        let into_func = |_caller: wasmtime::Caller<'_, &WasmVM>, param: i32| {
            println!("Got {:?} from WebAssembly", param);
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
