//! Keeps a second copy of Scrollify from starting.

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct InstanceLock(HANDLE);

impl InstanceLock {
    /// Returns `None` when another instance already holds the lock.
    pub fn acquire() -> Option<Self> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Local\\Scrollify.SingleInstance")).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(Self(handle))
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
