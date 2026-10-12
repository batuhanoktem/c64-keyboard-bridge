use super::*;

fn actor() -> (Actor, mpsc::Receiver<GuiEvent>) {
    let directory = std::env::temp_dir().join(format!(
        "c64bridge-gui-controller-{}",
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
    actor.stop_owned().await.unwrap();
    assert!(!external.is_finished());
    assert!(actor.state_dir.join("runtime.json").exists());
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
async fn returning_gui_focus_and_losing_tray_pause_owned_input() {
    let (mut actor, events) = actor();
    actor.start(false, false).await.unwrap();
    actor.owned.as_mut().unwrap().ready().await.unwrap();
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    assert!(actor.permit.arm(actor.permit.epoch()));
    control::send(&actor.state_dir, "resume").await.unwrap();
    actor.action(GuiAction::FocusChanged(true)).await.unwrap();
    assert!(
        !control::send(&actor.state_dir, "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    actor.action(GuiAction::FocusChanged(false)).await.unwrap();
    assert!(actor.permit.arm(actor.permit.epoch()));
    control::send(&actor.state_dir, "resume").await.unwrap();
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
        end: Instant::now(),
        test: true,
        gate_epoch: actor.permit.epoch(),
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
    actor.action(GuiAction::Enable).await.unwrap();
    actor.action(GuiAction::CancelCountdown).await.unwrap();
    assert!(actor.countdown.is_none());
    actor.stop_owned().await.unwrap();
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

#[test]
fn failed_secret_save_keeps_new_value_in_memory_without_plaintext_settings() {
    let (mut actor, events) = actor();
    actor.store = Box::new(MemorySecretStore {
        fail_set: true,
        ..Default::default()
    });
    let mut settings = actor.settings.clone();
    settings.remember_password = true;
    let sentinel = "SECRET_SENTINEL$password";
    actor.save_form(settings, sentinel.into()).unwrap();
    assert_eq!(actor.password.as_deref(), Some(sentinel));
    assert!(!actor.settings.remember_password);
    let file = config::read_regular(&actor.state_dir.join("gui-settings.toml"), true).unwrap();
    assert!(!file.contains(sentinel));
    assert!(
        actor
            .store
            .get(&actor.settings.credential_id)
            .unwrap()
            .is_none()
    );
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
    actor.store = Box::new(MemorySecretStore {
        value: actor.password.clone(),
        fail_delete: true,
        ..Default::default()
    });
    config::save_settings(&actor.state_dir, &actor.settings).unwrap();
    assert!(actor.action(GuiAction::ForgetPassword).await.is_err());
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
fn rapid_focus_flip_and_stale_countdown_cannot_reopen_output() {
    let permit = OutputPermit::default();
    permit.focus_changed(false);
    let deadline = permit.epoch();
    assert!(permit.arm(deadline));
    permit.focus_changed(true);
    permit.focus_changed(false);
    assert!(!permit.allow.load(Ordering::Acquire));
    assert!(!permit.arm(deadline));
    assert!(permit.arm(permit.epoch()));
    permit.close();
    assert!(!permit.allow.load(Ordering::Acquire));
}

#[test]
fn full_gui_queue_does_not_delay_focus_pause_stop_or_quit() {
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
    handle.send(GuiAction::FocusChanged(true)).unwrap();
    handle.send(GuiAction::FocusChanged(false)).unwrap();
    assert!(!handle.permit.allow.load(Ordering::Acquire));
    handle.send(GuiAction::Pause).unwrap();
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
    actor.action(GuiAction::Enable).await.unwrap();
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
    actor.store = Box::new(MemorySecretStore {
        fail_delete: true,
        ..Default::default()
    });
    let before = config::read_regular(&actor.state_dir.join("gui-settings.toml"), true).unwrap();
    assert!(
        actor
            .action(GuiAction::RemoveLocalData { confirmed: true })
            .await
            .is_err()
    );
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

#[test]
fn locked_saved_credentials_do_not_leak_or_silently_fall_back_to_plaintext() {
    let (mut actor, events) = actor();
    actor.settings.remember_password = true;
    actor.store = Box::new(MemorySecretStore {
        value: Some("LOCKED_SENTINEL_SECRET".into()),
        fail_get: true,
        ..Default::default()
    });
    actor.restore_password();
    assert!(actor.password.is_none());
    assert!(actor.settings.remember_password);
    assert!(actor.snapshot.message.contains("locked or unavailable"));
    assert!(!actor.state_dir.join("gui-settings.toml").exists());
    for event in events.try_iter() {
        assert!(!format!("{event:?}").contains("LOCKED_SENTINEL_SECRET"));
    }
    cleanup(actor);
}
