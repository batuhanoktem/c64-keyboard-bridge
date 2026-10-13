//! Exercise the actual desktop service's native, read-only initialization and
//! Doctor paths. No GUI smoke substitutes, vault access, C64 contact or input.
#[cfg(target_os = "macos")]
fn main() {
    use bridge_gui::{config, model::*, service::ServiceHandle};
    use bridge_host::{backend, pairing};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    let require_layout = std::env::args().any(|arg| arg == "--require-layout");
    backend::run_with_main_thread_loop(move || {
        let directory = std::env::temp_dir().join(format!(
            "c64-keyboard-bridge-native-service-{}", config::random_id().unwrap()
        ));
        pairing::prepare_directory(&directory).unwrap();
        let settings = GuiSettings {
            env_file: String::new(),
            backend: "macos".into(),
            credential_id: config::random_id().unwrap(),
            ..Default::default()
        };
        config::save_settings(&directory, &settings).unwrap();
        let mut service = ServiceHandle::spawn(directory.clone(), None, false, Arc::new(|| {})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut doctor_requested = false;
        let mut layout_verified = false;
        loop {
            assert!(Instant::now() < deadline, "Native desktop service must remain responsive");
            match service.try_recv() {
                Some(GuiEvent::Snapshot(snapshot)) => {
                    if let Some(probe) = &snapshot.backend {
                        assert_eq!(probe.name, "macos");
                        assert!(probe.available);
                        assert!(!probe.detail.contains("main-thread query timed out"));
                        assert!(!probe.detail.contains("main-thread service is unavailable"));
                        layout_verified = probe.layout_matches.is_some();
                        if !doctor_requested {
                            service.send(GuiAction::Doctor).unwrap();
                            doctor_requested = true;
                        }
                    }
                    if doctor_requested && !snapshot.checks.is_empty() && !snapshot.busy {
                        assert!(snapshot.checks.iter().any(|check| check.name == "Layout"));
                        assert!(snapshot.checks.iter().any(|check| check.name == "REST API" && check.state == "not configured"));
                        assert_eq!(snapshot.phase, GuiPhase::Idle);
                        assert!(!snapshot.owned);
                        assert!(snapshot.status.is_none());
                        break;
                    }
                }
                Some(GuiEvent::Error(_)) => panic!("Read-only desktop service failed"),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        service.shutdown_and_join().unwrap();
        assert!(!directory.join("runtime.json").exists());
        assert!(!directory.join("pairing.key").exists());
        std::fs::remove_dir_all(directory).unwrap();
        if require_layout { assert!(layout_verified, "Select a standard US/UK input source for the strict native check"); }
        println!("{{\"real_gui_service\":true,\"native_layout_verified\":{layout_verified},\"doctor_completed\":true,\"native_input\":false,\"permission_requested\":false,\"device_contacted\":false,\"credential_store_accessed\":false}}");
    }).expect("Native GUI service regression must finish");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS native GUI service regression is not applicable on this target");
}
