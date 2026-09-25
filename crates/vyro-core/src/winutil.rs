//! Small safe wrappers over Win32 APIs shared by the Windows collectors.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW, HKEY, KEY_READ,
    REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_SZ, REG_VALUE_TYPE, RRF_RT_ANY,
};

pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// Decodes UTF-16LE bytes, dropping trailing NULs.
pub fn utf16_bytes(bytes: &[u8]) -> String {
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&wide)
        .trim_end_matches('\0')
        .to_owned()
}

/// A registry value as read.
#[derive(Debug, Clone)]
pub struct RegValue {
    pub kind: REG_VALUE_TYPE,
    pub data: Vec<u8>,
}

impl RegValue {
    pub fn as_string(&self) -> Option<String> {
        match self.kind {
            REG_SZ | REG_EXPAND_SZ => Some(utf16_bytes(&self.data).trim().to_owned()),
            _ => None,
        }
    }

    pub fn as_dword(&self) -> Option<u32> {
        if self.kind != REG_DWORD {
            return None;
        }
        Some(u32::from_le_bytes(self.data.get(..4)?.try_into().ok()?))
    }

    pub fn as_multi_string(&self) -> Vec<String> {
        if self.kind != REG_MULTI_SZ {
            return Vec::new();
        }
        utf16_bytes(&self.data)
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

/// An open registry key, closed on drop. Read-only.
pub struct Key(HKEY);

impl Key {
    pub fn open(root: HKEY, path: &str) -> Option<Self> {
        let mut h = HKEY::default();
        // SAFETY: `h` is a valid out-pointer; the path is a NUL-terminated HSTRING.
        let rc = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut h) };
        (rc == ERROR_SUCCESS).then_some(Self(h))
    }

    pub fn subkey(&self, path: &str) -> Option<Self> {
        Self::open(self.0, path)
    }

    pub fn value(&self, name: &str) -> Option<RegValue> {
        let name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        // SAFETY: size query with no buffer; out-pointers are valid.
        let rc = unsafe {
            RegGetValueW(
                self.0,
                PCWSTR::null(),
                &name,
                RRF_RT_ANY,
                Some(&mut kind),
                None,
                Some(&mut size),
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        // Values can grow between the two calls; retry a few times.
        for _ in 0..4 {
            let mut data = vec![0u8; size as usize + 2];
            let mut len = u32::try_from(data.len()).ok()?;
            // SAFETY: `data` has `len` writable bytes.
            let rc = unsafe {
                RegGetValueW(
                    self.0,
                    PCWSTR::null(),
                    &name,
                    RRF_RT_ANY,
                    Some(&mut kind),
                    Some(data.as_mut_ptr().cast()),
                    Some(&mut len),
                )
            };
            if rc == ERROR_SUCCESS {
                data.truncate(len as usize);
                return Some(RegValue { kind, data });
            }
            if rc != ERROR_MORE_DATA {
                return None;
            }
            size = len;
        }
        None
    }

    pub fn string(&self, name: &str) -> Option<String> {
        self.value(name)?.as_string().filter(|s| !s.is_empty())
    }

    pub fn dword(&self, name: &str) -> Option<u32> {
        self.value(name)?.as_dword()
    }

    /// All values as `(name, value)`.
    pub fn values(&self) -> Vec<(String, RegValue)> {
        let mut out = Vec::new();
        let mut name = vec![0u16; 16_384];
        let mut data = vec![0u8; 4096];
        let mut index = 0;
        loop {
            let mut name_len = u32::try_from(name.len()).unwrap_or(u32::MAX);
            let mut data_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            let mut kind = 0u32;
            // SAFETY: buffers and lengths match; out-pointers are valid.
            let rc = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                )
            };
            if rc == ERROR_MORE_DATA {
                data.resize(data_len as usize + 2, 0);
                continue; // retry the same index with a bigger buffer
            }
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push((
                String::from_utf16_lossy(&name[..name_len as usize]),
                RegValue {
                    kind: REG_VALUE_TYPE(kind),
                    data: data[..data_len as usize].to_vec(),
                },
            ));
            index += 1;
        }
        out
    }

    pub fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut name = vec![0u16; 256];
        let mut index = 0;
        loop {
            let mut len = u32::try_from(name.len()).unwrap_or(u32::MAX);
            // SAFETY: buffer and length match.
            let rc = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
            index += 1;
        }
        out
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegOpenKeyExW and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Converts a NUL-terminated UTF-16 array (e.g. a fixed-size struct field) to a String.
pub fn wide_array(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end]).trim().to_owned()
}
