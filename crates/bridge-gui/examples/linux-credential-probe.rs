//! Exercise the real GUI credential adapter against the isolated, unavailable
//! Secret Service created by scripts/credential-smoke.py. Never use a live vault.
#[cfg(target_os = "linux")]
fn main() {
    use bridge_gui::{config, model::*, service::ServiceHandle};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    assert_eq!(
        std::env::var("C64_KEYBOARD_BRIDGE_PRIVATE_CREDENTIAL_PROBE").as_deref(),
        Ok("1"),
        "Run through scripts/credential-smoke.py"
    );
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("Private state path required"),
    );
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let settings = GuiSettings {
        env_file: String::new(),
        backend: "recording".into(),
        bridge_port: port,
        remember_password: true,
        credential_id: config::random_id().unwrap(),
        ..Default::default()
    };
    config::save_settings(&directory, &settings).unwrap();
    let mut service =
        ServiceHandle::spawn(directory.clone(), None, false, Arc::new(|| {})).unwrap();
    let sentinel = "PRIVATE_CREDENTIAL_PROBE_SECRET_SENTINEL";
    let mut restore_unavailable = false;
    let mut save_unavailable = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut next = 0;
    loop {
        assert!(
            Instant::now() < deadline,
            "Credential operations must leave the GUI responsive"
        );
        match service.try_recv() {
            Some(GuiEvent::Error(error)) => {
                assert!(!error.contains(sentinel));
                assert!(
                    !error.contains("panicked"),
                    "The actual keyring call must return normally outside Tokio"
                );
                if error.starts_with("Saved password is locked or unavailable") {
                    restore_unavailable = true;
                } else if error.starts_with("Password could not be saved in the OS store") {
                    save_unavailable = true;
                } else {
                    panic!("Unexpected isolated credential failure category");
                }
            }
            Some(GuiEvent::Snapshot(snapshot))
                if !snapshot.busy && !snapshot.credential_pending =>
            {
                match next {
                    0 if restore_unavailable => {
                        assert_eq!(snapshot.phase, GuiPhase::Idle);
                        service
                            .send(GuiAction::SaveSettings {
                                settings: settings.clone(),
                                password: sentinel.into(),
                            })
                            .unwrap();
                        next = 1;
                    }
                    1 if save_unavailable => {
                        service
                            .send(GuiAction::Start {
                                deploy: false,
                                allow_reset: false,
                            })
                            .unwrap();
                        next = 2;
                    }
                    2 if snapshot.phase == GuiPhase::Running => {
                        assert!(snapshot.owned);
                        assert!(!snapshot.status.as_ref().unwrap().enabled);
                        service.send(GuiAction::Pause).unwrap();
                        service.send(GuiAction::Stop).unwrap();
                        next = 3;
                    }
                    3 if snapshot.phase == GuiPhase::Idle && !snapshot.owned => break,
                    _ => {}
                }
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    service.shutdown_and_join().unwrap();
    assert!(!directory.join("runtime.json").exists());
    assert!(
        !std::fs::read_to_string(directory.join("gui-settings.toml"))
            .unwrap()
            .contains(sentinel)
    );
    println!(
        "{{\"real_gui_credential_adapter\":true,\"unavailable_store_reported\":true,\"hosting_and_cleanup_responsive\":true,\"native_input\":false,\"device_contacted\":false,\"live_vault_accessed\":false}}"
    );
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("Linux isolated credential regression is not applicable on this target");
}
