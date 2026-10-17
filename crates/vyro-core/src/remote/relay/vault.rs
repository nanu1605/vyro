//! Secrets at rest (1.4). The PC has to keep the relay keys it uses (unlike the local
//! companion's tokens, of which it keeps only hashes), so on Windows they are sealed to the
//! signed-in account with DPAPI: a copy of `remote.json` opens on no other PC or account.
//! Elsewhere (tests, development) they are kept as they are, marked so.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;

const DPAPI: &str = "dpapi:";
const PLAIN: &str = "plain:";

/// A secret as saved.
pub fn protect(secret: &[u8]) -> Option<String> {
    #[cfg(windows)]
    {
        win::protect(secret).map(|b| format!("{DPAPI}{}", STANDARD.encode(b)))
    }
    #[cfg(not(windows))]
    {
        Some(format!("{PLAIN}{}", URL_SAFE_NO_PAD.encode(secret)))
    }
}

/// A saved secret, or `None` when it does not open here (another account or PC, or damaged).
pub fn unprotect(saved: &str) -> Option<Vec<u8>> {
    if let Some(b) = saved.strip_prefix(PLAIN) {
        return URL_SAFE_NO_PAD.decode(b).ok();
    }
    let sealed = STANDARD.decode(saved.strip_prefix(DPAPI)?).ok()?;
    #[cfg(windows)]
    {
        win::unprotect(&sealed)
    }
    #[cfg(not(windows))]
    {
        let _ = sealed;
        None
    }
}

#[cfg(windows)]
mod win {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    /// Ties the sealed bytes to Vyro: another program of the same account that calls DPAPI
    /// without it cannot open them by accident.
    const ENTROPY: &[u8] = b"vyro relay keys";

    fn blob(b: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(b.len()).unwrap_or(0),
            pbData: b.as_ptr().cast_mut(),
        }
    }

    /// Copies DPAPI's output and frees it.
    fn take(out: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: DPAPI returned `cbData` bytes at `pbData`, ours to free with LocalFree.
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: as above; freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        }
        bytes
    }

    pub fn protect(secret: &[u8]) -> Option<Vec<u8>> {
        let (input, entropy) = (blob(secret), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: the blobs point at live slices for the call; DPAPI only reads them.
        let r = unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        let bytes = take(&out);
        r.ok().map(|()| bytes)
    }

    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        let (input, entropy) = (blob(sealed), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`.
        let r = unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        let bytes = take(&out);
        r.ok().map(|()| bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_comes_back_and_a_damaged_one_does_not() {
        let secret = [42u8; 32];
        let saved = protect(&secret).unwrap();
        assert_eq!(unprotect(&saved).unwrap(), secret);
        assert_eq!(unprotect("dpapi:not base64!"), None);
        assert_eq!(unprotect("something else"), None);
        #[cfg(windows)]
        assert!(saved.starts_with(DPAPI), "sealed with DPAPI on Windows");
    }
}
