//! Win32 HID transport for the Basilisk V3 vendor control interface.
//!
//! This module knows how to find the right interface (`MI_03`), open it, and
//! exchange 90-byte feature reports with BUSY retries. It deliberately does not
//! know what the bytes mean — that is `razer-protocol`'s job.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::thread::sleep;
use std::time::Duration;

use razer_protocol::report::{Report, Status, REPORT_LEN};
use razer_protocol::{CONTROL_INTERFACE, PRODUCT_ID, VENDOR_ID};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid, HidD_SetFeature, HIDD_ATTRIBUTES,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// Windows HID report for this device is 90 bytes plus the report id.
const FEATURE_LEN: usize = REPORT_LEN + 1;

#[derive(Debug)]
pub enum Error {
    /// No matching HID interface is plugged in.
    NotFound,
    /// The OS refused to give us the handle (usually a share conflict).
    Open { code: u32 },
    /// A feature transfer failed.
    Transfer { code: u32 },
    /// The device kept saying BUSY past the retry budget.
    Busy,
    /// The device answered with a non-OK status.
    Device { status: Status, class: u8, command: u8 },
    /// The reply did not answer the request: something else owns the device.
    Desync { expected: (u8, u8), got: (u8, u8) },
    /// Device answered, but its payload cannot be parsed. Distinct from a
    /// transport failure: previously these were reported as win32 error 0,
    /// which sent everybody looking at the wrong layer.
    Parse(&'static str),
    /// A local config-file operation failed (backup / restore / token).
    Io(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "no Basilisk V3 control interface found"),
            Error::Open { code } => write!(f, "cannot open the device (win32 error {code})"),
            Error::Transfer { code } => write!(f, "HID transfer failed (win32 error {code})"),
            Error::Busy => write!(f, "device stayed busy"),
            Error::Device { status, class, command } => write!(
                f,
                "device replied {status:?} to class 0x{class:02x} command 0x{command:02x}"
            ),
            Error::Desync { expected, got } => write!(
                f,
                "reply mismatch: asked 0x{:02x}/0x{:02x}, got 0x{:02x}/0x{:02x} \
                 (another program may be using the mouse)",
                expected.0, expected.1, got.0, got.1
            ),
            Error::Parse(what) => write!(f, "cannot parse device reply: {what}"),
            Error::Io(e) => write!(f, "config file error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// One open handle to the vendor interface.
pub struct Device {
    handle: HANDLE,
    pub path: String,
}

impl Device {
    /// Find and open the control interface.
    pub fn open() -> Result<Self> {
        let path = find_path()?.ok_or(Error::NotFound)?;
        Self::open_path(&path)
    }

    pub fn open_path(path: &str) -> Result<Self> {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(Error::Open { code: unsafe { GetLastError() } });
        }
        Ok(Device { handle, path: path.to_string() })
    }

    /// Send one request and wait for its reply, retrying while BUSY.
    pub fn call(&self, request: &Report) -> Result<Report> {
        self.send(request)?;
        // The first poll usually lands while the device is still working; the
        // delay grows so a slow write does not cost two dozen instant polls.
        for attempt in 0..24u32 {
            sleep(Duration::from_millis(2 + attempt as u64 * 3));
            let reply = self.recv()?;
            if !reply.answers(request) {
                return Err(Error::Desync {
                    expected: (request.command_class, request.command_id),
                    got: (reply.command_class, reply.command_id),
                });
            }
            match reply.status {
                Status::Ok => return Ok(reply),
                Status::Busy => continue,
                other => {
                    return Err(Error::Device {
                        status: other,
                        class: reply.command_class,
                        command: reply.command_id,
                    })
                }
            }
        }
        Err(Error::Busy)
    }

    /// Read the device's HID attributes: `(vid, pid, version)`.
    pub fn hid_attributes(&self) -> Option<(u16, u16, u16)> {
        let mut attrs: HIDD_ATTRIBUTES = unsafe { std::mem::zeroed() };
        attrs.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
        let ok = unsafe { HidD_GetAttributes(self.handle, &mut attrs) };
        if ok == 0 {
            None
        } else {
            Some((attrs.VendorID, attrs.ProductID, attrs.VersionNumber))
        }
    }

    fn send(&self, request: &Report) -> Result<()> {
        let buffer = request.to_feature_buffer();
        let ok = unsafe { HidD_SetFeature(self.handle, buffer.as_ptr() as *const core::ffi::c_void, FEATURE_LEN as u32) };
        if ok == 0 {
            return Err(Error::Transfer { code: unsafe { GetLastError() } });
        }
        Ok(())
    }

    fn recv(&self) -> Result<Report> {
        let mut buffer = [0u8; FEATURE_LEN];
        let ok = unsafe { HidD_GetFeature(self.handle, buffer.as_mut_ptr() as *mut core::ffi::c_void, FEATURE_LEN as u32) };
        if ok == 0 {
            return Err(Error::Transfer { code: unsafe { GetLastError() } });
        }
        // Drop the leading report id before parsing.
        Report::from_bytes(&buffer[1..]).ok_or(Error::Transfer { code: 0 })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

/// Path of the vendor control interface, if present.
pub fn find_path() -> Result<Option<String>> {
    let paths = all_hid_paths()?;
    for path in &paths {
        if let Some((vid, pid, mi)) = parse_hid_path(path) {
            if vid == VENDOR_ID && pid == PRODUCT_ID && mi == Some(CONTROL_INTERFACE) {
                return Ok(Some(path.clone()));
            }
        }
    }
    Ok(paths
        .into_iter()
        .find(|p| is_control_port(p) && p.to_ascii_lowercase().contains("vid_1532")))
}

/// Every HID interface path belonging to a Basilisk V3, control port first.
pub fn list_paths() -> Result<Vec<String>> {
    let mut out: Vec<String> = all_hid_paths()?
        .into_iter()
        .filter(|p| parse_hid_path(p).map(|(v, i, _)| v == VENDOR_ID && i == PRODUCT_ID).unwrap_or(false))
        .collect();
    out.sort_by_key(|p| if is_control_port(p) { 0 } else { 1 });
    Ok(out)
}

fn is_control_port(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("mi_03")
}

fn all_hid_paths() -> Result<Vec<String>> {
    let mut paths = Vec::new();
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let devinfo = SetupDiGetClassDevsW(
            &guid,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        if devinfo == INVALID_HANDLE_VALUE as isize {
            return Err(Error::Open { code: GetLastError() });
        }

        let mut index = 0u32;
        loop {
            let mut interface: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            interface.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            let ok =
                SetupDiEnumDeviceInterfaces(devinfo, std::ptr::null_mut(), &guid, index, &mut interface);
            if ok == 0 {
                break;
            }
            index += 1;

            let mut needed = 0u32;
            SetupDiGetDeviceInterfaceDetailW(
                devinfo,
                &interface,
                std::ptr::null_mut(),
                0,
                &mut needed,
                std::ptr::null_mut(),
            );
            if needed == 0 {
                continue;
            }

            let mut buffer = vec![0u8; needed as usize];
            let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            let ok = SetupDiGetDeviceInterfaceDetailW(
                devinfo,
                &interface,
                detail,
                needed,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if ok == 0 {
                continue;
            }

            let raw = (*detail).DevicePath.as_ptr();
            let mut len = 0usize;
            while *raw.add(len) != 0 {
                len += 1;
            }
            let wide = std::slice::from_raw_parts(raw, len);
            paths.push(OsString::from_wide(wide).to_string_lossy().into_owned());
        }
        SetupDiDestroyDeviceInfoList(devinfo);
    }
    Ok(paths)
}

/// Pull `(vid, pid, interface)` out of a HID interface path like
/// `\\?\hid#vid_1532&pid_0099&mi_03#7&...`.
pub fn parse_hid_path(path: &str) -> Option<(u16, u16, Option<u8>)> {
    let lower = path.to_ascii_lowercase();
    let vid = hex_after(&lower, "vid_", 4)?;
    let pid = hex_after(&lower, "pid_", 4)?;
    let mi = hex_after(&lower, "mi_", 2).map(|v| v as u8);
    Some((vid, pid, mi))
}

fn hex_after(haystack: &str, needle: &str, min_digits: usize) -> Option<u16> {
    let start = haystack.find(needle)? + needle.len();
    let digits: String = haystack[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if digits.len() < min_digits {
        return None;
    }
    u16::from_str_radix(&digits[..min_digits], 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_interface_path() {
        let p = r"\\?\hid#vid_1532&pid_0099&mi_03#7&2a1b3c4d&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(parse_hid_path(p), Some((0x1532, 0x0099, Some(3))));
        assert!(is_control_port(p));
    }

    #[test]
    fn ignores_a_different_interface_on_the_same_device() {
        let p = r"\\?\hid#vid_1532&pid_0099&mi_00#7&1&0000#{...}";
        assert_eq!(parse_hid_path(p), Some((0x1532, 0x0099, Some(0))));
        assert!(!is_control_port(p));
    }

    #[test]
    fn rejects_unrelated_devices() {
        let p = r"\\?\hid#vid_046d&pid_c52b&mi_00#7&1&0000#{...}";
        assert_eq!(parse_hid_path(p), Some((0x046d, 0xC52B, Some(0))));
    }
}
