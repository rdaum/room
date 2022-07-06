// Note: This doesn't work yet, symbol mangling isn't right.
// Just here to exercise the compiler.

extern "C" {
    pub fn host_log(s: &str);
}

pub unsafe fn log(s: &str) {
    host_log(s);
}
