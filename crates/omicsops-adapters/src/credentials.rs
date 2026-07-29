use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use uuid::Uuid;

use crate::{AdapterError, AdapterResult};

const SERVICE_NAME: &str = "io.omicsops.desktop";

pub trait CredentialVault: Send + Sync {
    fn set(&self, account: &str, secret: &str) -> AdapterResult<()>;
    fn get(&self, account: &str) -> AdapterResult<Option<String>>;
    fn delete(&self, account: &str) -> AdapterResult<()>;
}

pub fn credential_account(kind: &str, id: Uuid) -> String {
    format!("{kind}/{id}")
}

#[derive(Debug, Clone, Default)]
pub struct MemoryCredentialVault {
    values: Arc<Mutex<BTreeMap<String, String>>>,
}

impl MemoryCredentialVault {
    pub fn accounts(&self) -> Vec<String> {
        self.values
            .lock()
            .expect("credential vault lock")
            .keys()
            .cloned()
            .collect()
    }
}

impl CredentialVault for MemoryCredentialVault {
    fn set(&self, account: &str, secret: &str) -> AdapterResult<()> {
        self.values
            .lock()
            .expect("credential vault lock")
            .insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, account: &str) -> AdapterResult<Option<String>> {
        Ok(self
            .values
            .lock()
            .expect("credential vault lock")
            .get(account)
            .cloned())
    }

    fn delete(&self, account: &str) -> AdapterResult<()> {
        self.values
            .lock()
            .expect("credential vault lock")
            .remove(account);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCredentialVault;

impl CredentialVault for SystemCredentialVault {
    fn set(&self, account: &str, secret: &str) -> AdapterResult<()> {
        keyring::Entry::new(SERVICE_NAME, account)
            .map_err(keyring_error)?
            .set_password(secret)
            .map_err(keyring_error)
    }

    fn get(&self, account: &str) -> AdapterResult<Option<String>> {
        let entry = keyring::Entry::new(SERVICE_NAME, account).map_err(keyring_error)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn delete(&self, account: &str) -> AdapterResult<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, account).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

fn keyring_error(error: keyring::Error) -> AdapterError {
    AdapterError::Credential(error.to_string())
}
