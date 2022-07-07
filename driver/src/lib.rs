#![no_std]
#![allow(unused_attributes)]

/// WASM-side ABI / heap memory mgmt
/// Management of heap-arguments
//

// We need alloc for Vec (and probably String).
// We will probably need to define our own custom allocator for WASM-land.
// For now we will proceed using the one provided by alloc::System
extern crate alloc;

use core::slice::from_raw_parts;
use value::Error::NoError;
use value::Value;

#[link(wasm_import_module = "host")]
extern "C" {
    static memory: *mut u8;
    static mut __data_end: i32;
    static __heap_base: i32;
    fn log(stack_end: i32) -> i32;
}

/// static_end is the offset into memory of where memory passed into WASM-land from the runtime
/// ends.
/// action is the function to invoke with arguments passed.
/// the contents of this region are deserialized into a call record structure containing all
/// arguments and any additional context from the runtime.
/// from there, any heap allocations are performed above that wall and
/// the intended function is then dispatched with the deserialized arguments passed through
/// using rust's wasm calling conventions.
/// finally the return back to the runtime is a tuple containing the offset and size of the
/// returned values.
fn trampoline<F>(static_end: i32, action: F) -> (i32, i32)
where
    F: Fn(&Value) -> Value,
{
    let value: Value = unsafe {
        let tramp_args = from_raw_parts(memory, static_end as usize);
        postcard::from_bytes(tramp_args).unwrap()
    };
    let result = action(&value);
    unsafe {
        let tramp_res = postcard::to_allocvec(&result).unwrap();
        let (offset, size) = (__heap_base, tramp_res.len() as i32);
        let region = memory.offset(offset as isize);
        region.copy_from(region, size as usize);
        (offset, size)
    }
}

#[no_mangle]
pub extern "C" fn syslog(static_end: i32) -> (i32, i32) {
    trampoline(static_end, |_v| Value::Error(NoError))
}
