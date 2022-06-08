use rusty_v8 as v8;
use std::error::Error;

use crate::{object::Program, vm::VM, world::World};
use log::info;
use std::str;

pub struct V8VM {}

impl VM for V8VM {
    fn execute(&self, method: &Program, _world: &dyn World) -> Result<(), Box<dyn Error>> {
        // TODO Creating this all from scratch each time is dubious from a performance POV.
        // We need a way to stash at least some of this per-connection, or per transaction, or
        // something.
        // TODO wire up some builtins here.
        let isolate = &mut v8::Isolate::new(Default::default());
        let handle_scope = &mut v8::HandleScope::new(isolate);
        let context = v8::Context::new(handle_scope);
        let scope = &mut v8::ContextScope::new(handle_scope, context);
        let program_str = str::from_utf8(method.method.as_ref());
        let code = v8::String::new(scope, program_str.unwrap()).unwrap();
        info!("javascript code: {}", code.to_rust_string_lossy(scope));
        let script = v8::Script::compile(scope, code, None).unwrap();
        let result = script.run(scope).unwrap();
        let result = result.to_string(scope).unwrap();

        info!("result: {}", result.to_rust_string_lossy(scope));

        Ok(())
    }
}

impl V8VM {
    pub fn new() -> Box<dyn VM + Send + Sync> {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();

        Box::new(V8VM {})
    }
}
