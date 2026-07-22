use crate::machine::error::{MachineError, Result};

/// A page-backed allocation holding generated machine code. The memory is
/// writable only during construction; it is remapped read-execute before any
/// code pointer escapes, so W^X platforms are satisfied.
pub(super) struct ExecutableMemory {
    ptr: *mut u8,
    length: usize,
}

unsafe impl Send for ExecutableMemory {}
unsafe impl Sync for ExecutableMemory {}

impl ExecutableMemory {
    pub(super) fn new(code: &[u8]) -> Result<Self> {
        let length = code.len();
        let ptr = allocate(length)?;
        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, length);
        }
        protect(ptr, length)?;
        flush(ptr, length);
        Ok(Self { ptr, length })
    }

    pub(super) fn as_ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        release(self.ptr, self.length);
    }
}

#[cfg(unix)]
fn allocate(length: usize) -> Result<*mut u8> {
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            length,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(MachineError::MemoryUnavailable);
    }
    Ok(ptr as *mut u8)
}

#[cfg(unix)]
fn protect(ptr: *mut u8, length: usize) -> Result<()> {
    let status = unsafe {
        libc::mprotect(
            ptr as *mut libc::c_void,
            length,
            libc::PROT_READ | libc::PROT_EXEC,
        )
    };
    if status != 0 {
        return Err(MachineError::MemoryUnavailable);
    }
    Ok(())
}

#[cfg(unix)]
fn release(ptr: *mut u8, length: usize) {
    unsafe {
        libc::munmap(ptr as *mut libc::c_void, length);
    }
}

#[cfg(windows)]
mod kernel32 {
    use core::ffi::c_void;

    pub const MEM_COMMIT: u32 = 0x1000;
    pub const MEM_RESERVE: u32 = 0x2000;
    pub const MEM_RELEASE: u32 = 0x8000;
    pub const PAGE_READWRITE: u32 = 0x04;
    pub const PAGE_EXECUTE_READ: u32 = 0x20;

    unsafe extern "system" {
        pub fn VirtualAlloc(
            address: *mut c_void,
            size: usize,
            allocation: u32,
            protect: u32,
        ) -> *mut c_void;
        pub fn VirtualProtect(
            address: *mut c_void,
            size: usize,
            protect: u32,
            previous: *mut u32,
        ) -> i32;
        pub fn VirtualFree(address: *mut c_void, size: usize, free: u32) -> i32;
        pub fn FlushInstructionCache(
            process: *mut c_void,
            address: *const c_void,
            size: usize,
        ) -> i32;
        pub fn GetCurrentProcess() -> *mut c_void;
    }
}

#[cfg(windows)]
fn allocate(length: usize) -> Result<*mut u8> {
    let ptr = unsafe {
        kernel32::VirtualAlloc(
            std::ptr::null_mut(),
            length,
            kernel32::MEM_COMMIT | kernel32::MEM_RESERVE,
            kernel32::PAGE_READWRITE,
        )
    };
    if ptr.is_null() {
        return Err(MachineError::MemoryUnavailable);
    }
    Ok(ptr as *mut u8)
}

#[cfg(windows)]
fn protect(ptr: *mut u8, length: usize) -> Result<()> {
    let mut previous = 0;
    let status = unsafe {
        kernel32::VirtualProtect(
            ptr as *mut core::ffi::c_void,
            length,
            kernel32::PAGE_EXECUTE_READ,
            &mut previous,
        )
    };
    if status == 0 {
        return Err(MachineError::MemoryUnavailable);
    }
    Ok(())
}

#[cfg(windows)]
fn release(ptr: *mut u8, _length: usize) {
    unsafe {
        kernel32::VirtualFree(ptr as *mut core::ffi::c_void, 0, kernel32::MEM_RELEASE);
    }
}

#[cfg(windows)]
fn flush(ptr: *mut u8, length: usize) {
    unsafe {
        kernel32::FlushInstructionCache(
            kernel32::GetCurrentProcess(),
            ptr as *const core::ffi::c_void,
            length,
        );
    }
}

#[cfg(all(unix, target_arch = "x86_64"))]
fn flush(_ptr: *mut u8, _length: usize) {}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn flush(ptr: *mut u8, length: usize) {
    unsafe extern "C" {
        fn sys_icache_invalidate(start: *mut core::ffi::c_void, size: usize);
    }
    unsafe {
        sys_icache_invalidate(ptr as *mut core::ffi::c_void, length);
    }
}

#[cfg(all(unix, not(target_os = "macos"), target_arch = "aarch64"))]
fn flush(ptr: *mut u8, length: usize) {
    unsafe extern "C" {
        fn __clear_cache(start: *mut core::ffi::c_char, end: *mut core::ffi::c_char);
    }
    unsafe {
        __clear_cache(
            ptr as *mut core::ffi::c_char,
            ptr.add(length) as *mut core::ffi::c_char,
        );
    }
}
