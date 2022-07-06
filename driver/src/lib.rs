#![no_std]
#![allow(unused_attributes)]

extern crate alloc;

use value::Value;

#[link(wasm_import_module = "host")]
extern "C" {
    fn log(s: &str) -> Value;
    fn send(c: &value::Oid, s: &str) -> value::Value;
    fn get_slot(l: &value::Oid, k: &value::Oid, n: &str) -> value::Value;
    fn set_slot(l: &value::Oid, k: &value::Oid, n: &str, v: &value::Value) -> value::Value;
}

#[no_mangle]
pub fn syslog(s: &Value) -> value::Value {
    match s {
        Value::String(s) => unsafe { log(s.as_str()) },
        _ => value::Value::Error(value::Error::BadType),
    }
}
