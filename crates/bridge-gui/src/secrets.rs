use anyhow::{Result, bail};
use std::sync::{
    Arc, Once,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tokio::sync::oneshot;

thread_local! {
    static CREDENTIAL_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn protect_credential_panics() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // catch_unwind alone still prints the panic payload. Opaque OS
            // errors may contain credentials: suppress only this worker's
            // hook output, retaining the existing hook for every other thread.
            if !CREDENTIAL_THREAD.with(std::cell::Cell::get) {
                previous(info);
            }
        }));
    });
    CREDENTIAL_THREAD.with(|active| active.set(true));
}

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

// The platform stores use synchronous APIs. In particular, Linux keyring's
// zbus implementation enters its own runtime and must never run inside Tokio.
// OS unlock prompts may also wait indefinitely. This dedicated, detached owner
// serializes requests without making GUI shutdown wait for a blocking pool.
pub struct CredentialWorker {
    requests: mpsc::SyncSender<CredentialRequest>,
    occupied: Arc<AtomicBool>,
}
pub enum CredentialOperation {
    Get { id: String },
    Set { id: String, password: String },
    Delete { id: String },
}
pub enum CredentialValue {
    Password(Option<String>),
    Changed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialFailure {
    Unavailable,
    Panicked,
}
struct CredentialRequest {
    operation: CredentialOperation,
    response: oneshot::Sender<Result<CredentialValue, CredentialFailure>>,
}
impl CredentialWorker {
    pub fn new(mut store: Box<dyn SecretStore>) -> Self {
        let (requests, receiver) = mpsc::sync_channel::<CredentialRequest>(1);
        let occupied = Arc::new(AtomicBool::new(false));
        let worker_occupied = occupied.clone();
        let _ = std::thread::Builder::new()
            .name("c64-keyboard-bridge-credentials".into())
            .spawn(move || {
                protect_credential_panics();
                while let Ok(request) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match request.operation {
                            CredentialOperation::Get { id } => {
                                store.get(&id).map(CredentialValue::Password)
                            }
                            CredentialOperation::Set { id, password } => {
                                store.set(&id, &password).map(|()| CredentialValue::Changed)
                            }
                            CredentialOperation::Delete { id } => {
                                store.delete(&id).map(|()| CredentialValue::Changed)
                            }
                        }
                    }))
                    .map_err(|_| CredentialFailure::Panicked)
                    .and_then(|value| value.map_err(|_| CredentialFailure::Unavailable));
                    // A dropped receiver cancels only result application. A
                    // completed OS write/delete cannot safely be rolled back.
                    worker_occupied.store(false, Ordering::Release);
                    let _ = request.response.send(result);
                }
                // Credential objects are also dropped on this plain thread.
            });
        Self { requests, occupied }
    }
    pub fn request(
        &self,
        operation: CredentialOperation,
    ) -> Result<oneshot::Receiver<Result<CredentialValue, CredentialFailure>>> {
        if self
            .occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            bail!("An OS credential operation is still pending; unlock or dismiss its dialog");
        }
        let (response, receiver) = oneshot::channel();
        if self
            .requests
            .try_send(CredentialRequest {
                operation,
                response,
            })
            .is_err()
        {
            self.occupied.store(false, Ordering::Release);
            bail!("OS credential store worker unavailable; use a session password");
        }
        Ok(receiver)
    }
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
    struct RuntimeCheckingStore {
        observations: Arc<std::sync::Mutex<Vec<(&'static str, bool)>>>,
    }
    impl RuntimeCheckingStore {
        fn observe(&self, operation: &'static str) {
            self.observations
                .lock()
                .unwrap()
                .push((operation, tokio::runtime::Handle::try_current().is_ok()));
            assert!(
                tokio::runtime::Handle::try_current().is_err(),
                "credential operation entered Tokio"
            );
        }
    }
    impl SecretStore for RuntimeCheckingStore {
        fn get(&mut self, _: &str) -> Result<Option<String>> {
            self.observe("get");
            Ok(None)
        }
        fn set(&mut self, _: &str, _: &str) -> Result<()> {
            self.observe("set");
            Ok(())
        }
        fn delete(&mut self, _: &str) -> Result<()> {
            self.observe("delete");
            Ok(())
        }
    }
    impl Drop for RuntimeCheckingStore {
        fn drop(&mut self) {
            self.observe("drop");
        }
    }
    #[tokio::test]
    async fn credential_calls_and_drop_stay_outside_the_service_runtime() {
        let observations = Arc::new(std::sync::Mutex::new(Vec::new()));
        let worker = CredentialWorker::new(Box::new(RuntimeCheckingStore {
            observations: observations.clone(),
        }));
        for operation in [
            CredentialOperation::Get {
                id: "fixture".into(),
            },
            CredentialOperation::Set {
                id: "fixture".into(),
                password: "private".into(),
            },
            CredentialOperation::Delete {
                id: "fixture".into(),
            },
        ] {
            assert!(worker.request(operation).unwrap().await.unwrap().is_ok());
        }
        drop(worker);
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while observations.lock().unwrap().len() < 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            *observations.lock().unwrap(),
            [
                ("get", false),
                ("set", false),
                ("delete", false),
                ("drop", false)
            ]
        );
    }
    struct PanicOnceStore(bool);
    impl SecretStore for PanicOnceStore {
        fn get(&mut self, _: &str) -> Result<Option<String>> {
            if !self.0 {
                self.0 = true;
                panic!("CREDENTIAL_PANIC_SECRET_SENTINEL");
            }
            Ok(None)
        }
        fn set(&mut self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn delete(&mut self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn credential_worker_panic_payload_is_not_logged_and_retry_is_usable() {
        const CHILD: &str = "C64_KEYBOARD_BRIDGE_CREDENTIAL_PANIC_FIXTURE";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "secrets::tests::credential_worker_panic_payload_is_not_logged_and_retry_is_usable", "--nocapture"])
                .env(CHILD, "child")
                .output().unwrap();
            assert!(output.status.success());
            let log = String::from_utf8_lossy(&output.stderr);
            assert!(!log.contains("CREDENTIAL_PANIC_SECRET_SENTINEL"));
            assert!(
                !String::from_utf8_lossy(&output.stdout)
                    .contains("CREDENTIAL_PANIC_SECRET_SENTINEL")
            );
            assert!(
                log.contains("ORDINARY_THREAD_PANIC_TEST"),
                "unrelated thread panic hook was suppressed"
            );
            return;
        }
        let worker = CredentialWorker::new(Box::new(PanicOnceStore(false)));
        let first = worker
            .request(CredentialOperation::Get {
                id: "fixture".into(),
            })
            .unwrap()
            .await
            .unwrap();
        assert!(matches!(first, Err(CredentialFailure::Panicked)));
        assert!(matches!(
            worker
                .request(CredentialOperation::Get {
                    id: "fixture".into()
                })
                .unwrap()
                .await
                .unwrap(),
            Ok(CredentialValue::Password(None))
        ));
        assert!(
            std::thread::spawn(|| panic!("ORDINARY_THREAD_PANIC_TEST"))
                .join()
                .is_err()
        );
    }
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
