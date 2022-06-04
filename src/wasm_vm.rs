use std::error::Error;
use wasmtime::{self, Module};

use crate::{object::Method, vm::VM, world::World};

pub struct WasmVM<'vm_lifetime> {
    wasm_engine: wasmtime::Engine,
    wasm_linker: wasmtime::Linker<&'vm_lifetime WasmVM<'vm_lifetime>>,
}

impl<'vm_lifetime> VM for WasmVM<'vm_lifetime> {
    fn execute_method(&self, method: &Method, _world: &dyn World) -> Result<(), Box<dyn Error>> {
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

        verb_func.call(&mut store, 1).unwrap();
        Ok(())
    }
}

impl<'vm_lifetime> WasmVM<'vm_lifetime> {
    pub fn new() -> Box<dyn VM + Send + Sync + 'vm_lifetime> {
        let engine = wasmtime::Engine::default();
        let mut linker: wasmtime::Linker<&WasmVM<'vm_lifetime>> = wasmtime::Linker::new(&engine);

        let into_func = |_caller: wasmtime::Caller<'_, &WasmVM<'vm_lifetime>>, param: i32| {
            println!("Got {:?} from WebAssembly", param);
        };

        linker
            .func_wrap("host", "log", into_func)
            .expect("Unable to link externals");

        Box::new(WasmVM {
            wasm_engine: engine,
            wasm_linker: linker,
        })
    }
}
