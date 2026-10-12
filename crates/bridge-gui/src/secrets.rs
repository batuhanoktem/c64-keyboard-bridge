use anyhow::{Result, bail};

const SERVICE: &str = "com.flameliongames.c64bridge";

pub trait SecretStore: Send {
    fn get(&mut self, id: &str) -> Result<Option<String>>;
    fn set(&mut self, id: &str, password: &str) -> Result<()>;
    fn delete(&mut self, id: &str) -> Result<()>;
}
pub struct OsSecretStore;
impl SecretStore for OsSecretStore {
    fn get(&mut self, id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, id)
            .map_err(|_| anyhow::anyhow!("OS credential store unavailable"))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => bail!("OS credential store is locked or unavailable"),
        }
    }
    fn set(&mut self, id: &str, password: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, id)
            .map_err(|_| anyhow::anyhow!("OS credential store unavailable"))?
            .set_password(password)
            .map_err(|_| {
                anyhow::anyhow!(
                    "Password was not saved; keep it in memory or unlock the OS credential store"
                )
            })
    }
    fn delete(&mut self, id: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, id)
            .map_err(|_| anyhow::anyhow!("OS credential store unavailable"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => bail!("Credential deletion failed; the saved password may still exist"),
        }
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    pub value: Option<String>,
    pub fail_get: bool,
    pub fail_set: bool,
    pub fail_delete: bool,
}
impl SecretStore for MemorySecretStore {
    fn get(&mut self, _: &str) -> Result<Option<String>> {
        if self.fail_get {
            bail!("locked");
        }
        Ok(self.value.clone())
    }
    fn set(&mut self, _: &str, p: &str) -> Result<()> {
        if self.fail_set {
            bail!("unavailable");
        }
        self.value = Some(p.into());
        Ok(())
    }
    fn delete(&mut self, _: &str) -> Result<()> {
        if self.fail_delete {
            bail!("locked");
        }
        self.value = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_credential_changes_keep_prior_secret_and_are_retryable() {
        let mut store = MemorySecretStore {
            value: Some("old".into()),
            fail_set: true,
            fail_delete: true,
            ..Default::default()
        };
        assert!(store.set("id", "new").is_err());
        assert_eq!(store.get("id").unwrap().as_deref(), Some("old"));
        assert!(store.delete("id").is_err());
        assert!(store.value.is_some());
        store.fail_delete = false;
        store.delete("id").unwrap();
        assert!(store.value.is_none());
    }
}
