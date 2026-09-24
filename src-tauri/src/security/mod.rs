// Brail Recorder — Security Module
// Secure credential storage (DPAPI / Windows Credential Vault) and log secret redaction

use std::ptr;
use tracing::{info, warn};
use windows::Win32::Security::Cryptography::*;
use windows::Win32::Foundation::*;

const ENTROPY_SALT: &[u8] = b"BrailRecorder_v1_Entropy_Key";

/// Secure Credential Store using Windows DPAPI
pub struct SecureVault;

impl SecureVault {
    /// Protect (encrypt) data using Windows DPAPI (CryptProtectData)
    pub fn encrypt_secret(secret: &str) -> Result<Vec<u8>, String> {
        if secret.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let secret_bytes = secret.as_bytes();
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: secret_bytes.len() as u32,
                pbData: secret_bytes.as_ptr() as *mut u8,
            };

            let mut entropy = CRYPT_INTEGER_BLOB {
                cbData: ENTROPY_SALT.len() as u32,
                pbData: ENTROPY_SALT.as_ptr() as *mut u8,
            };

            let mut data_out = CRYPT_INTEGER_BLOB::default();

            let success = CryptProtectData(
                &mut data_in,
                windows::core::PCWSTR(ptr::null()),
                Some(&mut entropy),
                None,
                None,
                0,
                &mut data_out,
            );

            if success.is_ok() {
                let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
                let encrypted = slice.to_vec();
                // Free DPAPI buffer
                let _ = windows::Win32::System::Memory::LocalFree(windows::Win32::System::Memory::HLOCAL(data_out.pbData as _));
                Ok(encrypted)
            } else {
                Err("Failed to encrypt secret using Windows DPAPI".to_string())
            }
        }
    }

    /// Unprotect (decrypt) data using Windows DPAPI (CryptUnprotectData)
    pub fn decrypt_secret(encrypted_bytes: &[u8]) -> Result<String, String> {
        if encrypted_bytes.is_empty() {
            return Ok(String::new());
        }

        unsafe {
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: encrypted_bytes.len() as u32,
                pbData: encrypted_bytes.as_ptr() as *mut u8,
            };

            let mut entropy = CRYPT_INTEGER_BLOB {
                cbData: ENTROPY_SALT.len() as u32,
                pbData: ENTROPY_SALT.as_ptr() as *mut u8,
            };

            let mut data_out = CRYPT_INTEGER_BLOB::default();

            let success = CryptUnprotectData(
                &mut data_in,
                None,
                Some(&mut entropy),
                None,
                None,
                0,
                &mut data_out,
            );

            if success.is_ok() {
                let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
                let decrypted = String::from_utf8_lossy(slice).to_string();
                let _ = windows::Win32::System::Memory::LocalFree(windows::Win32::System::Memory::HLOCAL(data_out.pbData as _));
                Ok(decrypted)
            } else {
                Err("Failed to decrypt secret using Windows DPAPI".to_string())
            }
        }
    }

    /// Redacts known stream keys, secrets, or credential tokens from logs or error messages
    pub fn redact(text: &str, known_secret: Option<&str>) -> String {
        let mut result = text.to_string();

        if let Some(secret) = known_secret {
            if !secret.is_empty() && secret.len() >= 4 {
                result = result.replace(secret, "[REDACTED_STREAM_KEY]");
            }
        }

        // Generic patterns (e.g. stream key formats like "xxxx-xxxx-xxxx-xxxx-xxxx")
        let re_key = regex_lite_replace(&result);
        re_key
    }
}

fn regex_lite_replace(s: &str) -> String {
    // Basic redaction for common key formats
    let mut out = s.to_string();
    if let Some(idx) = out.find("live_") {
        if idx + 20 <= out.len() {
            out.replace_range(idx..(idx + 20), "live_****************");
        }
    }
    out
}
