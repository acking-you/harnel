//! Minimal ownership wrapper around the versioned fx C ABI.
//!
//! Native queues synchronize concurrent writes, reads, and close. Exactly one
//! reader is allowed by the higher-level broker. Destruction requires exclusive
//! ownership and joins all native workers before freeing the handle.
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    fmt,
    ptr::NonNull,
};

unsafe extern "C" {
    fn fx_abi_version() -> u32;
    fn fx_revision() -> *const c_char;
    fn fx_last_error() -> *const c_char;
    fn fx_runtime_create(config: *const u8, len: usize, output: *mut *mut c_void) -> c_int;
    fn fx_runtime_write(handle: *mut c_void, bytes: *const u8, len: usize) -> c_int;
    fn fx_runtime_read(
        handle: *mut c_void,
        bytes: *mut u8,
        len: usize,
        written: *mut usize,
    ) -> c_int;
    fn fx_runtime_close(handle: *mut c_void);
    fn fx_runtime_exit_code(handle: *mut c_void) -> u32;
    fn fx_runtime_destroy(handle: *mut c_void);
}

#[derive(Debug)]
pub struct Error {
    pub code: i32,
    pub message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native runtime error {}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

fn check(code: i32) -> Result<(), Error> {
    if code == 0 {
        return Ok(());
    }
    // SAFETY: fx returns a thread-local, NUL-terminated static buffer. Copy it
    // on the calling thread before another FFI operation can replace it.
    let message = unsafe { CStr::from_ptr(fx_last_error()) }
        .to_string_lossy()
        .into_owned();
    Err(Error { code, message })
}

pub fn revision() -> &'static str {
    // SAFETY: revision points to an immutable NUL-terminated static string.
    unsafe { CStr::from_ptr(fx_revision()) }
        .to_str()
        .unwrap_or("unknown")
}

pub struct Runtime(NonNull<c_void>);
// SAFETY: the C runtime synchronizes its queues; moving ownership does not
// move the allocation. Drop only runs after all Rust borrows/Arc owners end.
unsafe impl Send for Runtime {}
// SAFETY: read, write, close, and exit_code use native synchronization. No
// mutable native state is exposed to callers and destroy is exclusive.
unsafe impl Sync for Runtime {}

impl Runtime {
    pub fn new(config_json: &[u8]) -> Result<Self, Error> {
        // SAFETY: no pointer arguments; the version is a compile-time constant.
        if unsafe { fx_abi_version() } != 1 {
            return Err(Error {
                code: 1,
                message: "unsupported fx ABI version".into(),
            });
        }
        let mut handle = std::ptr::null_mut();
        // SAFETY: config is readable for its length and output is writable.
        // Native creation copies the configuration before returning.
        check(unsafe { fx_runtime_create(config_json.as_ptr(), config_json.len(), &mut handle) })?;
        NonNull::new(handle).map(Self).ok_or_else(|| Error {
            code: 1,
            message: "native creation returned a null handle".into(),
        })
    }
    pub fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        // SAFETY: handle lives for this borrow; native write copies bytes.
        check(unsafe { fx_runtime_write(self.0.as_ptr(), bytes.as_ptr(), bytes.len()) })
    }
    pub fn read(&self, bytes: &mut [u8]) -> Result<usize, Error> {
        let mut written = 0;
        // SAFETY: destination and count are writable; native respects capacity.
        check(unsafe {
            fx_runtime_read(
                self.0.as_ptr(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut written,
            )
        })?;
        Ok(written)
    }
    pub fn close(&self) {
        // SAFETY: close is idempotent, synchronized, and never frees the handle.
        unsafe { fx_runtime_close(self.0.as_ptr()) }
    }
    pub fn exit_code(&self) -> u32 {
        // SAFETY: exit code is atomic and handle remains live.
        unsafe { fx_runtime_exit_code(self.0.as_ptr()) }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: exclusive final ownership; destroy closes queues and joins
        // workers, then releases all handle-owned memory exactly once.
        unsafe { fx_runtime_destroy(self.0.as_ptr()) }
    }
}
