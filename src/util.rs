use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

pub fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}

pub fn hiword(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}
