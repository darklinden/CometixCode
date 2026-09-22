//! Maps to: CC `utils/secureStorage/index.ts`.
//!
//! The source `types.ts` file is generated/insufficient, so Rust does not claim
//! to port it. `SecureStorageBackend` and `SecureStorageUpdate` are narrow
//! internal carriers inferred only from the complete `index.ts`,
//! `fallbackStorage.ts`, `macOsKeychainStorage.ts`, and `plainTextStorage.ts`
//! method bodies.

pub mod fallback_storage;
pub mod keychain_prefetch;
pub mod mac_os_keychain_helpers;
pub mod mac_os_keychain_storage;
pub mod plain_text_storage;

pub(crate) type SecureStorageData = serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SecureStorageUpdate {
    pub(crate) success: bool,
    pub(crate) warning: Option<String>,
}

pub(crate) trait SecureStorageBackend: Send + Sync {
    fn name(&self) -> &str;
    fn read(&self) -> Option<SecureStorageData>;
    fn update(&self, data: &SecureStorageData) -> anyhow::Result<SecureStorageUpdate>;
    fn delete(&self) -> anyhow::Result<bool>;
}

/// Maps to: CC `utils/secureStorage/index.ts:9-19` `getSecureStorage`.
pub(crate) fn get_secure_storage() -> Box<dyn SecureStorageBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(fallback_storage::create_fallback_storage(
            Box::new(mac_os_keychain_storage::MacOsKeychainStorage),
            Box::new(plain_text_storage::PlainTextStorage),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Box::new(plain_text_storage::PlainTextStorage)
    }
}
