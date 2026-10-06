//! API keys live in the OS keychain (macOS Keychain, Windows Credential
//! Manager, Secret Service on Linux), never in plain files.

use std::collections::HashMap;

use parking_lot::Mutex;

const SERVICE: &str = "xyz.lsuite.folio";

/// Where API keys are kept: the OS keychain in the app, memory in tests and debug builds.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Option<String>;
    fn set(&self, id: &str, key: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// Keys in memory only (tests, debug builds).
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, id: &str) -> Option<String> {
        self.0.lock().get(id).cloned()
    }
    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        self.0.lock().insert(id.to_string(), key.to_string());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        self.0.lock().remove(id);
        Ok(())
    }
}

/// Keychain-backed store with an in-memory cache, so the OS is asked at most
/// once per provider per launch.
#[derive(Default)]
pub struct KeychainSecrets {
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl SecretStore for KeychainSecrets {
    fn get(&self, provider: &str) -> Option<String> {
        if let Some(v) = self.cache.lock().get(provider) {
            return v.clone();
        }
        let v = match keyring::Entry::new(SERVICE, provider).and_then(|e| e.get_password()) {
            Ok(k) => Some(k),
            Err(keyring::Error::NoEntry) => None,
            // A locked keychain or a secret service not up yet: ask again next time.
            Err(e) => {
                tracing::warn!("couldn't read the {provider} key from the keychain: {e}");
                return None;
            }
        };
        self.cache.lock().insert(provider.to_string(), v.clone());
        v
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, provider)
            .and_then(|e| e.set_password(key))
            .map_err(|e| format!("Couldn't save the key to the keychain: {e}"))?;
        self.cache.lock().insert(provider.to_string(), Some(key.to_string()));
        Ok(())
    }

    fn delete(&self, provider: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, provider).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("Couldn't remove the key: {e}")),
        }
        self.cache.lock().insert(provider.to_string(), None);
        Ok(())
    }
}

/// The store folio's programs use: the OS keychain, except in debug builds
/// (`cargo run`), where an unsigned binary would make macOS ask for the login
/// password every time it reads the signed app's keys. `FOLIO_KEYCHAIN=1`
/// forces the keychain, `FOLIO_KEYCHAIN=0` turns it off; without it, keys
/// still come from the usual environment variables.
pub fn default_store() -> std::sync::Arc<dyn SecretStore> {
    let on = match std::env::var("FOLIO_KEYCHAIN").ok().as_deref() {
        Some("1" | "on" | "true" | "yes") => true,
        Some(_) => false,
        None => !cfg!(debug_assertions),
    };
    if on { std::sync::Arc::new(KeychainSecrets::default()) } else { std::sync::Arc::new(MemorySecrets::default()) }
}
