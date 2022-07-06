#![no_std]
#![allow(unused_attributes)]

#[cfg(not(test))]
use core::panic::PanicInfo;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[link(wasm_import_module = "host")]
extern "C" {
    fn log(s: &str) -> i32;
}

#[no_mangle]
pub fn syslog(s: &str) -> i32 {
    unsafe { log(s) }
}
