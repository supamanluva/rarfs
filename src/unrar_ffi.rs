#![allow(non_snake_case)]

use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use std::ptr;

pub const ERAR_END_ARCHIVE: i32 = 10;

pub type ProcessCallback = extern "C" fn(user_data: usize, data: *const u8, len: usize) -> i32;

unsafe extern "C" {
    fn rarfs_open(path: *const c_char, err_out: *mut i32) -> *mut c_void;
    fn rarfs_read_next_name(handle: *mut c_void, buf: *mut c_char, buf_size: usize) -> i32;
    fn rarfs_skip_current(handle: *mut c_void) -> i32;
    fn rarfs_process_current(handle: *mut c_void) -> i32;
    fn rarfs_set_callback(handle: *mut c_void, cb: ProcessCallback, user_data: usize);
    fn rarfs_close(handle: *mut c_void) -> i32;
}

pub struct UnrarArchive(ptr::NonNull<c_void>);

impl UnrarArchive {
    pub fn open(path: &Path) -> Result<UnrarArchive, i32> {
        let c = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| -1)?;
        let mut err = 0i32;
        let h = unsafe { rarfs_open(c.as_ptr(), &mut err) };
        match ptr::NonNull::new(h) {
            Some(p) => Ok(UnrarArchive(p)),
            None => Err(err),
        }
    }

    /// Copies the current header's file name into `buf`; returns its length.
    /// Err(ERAR_END_ARCHIVE) when no more headers.
    pub fn read_next_name(&self, buf: &mut [u8]) -> Result<usize, i32> {
        let r = unsafe {
            rarfs_read_next_name(self.0.as_ptr(), buf.as_mut_ptr() as *mut c_char, buf.len())
        };
        if r != 0 {
            return Err(r);
        }
        Ok(buf.iter().position(|&b| b == 0).unwrap_or(buf.len()))
    }

    pub fn skip_current(&self) -> Result<(), i32> {
        let r = unsafe { rarfs_skip_current(self.0.as_ptr()) };
        if r == 0 { Ok(()) } else { Err(r) }
    }

    pub fn process_current(&self) -> Result<(), i32> {
        let r = unsafe { rarfs_process_current(self.0.as_ptr()) };
        if r == 0 { Ok(()) } else { Err(r) }
    }

    pub fn set_callback(&self, cb: ProcessCallback, user_data: usize) {
        unsafe { rarfs_set_callback(self.0.as_ptr(), cb, user_data) }
    }
}

impl Drop for UnrarArchive {
    fn drop(&mut self) {
        unsafe { rarfs_close(self.0.as_ptr()) };
    }
}

// The handle is used from exactly one decode thread at a time by UnrarReader.
unsafe impl Send for UnrarArchive {}
