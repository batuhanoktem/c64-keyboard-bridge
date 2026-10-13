use super::*;
use crate::secrets::SecretStore;

#[test]
fn trusted_external_targets_use_argument_vectors_and_platform_specific_openers() {
    for (platform, program, prefix) in [
        (GuiPlatform::MacOs, "/usr/bin/open", None),
        (
            GuiPlatform::Windows,
            "rundll32.exe",
            Some("url.dll,FileProtocolHandler"),
        ),
        (GuiPlatform::Linux, "xdg-open", None),
    ] {
        let command = external_command(platform, ExternalTarget::ProjectWebsite).unwrap();
        assert_eq!(command.get_program(), program);
        let mut expected: Vec<&str> = prefix.into_iter().collect();
        expected.push("https://github.com/batuhanoktem/c64-keyboard-bridge");
        assert_eq!(command.get_args().collect::<Vec<_>>(), expected);
        assert_eq!(
            external_command(platform, ExternalTarget::MacInputSettings).is_ok(),
            platform == GuiPlatform::MacOs,
        );
    }
}

#[tokio::test]
async fn isolated_gui_link_does_not_launch_a_browser_or_touch_settings() {
    let (mut actor, _events) = actor();
    let original = actor.settings.clone();
    actor.action(GuiAction::OpenProjectWebsite).await.unwrap();
    assert!(
        actor
            .snapshot
            .message
            .contains("external navigation suppressed")
    );
    assert_eq!(actor.settings, original);
    assert!(!actor.state_dir.join("gui-settings.toml").exists());
    assert!(actor.owned.is_none());
    cleanup(actor);
}

#[tokio::test]
async fn browser_failure_is_actionable_and_excludes_subprocess_output() {
    #[cfg(unix)]
    let mut command = {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "printf 'OPENER_SECRET_SENTINEL' >&2; exit 1"]);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("cmd.exe");
        command.args(["/C", "echo OPENER_SECRET_SENTINEL >&2 & exit /B 1"]);
        command
    };
    let error = run_external(
        &mut command,
        ExternalTarget::ProjectWebsite,
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert_eq!(
        display_error(&error),
        ExternalTarget::ProjectWebsite.failure()
    );
    assert!(!display_error(&error).contains("OPENER_SECRET_SENTINEL"));
}

#[tokio::test]
async fn hanging_desktop_opener_is_bounded() {
    #[cfg(unix)]
    let mut command = {
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("2");
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 2",
        ]);
        command
    };
    let before = Instant::now();
    assert!(
        run_external(
            &mut command,
            ExternalTarget::ProjectWebsite,
            Duration::from_millis(40)
        )
        .await
        .is_err()
    );
    assert!(before.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn unavailable_imported_backend_requires_correction_before_saving_or_using_form() {
    let (mut actor, _events) = actor();
    let mut foreign = actor.settings.clone();
    foreign.backend = if GuiPlatform::current() == GuiPlatform::MacOs {
        "windows"
    } else {
        "macos"
    }
    .into();
    actor.settings = foreign.clone();
    assert!(
        actor
            .save_form(foreign.clone(), "DO_NOT_STORE_SENTINEL".into())
            .is_err()
    );
    assert!(
        actor
            .use_form(foreign.clone(), "DO_NOT_STORE_SENTINEL".into())
            .is_err()
    );
    assert!(actor.action(GuiAction::Enable).await.is_err());
    assert_eq!(actor.settings, foreign);
    assert!(actor.password.is_none());
    assert!(!actor.state_dir.join("gui-settings.toml").exists());
    cleanup(actor);
}

async fn rest_fixture(status: &str, body: &str) -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let n = socket.read(&mut bytes).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&bytes[..n]);
            assert!(request.len() < 4096);
        }
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    });
    (port, task)
}

#[tokio::test]
async fn configured_device_button_uses_current_form_without_saving_or_injecting() {
    let (mut actor, _events) = actor();
    actor.smoke = false;
    let (port, request) = rest_fixture("200 OK", r#"{"product":"C64 Ultimate","firmware_version":"1.1.0","fpga_version":"122","hostname":"fixture","unique_id":"fixture","errors":[]}"#).await;
    let mut form = actor.settings.clone();
    form.device_port = port;
    form.remember_password = true;
    actor
        .action(GuiAction::Configured {
            settings: form,
            password: "CURRENT_FORM_SECRET".into(),
            operation: ConnectionOperation::DeviceInfo,
        })
        .await
        .unwrap();
    let request = String::from_utf8(request.await.unwrap()).unwrap();
    assert!(request.starts_with("GET /v1/info HTTP/1.1"));
    assert!(request.contains("CURRENT_FORM_SECRET"));
    assert_eq!(
        actor.snapshot.device.as_ref().unwrap().product,
        "C64 Ultimate"
    );
    assert!(actor.owned.is_none());
    assert!(!actor.state_dir.join("gui-settings.toml").exists());
    assert!(!actor.settings.remember_password);
    assert!(
        !String::from_utf8(diagnostic_export(&actor.snapshot).unwrap())
            .unwrap()
            .contains("CURRENT_FORM_SECRET")
    );
    cleanup(actor);
}

#[tokio::test]
async fn invalid_current_form_does_not_fall_through_to_saved_connection() {
    let (mut actor, _events) = actor();
    actor.smoke = false;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    actor.settings.device_port = listener.local_addr().unwrap().port();
    let original = actor.settings.clone();
    let mut invalid = original.clone();
    invalid.device_port = 0;
    assert!(
        actor
            .action(GuiAction::Configured {
                settings: invalid,
                password: "INVALID_FORM_SECRET".into(),
                operation: ConnectionOperation::DeviceInfo,
            })
            .await
            .is_err()
    );
    assert_eq!(actor.settings, original);
    assert!(actor.password.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    cleanup(actor);
}

#[tokio::test]
async fn background_service_shows_auth_failure_without_echoing_secret_or_peer_text() {
    let (actor, _events) = actor();
    let state_dir = actor.state_dir.clone();
    let settings = actor.settings.clone();
    config::save_settings(&state_dir, &settings).unwrap();
    drop(actor);
    let (port, request) =
        rest_fixture("403 Forbidden", r#"{"errors":["PEER_SECRET_SENTINEL"]}"#).await;
    let mut service =
        ServiceHandle::spawn(state_dir.clone(), None, false, Arc::new(|| {})).unwrap();
    let mut form = settings;
    form.device_port = port;
    service
        .send(GuiAction::Configured {
            settings: form,
            password: "HEADER_SECRET_SENTINEL".into(),
            operation: ConnectionOperation::DeviceInfo,
        })
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Some(GuiEvent::Error(error)) = service.try_recv() {
                break error;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(error.contains("Device information failed"));
    assert!(error.contains("authentication failed"));
    assert!(!error.contains("SECRET_SENTINEL"));
    request.await.unwrap();
    service.shutdown_and_join().unwrap();
    std::fs::remove_dir_all(state_dir).unwrap();
}

#[tokio::test]
async fn startup_bind_failure_is_visible_and_clears_host_ownership() {
    let (mut actor, _events) = actor();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    actor.settings.bridge_port = listener.local_addr().unwrap().port();
    actor.start(false, false).await.unwrap();
    assert!(actor.owned.as_mut().unwrap().ready().await.is_err());
    actor.refresh_status().await;
    assert!(actor.snapshot.message.contains("port is already in use"));
    assert!(actor.snapshot.message.contains("BRIDGE_PORT"));
    assert!(actor.snapshot.message.contains("Hosting and input are off"));
    assert_eq!(actor.snapshot.phase, GuiPhase::Idle);
    assert!(!actor.snapshot.owned);
    assert!(!actor.state_dir.join("runtime.json").exists());
    cleanup(actor);
}

#[test]
fn displayed_failures_are_bounded_and_omit_chained_secret_details() {
    let error = anyhow::anyhow!("CHAINED_SECRET_SENTINEL").context("Cannot reach C64 REST API");
    assert_eq!(display_error(&error), "Cannot reach C64 REST API");
    assert_eq!(
        display_error(&anyhow::anyhow!("é".repeat(1024)))
            .chars()
            .count(),
        512
    );
}

#[test]
fn missing_import_is_visible_and_unchanged_save_does_not_require_host_restart() {
    let (mut actor, _events) = actor();
    actor.settings.env_file = actor
        .state_dir
        .join("missing.env")
        .to_string_lossy()
        .into_owned();
    actor.configuration_warning();
    assert!(
        actor
            .snapshot
            .configuration_warning
            .contains(".env file was not found")
    );
    assert!(
        actor
            .snapshot
            .configuration_warning
            .contains("Desktop launchers")
    );
    actor.snapshot.status = Some(control::Status::default());
    actor
        .save_form(actor.settings.clone(), String::new())
        .unwrap();
    assert!(!actor.snapshot.settings_pending);
    cleanup(actor);
}

#[tokio::test]
async fn changed_form_pauses_owned_host_and_requires_apply_before_deployment() {
    let (mut actor, _events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    assert!(actor.permit.arm(actor.permit.epoch()));
    assert!(control::send(&actor.state_dir, "resume").await.unwrap().ok);
    actor.refresh_status().await;
    assert!(actor.snapshot.status.as_ref().unwrap().enabled);
    let mut changed = actor.settings.clone();
    changed.bridge_port = if changed.bridge_port == 65535 {
        65534
    } else {
        changed.bridge_port + 1
    };
    let error = actor
        .action(GuiAction::Configured {
            settings: changed,
            password: String::new(),
            operation: ConnectionOperation::Deploy {
                run: true,
                allow_reset: true,
            },
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Apply the changed settings"));
    assert!(actor.snapshot.settings_pending);
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    assert!(actor.action(GuiAction::Enable).await.is_err());
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

fn actor() -> (Actor, mpsc::Receiver<GuiEvent>) {
    let directory = std::env::temp_dir().join(format!(
        "c64-keyboard-bridge-gui-controller-{}",
        config::random_id().unwrap()
    ));
    pairing::prepare_directory(&directory).unwrap();
    let (events, receiver) = mpsc::sync_channel(128);
    let (quit, _) = watch::channel(false);
    let mut actor = Actor::new(
        directory,
        None,
        true,
        events,
        Arc::new(Mutex::new(None)),
        Arc::new(|| {}),
        quit,
    );
    actor.initialize();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    actor.settings.bridge_port = listener.local_addr().unwrap().port();
    drop(listener);
    (actor, receiver)
}
fn cleanup(actor: Actor) {
    let dir = actor.state_dir.clone();
    drop(actor);
    std::fs::remove_dir_all(dir).unwrap();
}

async fn finish_credentials(actor: &mut Actor) {
    let until = Instant::now() + Duration::from_secs(2);
    while actor.pending_credential.is_some() {
        actor.poll_credentials();
        assert!(
            Instant::now() < until,
            "credential fixture did not complete"
        );
        tokio::task::yield_now().await;
    }
}

struct CredentialGate {
    ready: Arc<(Mutex<bool>, std::sync::Condvar)>,
    entered: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    value: Arc<Mutex<Option<String>>>,
}
impl CredentialGate {
    fn release(&self) {
        *self.ready.0.lock().unwrap() = true;
        self.ready.1.notify_all();
    }
    async fn wait_entered(&self) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !self.entered.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for CredentialGate {
    fn drop(&mut self) {
        self.release();
    }
}
struct DelayedCredentialStore {
    ready: Arc<(Mutex<bool>, std::sync::Condvar)>,
    entered: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    value: Arc<Mutex<Option<String>>>,
}
impl DelayedCredentialStore {
    fn wait(&self, name: &'static str) {
        assert!(tokio::runtime::Handle::try_current().is_err());
        self.calls.lock().unwrap().push(name);
        self.entered.store(true, Ordering::Release);
        let mut ready = self.ready.0.lock().unwrap();
        while !*ready {
            ready = self.ready.1.wait(ready).unwrap();
        }
    }
}
impl SecretStore for DelayedCredentialStore {
    fn get(&mut self, _: &str) -> Result<Option<String>> {
        self.wait("get");
        Ok(self.value.lock().unwrap().clone())
    }
    fn set(&mut self, _: &str, password: &str) -> Result<()> {
        self.wait("set");
        *self.value.lock().unwrap() = Some(password.into());
        Ok(())
    }
    fn delete(&mut self, _: &str) -> Result<()> {
        self.wait("delete");
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}
fn delayed_credentials(value: Option<String>) -> (CredentialWorker, CredentialGate) {
    let gate = CredentialGate {
        ready: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
        entered: Arc::new(AtomicBool::new(false)),
        calls: Arc::new(Mutex::new(Vec::new())),
        value: Arc::new(Mutex::new(value)),
    };
    let worker = CredentialWorker::new(Box::new(DelayedCredentialStore {
        ready: gate.ready.clone(),
        entered: gate.entered.clone(),
        calls: gate.calls.clone(),
        value: gate.value.clone(),
    }));
    (worker, gate)
}

#[tokio::test]
async fn slow_secret_save_leaves_session_settings_usable_and_discards_stale_confirmation() {
    let (mut actor, events) = actor();
    let (worker, gate) = delayed_credentials(None);
    actor.store = worker;
    let mut settings = actor.settings.clone();
    settings.remember_password = true;
    actor
        .action(GuiAction::SaveSettings {
            settings,
            password: "FIRST_SECRET_SENTINEL".into(),
        })
        .await
        .unwrap();
    gate.wait_entered().await;
    assert!(actor.snapshot.credential_pending);
    assert!(!actor.snapshot.busy);
    assert!(!actor.settings.remember_password);
    assert_eq!(actor.password.as_deref(), Some("FIRST_SECRET_SENTINEL"));
    let mut next = actor.settings.clone();
    next.remember_password = true;
    next.device_port = 1234;
    actor
        .action(GuiAction::ApplySettings {
            settings: next,
            password: "SECOND_SECRET_SENTINEL".into(),
        })
        .await
        .unwrap();
    assert_eq!(actor.settings.device_port, 1234);
    assert_eq!(actor.password.as_deref(), Some("SECOND_SECRET_SENTINEL"));
    assert_eq!(*gate.calls.lock().unwrap(), ["set"]);
    actor.pending_credential.as_mut().unwrap().deadline = Instant::now();
    actor.poll_credentials();
    assert!(actor.snapshot.message.contains("unconfirmed"));
    assert!(actor.snapshot.credential_pending);
    assert!(actor.action(GuiAction::ForgetPassword).await.is_err());
    actor.action(GuiAction::Doctor).await.unwrap();
    assert!(!actor.snapshot.checks.is_empty());
    gate.release();
    finish_credentials(&mut actor).await;
    assert!(!actor.settings.remember_password);
    assert_eq!(actor.password.as_deref(), Some("SECOND_SECRET_SENTINEL"));
    assert_eq!(
        gate.value.lock().unwrap().as_deref(),
        Some("FIRST_SECRET_SENTINEL")
    );
    assert!(actor.snapshot.message.contains("previous session password"));
    let saved = config::read_regular(&actor.state_dir.join("gui-settings.toml"), true).unwrap();
    assert!(!saved.contains("SECRET_SENTINEL"));
    for event in events.try_iter() {
        assert!(!format!("{event:?}").contains("SECRET_SENTINEL"));
    }
    cleanup(actor);
}

#[tokio::test]
async fn late_password_restore_never_replaces_an_entered_session_password() {
    let (mut actor, _events) = actor();
    let (worker, gate) = delayed_credentials(Some("OLD_SECRET_SENTINEL".into()));
    actor.store = worker;
    actor.settings.remember_password = true;
    actor.restore_password();
    gate.wait_entered().await;
    actor
        .use_form(actor.settings.clone(), "NEW_SECRET_SENTINEL".into())
        .unwrap();
    gate.release();
    finish_credentials(&mut actor).await;
    assert_eq!(actor.password.as_deref(), Some("NEW_SECRET_SENTINEL"));
    assert!(actor.snapshot.message.contains("result ignored"));
    cleanup(actor);
}

#[tokio::test]
async fn late_forget_confirmation_preserves_a_newer_session_password() {
    let (mut actor, events) = actor();
    let (worker, gate) = delayed_credentials(Some("OLD_FORGET_SECRET_SENTINEL".into()));
    actor.store = worker;
    actor.password = Some("OLD_FORGET_SECRET_SENTINEL".into());
    actor.settings.remember_password = true;
    config::save_settings(&actor.state_dir, &actor.settings).unwrap();
    actor.action(GuiAction::ForgetPassword).await.unwrap();
    gate.wait_entered().await;
    actor
        .use_form(actor.settings.clone(), "NEW_SESSION_SECRET_SENTINEL".into())
        .unwrap();
    gate.release();
    finish_credentials(&mut actor).await;
    assert_eq!(
        actor.password.as_deref(),
        Some("NEW_SESSION_SECRET_SENTINEL")
    );
    assert!(!actor.settings.remember_password);
    assert!(gate.value.lock().unwrap().is_none());
    assert!(
        !config::load_settings(&actor.state_dir, None)
            .unwrap()
            .remember_password
    );
    assert!(events.try_iter().any(|event| matches!(
        event,
        GuiEvent::CredentialChanged {
            remember_password: false,
            password_forgotten: false
        }
    )));
    cleanup(actor);
}

async fn wait_snapshot(
    latest: &Arc<Mutex<Option<GuiSnapshot>>>,
    predicate: impl Fn(&GuiSnapshot) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if latest.lock().unwrap().as_ref().is_some_and(&predicate) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn pending_secret_prompt_keeps_pause_stop_start_and_quit_responsive() {
    use bridge_protocol::{Frame, Message, PhysicalKey};
    use tokio::io::AsyncWriteExt;
    let (mut actor, events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    let mut client = authenticated_client(&mut actor).await;
    actor.action(GuiAction::Enable).await.unwrap();
    client
        .write_all(
            &Frame {
                sequence: 2,
                message: Message::KeyDown {
                    key: PhysicalKey::A,
                    modifiers: 0,
                },
            }
            .encode()
            .unwrap(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .events
            == 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (worker, gate) = delayed_credentials(None);
    actor.store = worker;
    let mut settings = actor.settings.clone();
    settings.remember_password = true;
    let (commands, receiver) = async_mpsc::channel(4);
    let (safety, safety_rx) = watch::channel(SafetyState::default());
    let handle = ServiceHandle {
        commands,
        events,
        latest: actor.latest.clone(),
        quit: actor.quit.clone(),
        thread: None,
        safety,
        permit: actor.permit.clone(),
    };
    let latest = actor.latest.clone();
    let directory = actor.state_dir.clone();
    let quit_rx = actor.quit.subscribe();
    let task = tokio::spawn(async move {
        actor.run(receiver, quit_rx, safety_rx).await;
        actor
    });
    handle
        .send(GuiAction::SaveSettings {
            settings,
            password: "PENDING_SECRET_SENTINEL".into(),
        })
        .unwrap();
    gate.wait_entered().await;
    wait_snapshot(&latest, |snapshot| {
        snapshot.credential_pending && !snapshot.busy
    })
    .await;
    handle.send(GuiAction::Pause).unwrap();
    wait_snapshot(&latest, |snapshot| {
        snapshot.status.as_ref().is_some_and(|s| !s.enabled)
    })
    .await;
    assert!(!handle.permit.allow.load(Ordering::Acquire));
    assert!(control::send(&directory, "status").await.unwrap().ok);
    handle.send(GuiAction::Stop).unwrap();
    wait_snapshot(&latest, |snapshot| snapshot.phase == GuiPhase::Idle).await;
    assert!(!directory.join("runtime.json").exists());
    handle
        .send(GuiAction::Start {
            deploy: false,
            allow_reset: false,
        })
        .unwrap();
    wait_snapshot(&latest, |snapshot| snapshot.phase == GuiPhase::Running).await;
    assert!(
        !latest
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .status
            .as_ref()
            .unwrap()
            .enabled
    );
    handle.send(GuiAction::Quit).unwrap();
    let actor = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(actor.snapshot.quit_ready);
    assert!(!actor.snapshot.credential_pending);
    assert!(!actor.settings.remember_password);
    assert!(!directory.join("runtime.json").exists());
    assert_eq!(*gate.calls.lock().unwrap(), ["set"]);
    gate.release();
    cleanup(actor);
}

#[tokio::test]
async fn local_removal_waits_for_delete_and_preserves_files_if_hosting_starts() {
    let (mut actor, _events) = actor();
    let (worker, gate) = delayed_credentials(Some("DELETE_SECRET_SENTINEL".into()));
    actor.store = worker;
    actor.settings.remember_password = true;
    config::save_settings(&actor.state_dir, &actor.settings).unwrap();
    actor
        .action(GuiAction::RemoveLocalData { confirmed: true })
        .await
        .unwrap();
    gate.wait_entered().await;
    assert!(actor.state_dir.join("gui-settings.toml").exists());
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    gate.release();
    finish_credentials(&mut actor).await;
    assert!(actor.owned.is_some());
    assert!(!actor.settings.remember_password);
    assert!(actor.state_dir.join("gui-settings.toml").exists());
    assert!(actor.snapshot.message.contains("removal was cancelled"));
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn partial_local_removal_keeps_confirmed_credential_deletion_visible() {
    let (mut actor, events) = actor();
    actor.settings.remember_password = true;
    actor.password = Some("REMOVAL_SECRET_SENTINEL".into());
    config::save_settings(&actor.state_dir, &actor.settings).unwrap();
    // An unexpected directory at this owned file path makes remove_file fail
    // without needing platform-specific permission changes or root behavior.
    std::fs::create_dir(actor.state_dir.join("pairing.key")).unwrap();
    actor
        .action(GuiAction::RemoveLocalData { confirmed: true })
        .await
        .unwrap();
    finish_credentials(&mut actor).await;
    assert!(!actor.settings.remember_password);
    assert!(actor.password.is_none());
    assert!(actor.state_dir.join("pairing.key").is_dir());
    assert!(actor.snapshot.message.contains("some local files"));
    assert!(events.try_iter().any(|event| matches!(
        event,
        GuiEvent::CredentialChanged {
            remember_password: false,
            password_forgotten: true
        }
    )));
    cleanup(actor);
}

async fn authenticated_client(actor: &mut Actor) -> tokio::net::TcpStream {
    use bridge_protocol::{Frame, MatrixState, Message};
    use tokio::io::AsyncWriteExt;
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", actor.settings.bridge_port))
        .await
        .unwrap();
    for (sequence, message) in [
        Message::Hello {
            token: pairing::read_token(&actor.state_dir).unwrap().0,
            capabilities: 1,
            device_id: "isolated-gui-test".into(),
        },
        Message::StateSync(MatrixState::default()),
    ]
    .into_iter()
    .enumerate()
    {
        client
            .write_all(
                &Frame {
                    sequence: sequence as u16,
                    message,
                }
                .encode()
                .unwrap(),
            )
            .await
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            actor.refresh_status().await;
            if actor
                .snapshot
                .status
                .as_ref()
                .unwrap()
                .diagnostics
                .current
                .phase
                == bridge_host::session_diagnostics::SessionPhase::Active
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    client
}

#[tokio::test]
async fn gui_quit_leaves_external_cli_running_until_explicit_stop() {
    let (mut actor, _events) = actor();
    let config = actor.effective_config().unwrap();
    let mut external = ManagedRuntime::spawn(
        config,
        StartOptions {
            enable_input: false,
            deploy: false,
            allow_reset: false,
            template: KEYBOARD_PRG.to_vec(),
            fake_device: true,
        },
    );
    external.ready().await.unwrap();
    actor.refresh_status().await;
    assert!(!actor.snapshot.owned);
    assert!(actor.snapshot.status.as_ref().unwrap().running);
    let _client = authenticated_client(&mut actor).await;
    // The CLI is the posting process: an unavailable GUI backend must not
    // prevent explicit control of its recording runtime.
    actor.snapshot.backend = None;
    actor.action(GuiAction::Enable).await.unwrap();
    assert!(actor.focused);
    assert!(actor.countdown.is_none());
    assert!(actor.snapshot.status.as_ref().unwrap().enabled);
    actor.stop_owned().await.unwrap();
    assert!(!external.is_finished());
    assert!(actor.state_dir.join("runtime.json").exists());
    assert!(
        control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    actor.action(GuiAction::Stop).await.unwrap();
    external.wait().await.unwrap();
    assert!(!actor.state_dir.join("runtime.json").exists());
    cleanup(actor);
}

#[tokio::test]
async fn mapping_apply_restarts_owned_host_without_reset_or_input_enable() {
    let (mut actor, _events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    assert!(actor.permit.arm(actor.permit.epoch()));
    assert!(control::send(&actor.state_dir, "resume").await.unwrap().ok);
    actor.refresh_status().await;
    assert!(actor.snapshot.status.as_ref().unwrap().enabled);
    let document = MappingDocument::builtin("modern", "UK").unwrap();
    actor
        .action(GuiAction::ApplyProfile { document })
        .await
        .unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.refresh_status().await;
    assert!(actor.snapshot.owned);
    assert!(!actor.snapshot.status.as_ref().unwrap().enabled);
    assert_eq!(actor.settings.layout, "UK");
    assert!(!actor.settings.mappings_file.is_empty());
    assert!(Path::new(&actor.settings.mappings_file).exists());
    actor.stop_owned().await.unwrap();
    assert!(!actor.state_dir.join("runtime.json").exists());
    cleanup(actor);
}

#[tokio::test]
async fn focused_enable_is_immediate_focus_changes_keep_input_on_and_tray_loss_pauses() {
    let (mut actor, events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    let _client = authenticated_client(&mut actor).await;
    assert!(actor.focused);
    actor.action(GuiAction::Enable).await.unwrap();
    assert!(actor.countdown.is_none());
    assert!(actor.snapshot.status.as_ref().unwrap().enabled);
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    actor.action(GuiAction::FocusChanged(true)).await.unwrap();
    assert!(
        control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    assert!(actor.permit.allow.load(Ordering::Acquire));
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    actor.action(GuiAction::TrayAvailable(true)).await.unwrap();
    actor.action(GuiAction::TrayAvailable(false)).await.unwrap();
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    assert!(events.try_iter().any(|e| matches!(e, GuiEvent::ShowWindow)));
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn focused_deadline_and_cancelled_countdown_never_enable_input() {
    let (mut actor, _events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.countdown = Some(Countdown {
        prepared: None,
        end: Instant::now(),
        gate_epoch: actor.permit.epoch(),
        focus_epoch: actor.permit.test_focus_epoch(),
    });
    actor.tick().await;
    assert!(actor.countdown.is_none());
    assert!(actor.snapshot.message.contains("cancelled"));
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    actor.action(GuiAction::TestInput).await.unwrap();
    actor.action(GuiAction::CancelCountdown).await.unwrap();
    assert!(actor.countdown.is_none());
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn immediate_enable_without_authenticated_client_stays_off() {
    let (mut actor, _events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    let error = actor.action(GuiAction::Enable).await.unwrap_err();
    assert!(error.to_string().contains("No authenticated client"));
    assert!(actor.countdown.is_none());
    assert!(!actor.permit.allow.load(Ordering::Acquire));
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn priority_cancel_during_immediate_resume_also_pauses_external_cli() {
    let (mut actor, _events) = actor();
    let token = pairing::load_or_create_token(&actor.state_dir).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    config::atomic_write(
        &actor.state_dir.join("runtime.json"),
        &serde_json::to_vec(&control::RuntimeRecord {
            pid: std::process::id(),
            port: listener.local_addr().unwrap().port(),
        })
        .unwrap(),
    )
    .unwrap();
    let permit = actor.permit.clone();
    let fixture = tokio::spawn(async move {
        for command in ["pause", "status", "resume", "pause"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request: control::ControlRequest = control::read_json(&mut socket).await.unwrap();
            assert_eq!(request.token, token.hex());
            assert_eq!(request.command, command);
            if command == "resume" {
                // The window thread closes this same gate for Pause/Stop/Quit,
                // without waiting for an in-flight control response.
                permit.close();
            }
            control::write_json(
                &mut socket,
                &control::ControlResponse {
                    ok: true,
                    status: control::Status {
                        connected: true,
                        enabled: command == "resume",
                        backend: "recording".into(),
                        ..Default::default()
                    },
                    error: None,
                },
            )
            .await
            .unwrap();
        }
    });
    tokio::time::timeout(Duration::from_secs(2), actor.action(GuiAction::Enable))
        .await
        .unwrap()
        .unwrap();
    fixture.await.unwrap();
    assert!(actor.countdown.is_none());
    assert!(!actor.permit.allow.load(Ordering::Acquire));
    assert!(!actor.snapshot.status.as_ref().unwrap().enabled);
    assert!(
        actor
            .snapshot
            .message
            .contains("cancelled by a newer safety action")
    );
    cleanup(actor);
}

#[tokio::test]
async fn smoke_guards_reject_disruptive_requests_before_contacting_devices() {
    let (mut actor, _events) = actor();
    assert!(
        actor
            .action(GuiAction::Deploy {
                run: true,
                allow_reset: true
            })
            .await
            .is_err()
    );
    assert!(
        actor
            .action(GuiAction::Probe { allow_reset: true })
            .await
            .is_err()
    );
    assert!(actor.action(GuiAction::DeviceInfo).await.is_err());
    assert!(actor.action(GuiAction::RequestPermission).await.is_err());
    assert!(actor.owned.is_none());
    assert!(!actor.state_dir.join("pairing.key").exists());
    cleanup(actor);
}

#[tokio::test]
async fn failed_secret_save_keeps_new_value_in_memory_without_plaintext_settings() {
    let (mut actor, events) = actor();
    actor.store = CredentialWorker::new(Box::new(MemorySecretStore {
        fail_set: true,
        ..Default::default()
    }));
    let mut settings = actor.settings.clone();
    settings.remember_password = true;
    let sentinel = "SECRET_SENTINEL$password";
    actor.save_form(settings, sentinel.into()).unwrap();
    finish_credentials(&mut actor).await;
    assert_eq!(actor.password.as_deref(), Some(sentinel));
    assert!(!actor.settings.remember_password);
    let file = config::read_regular(&actor.state_dir.join("gui-settings.toml"), true).unwrap();
    assert!(!file.contains(sentinel));
    let stored = actor
        .store
        .request(CredentialOperation::Get {
            id: actor.settings.credential_id.clone(),
        })
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(stored, CredentialValue::Password(None)));
    for event in events.try_iter() {
        assert!(!format!("{event:?}").contains(sentinel));
    }
    cleanup(actor);
}

#[tokio::test]
async fn failed_secret_delete_does_not_claim_password_was_forgotten() {
    let (mut actor, _events) = actor();
    actor.settings.remember_password = true;
    actor.password = Some("SECRET_SENTINEL_old".into());
    actor.store = CredentialWorker::new(Box::new(MemorySecretStore {
        value: actor.password.clone(),
        fail_delete: true,
        ..Default::default()
    }));
    config::save_settings(&actor.state_dir, &actor.settings).unwrap();
    actor.action(GuiAction::ForgetPassword).await.unwrap();
    finish_credentials(&mut actor).await;
    assert!(actor.snapshot.message.contains("deletion failed"));
    assert!(actor.settings.remember_password);
    assert!(actor.password.is_some());
    assert!(
        config::load_settings(&actor.state_dir, None)
            .unwrap()
            .remember_password
    );
    cleanup(actor);
}

#[test]
fn diagnostic_export_and_history_are_bounded_and_exclude_peer_secrets_and_keys() {
    let (mut actor, _events) = actor();
    let sentinel = "SECRET_PEER_MATRIX_ADDRESS_KEY";
    let status = control::Status {
        detail: sentinel.into(),
        device: Some(sentinel.into()),
        ..Default::default()
    };
    actor.snapshot.status = Some(status);
    actor.snapshot.device = Some(GuiDeviceInfo {
        product: sentinel.into(),
        firmware: sentinel.into(),
        fpga: sentinel.into(),
        core: sentinel.into(),
    });
    if let Some(backend) = actor.snapshot.backend.as_mut() {
        backend.detail = sentinel.into();
        backend.name = sentinel.into();
    }
    actor.snapshot.message = sentinel.into();
    let export = diagnostic_export(&actor.snapshot).unwrap();
    assert!(export.len() < 4096);
    assert!(!String::from_utf8(export).unwrap().contains(sentinel));
    for _ in 0..1000 {
        actor.note("Safe operation summary");
    }
    assert_eq!(actor.history.len(), 128);
    cleanup(actor);
}

#[test]
fn rapid_focus_flip_closes_only_test_output_and_stale_test_cannot_reopen() {
    let permit = OutputPermit::default();
    permit.focus_changed(false);
    let deadline = permit.epoch();
    let focus_epoch = permit.test_focus_epoch();
    assert!(permit.arm(deadline));
    assert!(permit.arm_test(deadline, focus_epoch));
    permit.focus_changed(true);
    permit.focus_changed(false);
    assert!(permit.allow.load(Ordering::Acquire));
    assert!(!permit.test_allow.load(Ordering::Acquire));
    assert!(!permit.arm_test(deadline, focus_epoch));
    assert!(permit.arm(deadline));
    permit.close();
    assert!(!permit.allow.load(Ordering::Acquire));
    assert!(!permit.test_allow.load(Ordering::Acquire));
    assert!(!permit.arm(deadline));
}

#[test]
fn full_gui_queue_keeps_normal_focus_independent_and_does_not_delay_safety_actions() {
    let (commands, mut receiver) = async_mpsc::channel(64);
    let (events, event_rx) = mpsc::sync_channel(128);
    let (quit, quit_rx) = watch::channel(false);
    let (safety, _) = watch::channel(SafetyState::default());
    let handle = ServiceHandle {
        commands,
        events: event_rx,
        latest: Arc::new(Mutex::new(None)),
        quit,
        thread: None,
        safety,
        permit: Arc::new(OutputPermit::default()),
    };
    for _ in 0..64 {
        handle.send(GuiAction::Doctor).unwrap();
    }
    assert!(handle.send(GuiAction::Doctor).is_err());
    handle.send(GuiAction::FocusChanged(false)).unwrap();
    assert!(handle.permit.arm(handle.permit.epoch()));
    assert!(
        handle
            .permit
            .arm_test(handle.permit.epoch(), handle.permit.test_focus_epoch())
    );
    handle.send(GuiAction::FocusChanged(true)).unwrap();
    handle.send(GuiAction::FocusChanged(false)).unwrap();
    assert!(handle.permit.allow.load(Ordering::Acquire));
    assert!(!handle.permit.test_allow.load(Ordering::Acquire));
    handle.send(GuiAction::Pause).unwrap();
    assert!(!handle.permit.allow.load(Ordering::Acquire));
    handle.send(GuiAction::CancelCountdown).unwrap();
    handle.send(GuiAction::Stop).unwrap();
    handle.send(GuiAction::TrayAvailable(false)).unwrap();
    handle.send(GuiAction::Quit).unwrap();
    assert_eq!(handle.safety.borrow().pause, 1);
    assert_eq!(handle.safety.borrow().stop, 1);
    assert_eq!(handle.safety.borrow().cancel, 3);
    assert!(*quit_rx.borrow());
    assert!(receiver.try_recv().is_ok());
    drop(events);
}

#[tokio::test]
async fn newer_safety_epoch_cancels_countdown_even_before_actor_observes_focus() {
    let (mut actor, _events) = actor();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.action(GuiAction::TestInput).await.unwrap();
    actor.countdown.as_mut().unwrap().end = Instant::now();
    // Simulate the window thread returning focus, then leaving before actor polling.
    actor.permit.focus_changed(true);
    actor.permit.focus_changed(false);
    actor.tick().await;
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    assert!(!actor.permit.allow.load(Ordering::Acquire));
    assert!(!actor.permit.test_allow.load(Ordering::Acquire));
    assert!(actor.countdown.is_none());
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn requesting_input_test_pauses_an_already_enabled_owned_host() {
    let (mut actor, _events) = actor();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    assert!(actor.permit.arm(actor.permit.epoch()));
    assert!(control::send(&actor.state_dir, "resume").await.unwrap().ok);
    actor.action(GuiAction::TestInput).await.unwrap();
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    assert!(actor.countdown.is_some());
    actor.action(GuiAction::CancelCountdown).await.unwrap();
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn external_resume_during_test_countdown_cancels_the_phrase_and_pauses_hosting() {
    let (mut actor, _events) = actor();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.action(GuiAction::TestInput).await.unwrap();
    // Simulate another CLI invocation enabling forwarding during the countdown.
    assert!(actor.permit.arm(actor.permit.epoch()));
    assert!(control::send(&actor.state_dir, "resume").await.unwrap().ok);
    actor.countdown.as_mut().unwrap().end = Instant::now();
    actor.tick().await;
    assert!(actor.countdown.is_none());
    assert!(
        actor
            .snapshot
            .message
            .contains("enabled during its countdown")
    );
    assert!(!actor.permit.allow.load(Ordering::Acquire));
    assert!(!actor.permit.test_allow.load(Ordering::Acquire));
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn explicit_local_data_reset_preserves_imports_and_rejects_stale_forms() {
    let (mut actor, _events) = actor();
    let old = actor.settings.clone();
    config::save_settings(&actor.state_dir, &old).unwrap();
    config::atomic_write(
        &actor.state_dir.join("pairing.key"),
        b"SECRET_TOKEN_SENTINEL",
    )
    .unwrap();
    let imported = actor.state_dir.join("import.env");
    config::atomic_write(&imported, b"C64_NETWORK_PASSWORD=SECRET_EXTERNAL_SENTINEL").unwrap();
    let profile = actor
        .save_profile(&MappingDocument::builtin("modern", "US").unwrap(), None)
        .unwrap();
    let exported = actor.state_dir.join("profiles").join("my-export.toml");
    config::atomic_write(&exported, b"keep").unwrap();
    actor
        .action(GuiAction::RemoveLocalData { confirmed: true })
        .await
        .unwrap();
    finish_credentials(&mut actor).await;
    assert!(!profile.exists());
    assert!(!actor.state_dir.join("pairing.key").exists());
    assert!(imported.exists());
    assert!(exported.exists());
    assert_ne!(actor.settings.credential_id, old.credential_id);
    assert!(actor.settings.env_file.is_empty());
    assert!(
        actor
            .save_form(old, "STALE_PASSWORD_SENTINEL".into())
            .is_err()
    );
    assert!(actor.password.is_none());
    cleanup(actor);
}

#[tokio::test]
async fn failed_credential_reset_preserves_private_files_and_running_start_is_protected() {
    let (mut actor, _events) = actor();
    config::save_settings(&actor.state_dir, &actor.settings).unwrap();
    actor.store = CredentialWorker::new(Box::new(MemorySecretStore {
        fail_delete: true,
        ..Default::default()
    }));
    let before = config::read_regular(&actor.state_dir.join("gui-settings.toml"), true).unwrap();
    actor
        .action(GuiAction::RemoveLocalData { confirmed: true })
        .await
        .unwrap();
    finish_credentials(&mut actor).await;
    assert!(actor.snapshot.message.contains("deletion failed"));
    assert_eq!(
        before,
        config::read_regular(&actor.state_dir.join("gui-settings.toml"), true).unwrap()
    );
    actor.start(false, false).await.unwrap();
    assert!(actor.action(GuiAction::RotatePairing).await.is_err());
    assert!(
        actor
            .action(GuiAction::RemoveLocalData { confirmed: true })
            .await
            .is_err()
    );
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn locked_saved_credentials_do_not_leak_or_silently_fall_back_to_plaintext() {
    let (mut actor, events) = actor();
    actor.settings.remember_password = true;
    actor.store = CredentialWorker::new(Box::new(MemorySecretStore {
        value: Some("LOCKED_SENTINEL_SECRET".into()),
        fail_get: true,
        ..Default::default()
    }));
    actor.restore_password();
    finish_credentials(&mut actor).await;
    assert!(actor.password.is_none());
    assert!(actor.settings.remember_password);
    assert!(actor.snapshot.message.contains("locked or unavailable"));
    assert!(!actor.state_dir.join("gui-settings.toml").exists());
    for event in events.try_iter() {
        assert!(!format!("{event:?}").contains("LOCKED_SENTINEL_SECRET"));
    }
    cleanup(actor);
}

#[tokio::test]
async fn deployment_cannot_overlap_managed_startup_before_its_runtime_record_exists() {
    let (mut actor, _events) = actor();
    actor.start(false, false).await.unwrap();
    assert!(matches!(
        *actor.owned.as_ref().unwrap().subscribe().borrow(),
        RuntimeState::Starting
    ));
    assert!(!actor.state_dir.join("runtime.json").exists());
    let error = actor
        .action(GuiAction::Deploy {
            run: true,
            allow_reset: true,
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Wait for startup"));
    actor.stop_owned().await.unwrap();
    cleanup(actor);
}

#[tokio::test]
async fn desktop_authorization_is_routed_to_the_running_cli_process_and_stays_paused() {
    let (mut actor, _events) = actor();
    actor.smoke = false;
    actor.settings.backend = "portal".into();
    let token = pairing::load_or_create_token(&actor.state_dir).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    config::atomic_write(
        &actor.state_dir.join("runtime.json"),
        &serde_json::to_vec(&control::RuntimeRecord {
            pid: std::process::id(),
            port: listener.local_addr().unwrap().port(),
        })
        .unwrap(),
    )
    .unwrap();
    let fixture = tokio::spawn(async move {
        for command in ["status", "authorize"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request: control::ControlRequest = control::read_json(&mut socket).await.unwrap();
            assert_eq!(request.token, token.hex());
            assert_eq!(request.command, command);
            control::write_json(
                &mut socket,
                &control::ControlResponse {
                    ok: true,
                    status: control::Status {
                        authorization: control::AuthorizationStatus {
                            phase: if command == "authorize" {
                                control::AuthorizationPhase::Pending
                            } else {
                                control::AuthorizationPhase::NotRequested
                            },
                            enable_requested: false,
                        },
                        ..Default::default()
                    },
                    error: None,
                },
            )
            .await
            .unwrap();
        }
    });
    actor.action(GuiAction::RequestPermission).await.unwrap();
    fixture.await.unwrap();
    assert!(actor.owned.is_none());
    assert!(!actor.snapshot.owned);
    let status = actor.snapshot.status.as_ref().unwrap();
    assert_eq!(
        status.authorization.phase,
        control::AuthorizationPhase::Pending
    );
    assert!(!status.enabled);
    assert!(
        actor
            .snapshot
            .message
            .contains("separately running CLI process")
    );
    assert!(!actor.permit.allow.load(Ordering::SeqCst));
    cleanup(actor);
}
#[tokio::test]
async fn desktop_authorization_requires_a_running_posting_process() {
    let (mut actor, _events) = actor();
    actor.smoke = false;
    actor.settings.backend = "portal".into();
    let error = actor
        .action(GuiAction::RequestPermission)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Start hosting with input disabled first")
    );
    assert!(!actor.state_dir.join("runtime.json").exists());
    assert!(actor.owned.is_none());
    cleanup(actor);
}
#[test]
fn tray_acknowledgments_are_typed_monotonic_and_independent_of_error_text() {
    let (mut actor, _events) = actor();
    actor.acknowledge_tray_command(TrayCommand::Stop, CommandOutcome::Completed);
    let stopped = actor.snapshot.tray_command_ack.unwrap();
    assert_eq!(stopped.sequence, 1);
    assert_eq!(stopped.command, TrayCommand::Stop);
    actor.acknowledge_tray_command(TrayCommand::Enable, CommandOutcome::Rejected);
    let rejected = actor.snapshot.tray_command_ack.unwrap();
    assert_eq!(rejected.sequence, 2);
    assert_eq!(rejected.outcome, CommandOutcome::Rejected);
    actor.error("SENTINEL_ERROR_IS_NOT_A_COMMAND_RESULT");
    assert_eq!(actor.snapshot.tray_command_ack, Some(rejected));
    cleanup(actor);
}

struct RetainedTestBackend {
    events: Arc<Mutex<Vec<bridge_core::RecordingEvent>>>,
    drops: Arc<std::sync::atomic::AtomicU64>,
}
impl Drop for RetainedTestBackend {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl bridge_core::KeyboardBackend for RetainedTestBackend {
    fn capabilities(&self) -> bridge_core::BackendCapabilities {
        bridge_core::BackendCapabilities {
            key_events: true,
            text: false,
            repeat: false,
        }
    }
    fn press(
        &mut self,
        key: bridge_core::NativeKey,
    ) -> std::result::Result<(), bridge_core::BackendError> {
        self.events
            .lock()
            .unwrap()
            .push(bridge_core::RecordingEvent::Press(key));
        Ok(())
    }
    fn release(
        &mut self,
        key: bridge_core::NativeKey,
    ) -> std::result::Result<(), bridge_core::BackendError> {
        self.events
            .lock()
            .unwrap()
            .push(bridge_core::RecordingEvent::Release(key));
        Ok(())
    }
    fn repeat(
        &mut self,
        _: bridge_core::NativeKey,
    ) -> std::result::Result<(), bridge_core::BackendError> {
        unreachable!()
    }
}
fn pending_test_fixture(
    actor: &mut Actor,
    grant: tokio::sync::oneshot::Receiver<()>,
    complete_on_cancel: bool,
) -> (
    Arc<Mutex<Vec<bridge_core::RecordingEvent>>>,
    Arc<std::sync::atomic::AtomicU64>,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let drops = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let output = events.clone();
    let releases = drops.clone();
    let (cancellation, mut cancellation_rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        if complete_on_cancel {
            cancellation_rx.changed().await.unwrap();
        } else {
            grant.await.unwrap();
        }
        Ok(Box::new(RetainedTestBackend {
            events: output,
            drops: releases,
        }) as Box<dyn bridge_core::KeyboardBackend>)
    });
    actor.test_preparation = Some(PendingTestPreparation {
        cancellation,
        task,
        gate_epoch: actor.permit.epoch(),
    });
    actor.snapshot.test_authorization_pending = true;
    (events, drops)
}
#[tokio::test]
async fn deliberate_test_countdown_starts_after_consent_and_reuses_retained_backend() {
    let (mut actor, _events) = actor();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    let (grant, ready) = tokio::sync::oneshot::channel();
    let (events, drops) = pending_test_fixture(&mut actor, ready, false);
    actor.poll_test_preparation().await;
    assert!(actor.countdown.is_none());
    assert!(actor.snapshot.test_authorization_pending);
    assert!(events.lock().unwrap().is_empty());
    grant.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    while actor.countdown.is_none() {
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
        actor.poll_test_preparation().await;
    }
    assert!(actor.countdown.as_ref().unwrap().end > Instant::now() + Duration::from_secs(4));
    assert!(events.lock().unwrap().is_empty());
    assert!(!actor.snapshot.test_authorization_pending);
    actor.countdown.as_mut().unwrap().end = Instant::now();
    actor.tick().await;
    assert_eq!(events.lock().unwrap().len(), 40);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(actor.countdown.is_none());
    assert!(!actor.permit.test_allow.load(Ordering::SeqCst));
    cleanup(actor);
}
#[tokio::test]
async fn late_test_approval_after_quit_is_closed_and_joined_without_countdown_or_input() {
    let (mut actor, _events) = actor();
    let (_grant, ready) = tokio::sync::oneshot::channel();
    let (events, drops) = pending_test_fixture(&mut actor, ready, true);
    assert!(actor.try_quit().await);
    assert!(actor.test_preparation.is_none());
    assert!(actor.cancelled_test_preparations.is_empty());
    assert!(actor.countdown.is_none());
    assert!(actor.snapshot.quit_ready);
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    cleanup(actor);
}
#[tokio::test]
async fn consent_dialog_focus_return_starts_fresh_countdown_and_later_gui_focus_cancels() {
    let (mut actor, _events) = actor();
    let (grant, ready) = tokio::sync::oneshot::channel();
    let (events, drops) = pending_test_fixture(&mut actor, ready, false);
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    actor.action(GuiAction::FocusChanged(true)).await.unwrap();
    actor.poll_test_preparation().await;
    assert!(actor.test_preparation.is_some());
    assert!(actor.countdown.is_none());
    assert!(events.lock().unwrap().is_empty());
    grant.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    while actor.countdown.is_none() {
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
        actor.poll_test_preparation().await;
    }
    assert_eq!(
        actor.countdown.as_ref().unwrap().focus_epoch,
        actor.permit.test_focus_epoch()
    );
    assert!(!actor.permit.test_allow.load(Ordering::SeqCst));
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    assert!(actor.countdown.is_some());
    actor.action(GuiAction::FocusChanged(true)).await.unwrap();
    assert!(actor.countdown.is_none());
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    cleanup(actor);
}
#[tokio::test]
async fn prepared_test_closed_guard_never_types_and_always_drops_session() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let drops = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let output = Box::new(RetainedTestBackend {
        events: events.clone(),
        drops: drops.clone(),
    });
    let error =
        operations::test_input_prepared(output, Some(Arc::new(AtomicBool::new(false))), None)
            .await
            .unwrap_err();
    assert!(error.to_string().contains("guard is closed"));
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stop_then_unauthenticated_enable_emits_ordered_completed_and_rejected_acks() {
    let (mut actor, _events) = actor();
    actor.permit.close();
    let next = SafetyState {
        stop: 1,
        cancel: 1,
        ..Default::default()
    };
    actor.apply_safety(next).await;
    let stopped = actor.snapshot.tray_command_ack.unwrap();
    assert_eq!(stopped.command, TrayCommand::Stop);
    assert_eq!(stopped.outcome, CommandOutcome::Completed);
    let latest = actor.latest.clone();
    let gate_epoch = actor.permit.epoch();
    let focus_epoch = actor.permit.test_focus_epoch();
    let quit = actor.quit.clone();
    let quit_rx = quit.subscribe();
    let (_safety, safety_rx) = watch::channel(next);
    let (commands, receiver) = async_mpsc::channel(4);
    commands
        .send(QueuedAction {
            action: GuiAction::Enable,
            cancel_epoch: next.cancel,
            auto_pause_epoch: next.auto_pause,
            gate_epoch,
            test_focus_epoch: focus_epoch,
        })
        .await
        .unwrap();
    let task = tokio::spawn(async move {
        actor.run(receiver, quit_rx, safety_rx).await;
        actor
    });
    let until = Instant::now() + Duration::from_secs(1);
    loop {
        let ack = latest
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|snapshot| snapshot.tray_command_ack);
        if let Some(ack) = ack.filter(|ack| ack.sequence > stopped.sequence) {
            assert_eq!(ack.command, TrayCommand::Enable);
            assert_eq!(ack.outcome, CommandOutcome::Rejected);
            break;
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    }
    quit.send_replace(true);
    cleanup(task.await.unwrap());
}

#[tokio::test]
async fn settings_change_cancels_pending_test_consent_and_late_grant_cannot_start_countdown() {
    let (mut actor, _events) = actor();
    let (_grant, ready) = tokio::sync::oneshot::channel();
    let (events, drops) = pending_test_fixture(&mut actor, ready, true);
    let mut settings = actor.settings.clone();
    settings.device_port = 81;
    actor
        .action(GuiAction::SaveSettings {
            settings,
            password: String::new(),
        })
        .await
        .unwrap();
    assert!(actor.test_preparation.is_none());
    assert!(!actor.snapshot.test_authorization_pending);
    actor.join_cancelled_test_preparations().await;
    actor.poll_test_preparation().await;
    assert!(actor.countdown.is_none());
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    cleanup(actor);
}
