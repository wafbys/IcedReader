//! Send a library file to the Windows Recycle Bin instead of deleting it, so a
//! shelf deletion stays recoverable without any UI of our own.
//!
//! Windows-only reader; other targets (cargo test on non-Windows, plain
//! `cargo check` tooling) fall back to a normal delete.

use std::path::Path;

/// Move one file to the Recycle Bin. A file that is already gone is a no-op.
pub fn send(path: &Path) -> crate::error::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    platform_send(path)
}

// `cfg(test)` keeps unit tests from dumping their temp fixtures into the real
// Recycle Bin; the tests only care that the file is gone.
#[cfg(all(windows, not(test)))]
fn platform_send(path: &Path) -> crate::error::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    // `pFrom` is a NUL-separated list of paths, ended by an extra NUL.
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        pTo: PCWSTR::null(),
        // ALLOWUNDO is what routes the delete through the Recycle Bin.
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0) as u16,
        fAnyOperationsAborted: BOOL::from(false),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };

    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(format!("移到回收站失败（代码 {code}）：{}", path.display()).into());
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err(format!("移到回收站被取消：{}", path.display()).into());
    }
    Ok(())
}

#[cfg(any(not(windows), test))]
fn platform_send(path: &Path) -> crate::error::Result<()> {
    std::fs::remove_file(path)?;
    Ok(())
}
