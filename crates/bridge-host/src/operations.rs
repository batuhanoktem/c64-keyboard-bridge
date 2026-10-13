//! Operations shared by the CLI and desktop frontend.
//! Frontends own confirmations and countdowns; this module enforces reset opt-in
//! and never retries a disruptive request after an ambiguous transport failure.
use crate::{backend, config::Config, pairing, prg, rest::RestClient, runtime};
use anyhow::{Context, Result, bail};
use bridge_core::{KeyboardBackend, NativeKey};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub type ProbeEvidence = serde_json::Value;

/// Patch an in-memory copy and upload complete PRG bytes. Both runner modes
/// reset the C64, so each invocation requires a fresh frontend opt-in.
pub async fn deploy(config: &Config, template: &[u8], run: bool, allow_reset: bool) -> Result<()> {
    if !allow_reset {
        bail!(
            "PRG deployment resets the C64 and interrupts its software; add --allow-reset explicitly"
        );
    }
    let client = RestClient::new(config)?;
    let info = client.info().await?;
    let network = config.network().await?;
    let token = pairing::load_or_create_token(&config.state_dir)?;
    let patched = prg::patch(
        template,
        network.advertise_ip,
        config.bridge_port,
        token,
        &info.unique_id,
    )?;
    client.upload(patched, run, allow_reset).await
}

/// Run the networking probe, retaining its private hardware evidence. Dropping
/// the future closes both probe and REST sockets instead of leaking a listener.
pub async fn probe(config: &Config, template: &[u8], allow_reset: bool) -> Result<ProbeEvidence> {
    if !allow_reset {
        bail!(
            "The hardware probe resets and replaces the running C64 program; add --allow-reset explicitly"
        );
    }
    if config.state_dir.join("runtime.json").exists() {
        bail!("Stop the active bridge before running the networking probe");
    }
    let client = RestClient::new(config)?;
    let info = client.info().await?;
    let network = config.network().await?;
    let listener = tokio::net::TcpListener::bind(network.bind)
        .await
        .context("Probe listener port unavailable")?;
    let token = pairing::load_or_create_token(&config.state_dir)?;
    let bytes = prg::patch(
        template,
        network.advertise_ip,
        config.bridge_port,
        token,
        &info.unique_id,
    )?;
    // Poll networking concurrently with upload so a fast C64 receives its ACK
    // while the runner response is still in flight. Cancellation drops both.
    let mut evidence = match tokio::try_join!(
        client.upload(bytes, true, allow_reset),
        runtime::hardware_probe(listener, &network, token, &info),
    ) {
        Ok(((), evidence)) => evidence,
        Err(error) => {
            let failure = serde_json::json!({"device":info,"network_acceptance":"failed; not verified","error":format!("{error:#}"),"host_version":env!("CARGO_PKG_VERSION"),"observed_unix_seconds":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs()});
            let mut file =
                pairing::private_file(&config.state_dir.join("hardware-failure.json"), false)?;
            serde_json::to_writer_pretty(&mut file, &failure)?;
            return Err(error);
        }
    };
    let build_id = template.iter().fold(0xcbf29ce484222325u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    evidence["probe_template_fnv1a64"] = serde_json::json!(format!("{build_id:016x}"));
    let mut file = pairing::private_file(&config.state_dir.join("hardware-evidence.json"), false)?;
    serde_json::to_writer_pretty(&mut file, &evidence)?;
    Ok(evidence)
}

#[derive(Clone, Debug, Serialize)]
pub struct InputTestReport {
    pub phrase: &'static str,
    pub accepted_pairs: usize,
    /// This is a posting result. Visual delivery is always observed by the user.
    pub delivery_verified: bool,
}

/// Post the deliberate phrase immediately on a blocking worker. The caller must
/// finish its focus countdown before calling; no host input is ever captured.
pub async fn test_input(config: &Config) -> Result<InputTestReport> {
    test_input_with_guard(config, None, None).await
}

/// GUI countdowns additionally gate construction and every concrete press so a
/// focus change during backend authorization cannot post into the desktop UI.
pub async fn test_input_guarded(
    config: &Config,
    guard: Arc<AtomicBool>,
) -> Result<InputTestReport> {
    test_input_with_guard(config, Some(guard), None).await
}

/// Focus state is written only by the window thread, independently from a
/// background countdown rearming its own permission flag.
pub async fn test_input_guarded_with_focus(
    config: &Config,
    allow: Arc<AtomicBool>,
    focus_allowed: Arc<AtomicBool>,
) -> Result<InputTestReport> {
    test_input_with_guard(config, Some(allow), Some(focus_allowed)).await
}

async fn test_input_with_guard(
    config: &Config,
    guard: Option<Arc<AtomicBool>>,
    focus_allowed: Option<Arc<AtomicBool>>,
) -> Result<InputTestReport> {
    let selection = config.backend.clone();
    let layout = runtime::layout(config);
    tokio::task::spawn_blocking(move || {
        if guard
            .as_ref()
            .is_some_and(|guard| !guard.load(Ordering::SeqCst))
            || focus_allowed
                .as_ref()
                .is_some_and(|focus| !focus.load(Ordering::SeqCst))
        {
            bail!("GUI input guard is closed; input test stayed disabled");
        }
        let backend = backend::create_backend(&selection, layout)?;
        post_prepared_test_blocking(backend, guard, focus_allowed)
    })
    .await
    .context("Input test worker failed")?
}

/// Authorization precedes the frontend countdown. Preparation never posts input.
pub async fn prepare_input_test(
    config: &Config,
    cancellation: tokio::sync::watch::Receiver<bool>,
) -> Result<Box<dyn KeyboardBackend>> {
    backend::prepare_backend(&config.backend, runtime::layout(config), cancellation)
        .await
        .map_err(Into::into)
}
/// Reuse a backend retained through the countdown; no permission dialog can open
/// between the countdown finishing and the first deliberate press.
pub async fn test_input_prepared(
    backend: Box<dyn KeyboardBackend>,
    guard: Option<Arc<AtomicBool>>,
    focus_allowed: Option<Arc<AtomicBool>>,
) -> Result<InputTestReport> {
    tokio::task::spawn_blocking(move || post_prepared_test_blocking(backend, guard, focus_allowed))
        .await
        .context("Input test worker failed")?
}
fn post_prepared_test_blocking(
    backend: Box<dyn KeyboardBackend>,
    guard: Option<Arc<AtomicBool>>,
    focus_allowed: Option<Arc<AtomicBool>>,
) -> Result<InputTestReport> {
    if guard
        .as_ref()
        .is_some_and(|guard| !guard.load(Ordering::SeqCst))
        || focus_allowed
            .as_ref()
            .is_some_and(|focus| !focus.load(Ordering::SeqCst))
    {
        bail!("GUI input guard is closed; input test stayed disabled");
    }
    let mut backend = if let Some(guard) = guard {
        Box::new(if let Some(focus) = focus_allowed {
            runtime::GuardedBackend::new_with_focus(backend, guard, focus)
        } else {
            runtime::GuardedBackend::new(backend, guard)
        }) as Box<dyn KeyboardBackend>
    } else {
        backend
    };
    post_input_test(backend.as_mut())
}

fn post_input_test(backend: &mut dyn KeyboardBackend) -> Result<InputTestReport> {
    use NativeKey as K;
    let keys = [
        K::C,
        K::Num6,
        K::Num4,
        K::Space,
        K::B,
        K::R,
        K::I,
        K::D,
        K::G,
        K::E,
        K::Enter,
    ];
    for key in keys {
        backend.validate_layout()?;
        if let Err(error) = backend.press(key) {
            // A partially accepted press is still ours to release. Preserve a
            // secondary cleanup failure so frontends cannot report safe exit.
            return match backend.release(key) {
                Ok(()) => Err(error.into()),
                Err(cleanup) => Err(anyhow::Error::new(error)
                    .context(format!("Input test cleanup failed: {cleanup}"))),
            };
        }
        if let Err(error) = backend.release(key) {
            return match backend.release(key) {
                Ok(()) => Err(error.into()),
                Err(cleanup) => Err(anyhow::Error::new(error)
                    .context(format!("Input test cleanup failed: {cleanup}"))),
            };
        }
    }
    Ok(InputTestReport {
        phrase: "c64 bridge\n",
        accepted_pairs: keys.len(),
        delivery_verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{BackendCapabilities, BackendError, RecordingBackend, RecordingEvent};
    use std::collections::BTreeMap;

    #[test]
    fn deliberate_phrase_posts_balanced_pairs_without_claiming_delivery() {
        let mut backend = RecordingBackend::default();
        let report = post_input_test(&mut backend).unwrap();
        assert_eq!(report.phrase, "c64 bridge\n");
        assert_eq!(report.accepted_pairs, 11);
        assert!(!report.delivery_verified);
        assert_eq!(backend.events.len(), 22);
        for pair in backend.events.as_chunks::<2>().0 {
            assert!(
                matches!(pair, [RecordingEvent::Press(down), RecordingEvent::Release(up)] if down == up)
            );
        }
    }

    struct PartialFailure {
        releases: usize,
        fail_cleanup: bool,
    }
    impl KeyboardBackend for PartialFailure {
        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                key_events: true,
                text: false,
                repeat: false,
            }
        }
        fn press(&mut self, _: NativeKey) -> std::result::Result<(), BackendError> {
            Err(BackendError::Injection("partially accepted press".into()))
        }
        fn release(&mut self, _: NativeKey) -> std::result::Result<(), BackendError> {
            self.releases += 1;
            if self.fail_cleanup {
                Err(BackendError::Injection("release failed".into()))
            } else {
                Ok(())
            }
        }
        fn repeat(&mut self, _: NativeKey) -> std::result::Result<(), BackendError> {
            unreachable!()
        }
    }
    #[test]
    fn deliberate_phrase_releases_partial_press_and_retains_cleanup_failure() {
        for fail_cleanup in [false, true] {
            let mut backend = PartialFailure {
                releases: 0,
                fail_cleanup,
            };
            let error = post_input_test(&mut backend).unwrap_err();
            assert_eq!(backend.releases, 1);
            assert!(format!("{error:#}").contains("partially accepted press"));
            assert_eq!(
                format!("{error:#}").contains("cleanup failed"),
                fail_cleanup
            );
        }
    }
    #[tokio::test]
    async fn guarded_input_test_never_constructs_unknown_backend_when_closed() {
        let mut config = Config::from_values(
            BTreeMap::new(),
            BTreeMap::new(),
            crate::config::Overrides::default(),
        )
        .unwrap();
        config.backend = "invalid-backend-would-fail-construction".into();
        let error = test_input_guarded(&config, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("guard is closed"));
    }

    #[tokio::test]
    async fn disruptive_operations_reject_missing_opt_in_before_network_or_storage() {
        let parent = tempfile::tempdir().unwrap();
        let state = parent.path().join("unused-private");
        let config = Config::from_values(
            BTreeMap::from([("BRIDGE_STATE_DIR".into(), state.display().to_string())]),
            BTreeMap::new(),
            crate::config::Overrides::default(),
        )
        .unwrap();
        assert!(
            deploy(&config, &[], true, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("resets")
        );
        assert!(
            probe(&config, &[], false)
                .await
                .unwrap_err()
                .to_string()
                .contains("resets")
        );
        assert!(!state.exists());
    }
}
