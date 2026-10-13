use anyhow::{Result, bail};

// This namespace intentionally has no lookup or migration from older builds.
const CREDENTIAL_SERVICE: &str = bridge_host::config::APPLICATION_ID;

fn credential_entry<T>(
    id: &str,
    create: impl FnOnce(&str, &str) -> keyring::Result<T>,
) -> Result<T> {
    create(CREDENTIAL_SERVICE, id).map_err(|_| anyhow::anyhow!("OS credential store unavailable"))
}

pub trait SecretStore: Send {
    fn get(&mut self, id: &str) -> Result<Option<String>>;
    fn set(&mut self, id: &str, password: &str) -> Result<()>;
    fn delete(&mut self, id: &str) -> Result<()>;
}
pub struct OsSecretStore;
impl SecretStore for OsSecretStore {
    fn get(&mut self, id: &str) -> Result<Option<String>> {
        let entry = credential_entry(id, keyring::Entry::new)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => bail!("OS credential store is locked or unavailable"),
        }
    }
    fn set(&mut self, id: &str, password: &str) -> Result<()> {
        credential_entry(id, keyring::Entry::new)?
            .set_password(password)
            .map_err(|_| {
                anyhow::anyhow!(
                    "Password was not saved; keep it in memory or unlock the OS credential store"
                )
            })
    }
    fn delete(&mut self, id: &str) -> Result<()> {
        let entry = credential_entry(id, keyring::Entry::new)?;
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
    fn credential_selection_uses_only_the_new_namespace_without_fallback() {
        use std::collections::BTreeMap;
        let mut entries = BTreeMap::from([(
            ("previous.application".to_owned(), "device".to_owned()),
            "older secret sentinel",
        )]);
        let mut requests = Vec::new();
        let current = credential_entry("device", |service, user| {
            requests.push((service.to_owned(), user.to_owned()));
            Ok(entries.get(&(service.to_owned(), user.to_owned())).copied())
        })
        .unwrap();
        assert_eq!(current, None);
        assert_eq!(
            requests,
            [(
                "com.flameliongames.c64keyboardbridge".into(),
                "device".into()
            )]
        );
        entries.insert(
            (CREDENTIAL_SERVICE.into(), "device".into()),
            "current secret",
        );
        let current = credential_entry("device", |service, user| {
            Ok(entries.get(&(service.to_owned(), user.to_owned())).copied())
        })
        .unwrap();
        assert_eq!(current, Some("current secret"));
        assert_eq!(
            entries[&("previous.application".into(), "device".into())],
            "older secret sentinel"
        );
    }
    #[test]
    fn credential_entry_failure_is_sanitized_and_does_not_try_another_namespace() {
        let mut attempts = 0;
        let failure = credential_entry::<()>("device", |service, _| {
            attempts += 1;
            assert_eq!(service, bridge_host::config::APPLICATION_ID);
            Err(keyring::Error::Invalid(
                "sentinel identity".into(),
                "sentinel secret".into(),
            ))
        })
        .unwrap_err()
        .to_string();
        assert_eq!(attempts, 1);
        assert_eq!(failure, "OS credential store unavailable");
        assert!(!failure.contains("sentinel"));
    }
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
