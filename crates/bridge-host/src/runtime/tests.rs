use super::*;
use bridge_core::{BackendCapabilities, BackendError, NativeKey, RecordingEvent};
use bridge_protocol::PhysicalKey;
use std::{collections::BTreeMap, sync::atomic::AtomicBool};

#[derive(Default)]
struct Trace {
    events: Mutex<Vec<RecordingEvent>>,
    block_press: AtomicBool,
    block_key: Mutex<Option<NativeKey>>,
    entered: AtomicBool,
    fail_release: AtomicBool,
    fail_layout: AtomicBool,
}
struct TestBackend(Arc<Trace>);
impl KeyboardBackend for TestBackend {
    fn validate_layout(&mut self) -> std::result::Result<(), BackendError> {
        if self.0.fail_layout.load(Ordering::SeqCst) {
            Err(BackendError::Unsupported("recorded layout mismatch".into()))
        } else {
            Ok(())
        }
    }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            key_events: true,
            text: true,
            repeat: true,
        }
    }
    fn press(&mut self, key: NativeKey) -> std::result::Result<(), BackendError> {
        self.0
            .events
            .lock()
            .unwrap()
            .push(RecordingEvent::Press(key));
        self.0.entered.store(true, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(2);
        let blocked_key = *self.0.block_key.lock().unwrap();
        while blocked_key.is_none_or(|blocked| blocked == key)
            && self.0.block_press.load(Ordering::SeqCst)
            && Instant::now() < until
        {
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
    fn release(&mut self, key: NativeKey) -> std::result::Result<(), BackendError> {
        if self.0.fail_release.load(Ordering::SeqCst) {
            return Err(BackendError::Injection("recorded release failure".into()));
        }
        self.0
            .events
            .lock()
            .unwrap()
            .push(RecordingEvent::Release(key));
        Ok(())
    }
    fn repeat(&mut self, key: NativeKey) -> std::result::Result<(), BackendError> {
        self.0
            .events
            .lock()
            .unwrap()
            .push(RecordingEvent::Repeat(key));
        Ok(())
    }
    fn type_text(&mut self, text: &str) -> std::result::Result<(), BackendError> {
        self.0
            .events
            .lock()
            .unwrap()
            .push(RecordingEvent::Text(text.into()));
        Ok(())
    }
}
struct Harness {
    worker: WorkerHandle,
    thread: Option<InputThread>,
    status: SharedStatus,
    physical: SharedMatrix,
    trace: Arc<Trace>,
}
impl Harness {
    fn new() -> Self {
        let config = Config::from_values(
            BTreeMap::from([
                ("INPUT_BACKEND".into(), "recording".into()),
                ("REPEAT_DELAY_MS".into(), "100".into()),
            ]),
            BTreeMap::new(),
            crate::config::Overrides::default(),
        )
        .unwrap();
        let status = Arc::new(Mutex::new(Status::default()));
        let physical = Arc::new(Mutex::new(MatrixState::default()));
        let trace = Arc::new(Trace::default());
        let output = trace.clone();
        let (worker, thread) =
            spawn_worker_with_factory(&config, status.clone(), physical.clone(), move || {
                Ok(Box::new(TestBackend(output.clone())))
            })
            .unwrap();
        Self {
            worker,
            thread: Some(thread),
            status,
            physical,
            trace,
        }
    }
    fn events(&self) -> Vec<RecordingEvent> {
        self.trace.events.lock().unwrap().clone()
    }
    fn event(&self, key: PhysicalKey, pressed: bool) {
        let modifiers = {
            let mut state = self.physical.lock().unwrap();
            state.set(key, pressed);
            state.modifiers()
        };
        self.worker
            .event(KeyEvent {
                key,
                pressed,
                modifiers,
            })
            .unwrap();
    }
    async fn wait_for(&self, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "Recording worker did not reach expected state: {:?}",
                self.events()
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
    async fn client(&self, token: Token) -> Client {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (stop_tx, stop_rx) = watch::channel(false);
        let worker = self.worker.clone();
        let status = self.status.clone();
        let physical = self.physical.clone();
        let task = tokio::spawn(async move {
            let result = session(
                server,
                token,
                worker.clone(),
                status,
                physical.clone(),
                stop_rx,
            )
            .await;
            *physical.lock().unwrap() = MatrixState::default();
            worker.reset();
            result
        });
        Client {
            socket,
            sequence: Sequencer::new(),
            task,
            _stop: stop_tx,
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.trace.block_press.store(false, Ordering::SeqCst);
        let _ = self.worker.controls.send(WorkerControl::Shutdown);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap().unwrap();
        }
    }
}
struct Client {
    socket: TcpStream,
    sequence: Sequencer,
    task: tokio::task::JoinHandle<Result<()>>,
    _stop: watch::Sender<bool>,
}
impl Client {
    async fn send(&mut self, message: Message) {
        write_frame(&mut self.socket, &mut self.sequence, message)
            .await
            .unwrap();
    }
    async fn hello(&mut self, token: Token) {
        self.send(Message::Hello {
            token: token.0,
            capabilities: 1,
            device_id: "fake-c64".into(),
        })
        .await;
        assert!(matches!(
            FrameReader::default()
                .next(&mut self.socket)
                .await
                .unwrap()
                .message,
            Message::HelloAck { .. }
        ));
    }
    async fn sync(&mut self, matrix: MatrixState) {
        self.send(Message::StateSync(matrix)).await;
    }
    async fn finish(mut self) {
        self.send(Message::Disconnect { reason: 0 }).await;
        self.task.await.unwrap().unwrap();
    }
}
const TOKEN: Token = Token([42; 16]);

#[tokio::test]
async fn reset_preserves_events_from_new_synchronization_generation() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.trace.block_press.store(true, Ordering::SeqCst);
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.trace.entered.load(Ordering::SeqCst)).await;
    h.worker.reset();
    h.worker.synchronize(MatrixState::default());
    h.worker
        .event(KeyEvent {
            key: PhysicalKey::B,
            pressed: true,
            modifiers: 0,
        })
        .unwrap();
    h.trace.block_press.store(false, Ordering::SeqCst);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::B)))
        .await;
    assert!(h.events().contains(&RecordingEvent::Release(NativeKey::A)));
}

#[tokio::test]
async fn overflow_ends_generation_and_releases_owned_keys() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.trace.block_press.store(true, Ordering::SeqCst);
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.trace.entered.load(Ordering::SeqCst)).await;
    for _ in 0..256 {
        h.worker
            .event(KeyEvent {
                key: PhysicalKey::B,
                pressed: true,
                modifiers: 0,
            })
            .unwrap();
    }
    assert!(
        h.worker
            .event(KeyEvent {
                key: PhysicalKey::A,
                pressed: false,
                modifiers: 0
            })
            .unwrap_err()
            .to_string()
            .contains("overflow")
    );
    h.worker.reset();
    h.trace.block_press.store(false, Ordering::SeqCst);
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    assert!(!h.events().contains(&RecordingEvent::Press(NativeKey::B)));
}

#[tokio::test]
async fn pause_releases_ordinary_keys_before_shared_modifiers() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::Control, true);
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.worker.enable(false).await.unwrap();
    let events = h.events();
    let key = events
        .iter()
        .position(|e| *e == RecordingEvent::Release(NativeKey::A))
        .unwrap();
    let modifier = events
        .iter()
        .position(|e| *e == RecordingEvent::Release(NativeKey::Control))
        .unwrap();
    assert!(key < modifier);
    assert!(!h.status.lock().unwrap().enabled);
}

#[tokio::test]
async fn backend_release_failure_disables_input_and_reports_failure() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.trace.fail_release.store(true, Ordering::SeqCst);
    assert!(h.worker.enable(false).await.is_err());
    assert!(!h.status.lock().unwrap().enabled);
    assert!(h.status.lock().unwrap().detail.contains("failed"));
    h.trace.fail_release.store(false, Ordering::SeqCst);
    h.worker.enable(false).await.unwrap();
    assert!(h.events().contains(&RecordingEvent::Release(NativeKey::A)));
}

#[tokio::test]
async fn authentication_precedes_all_input_and_connection_admission() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.send(Message::Hello {
        token: [0; 16],
        capabilities: 1,
        device_id: "fake".into(),
    })
    .await;
    assert!(
        c.task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("authentication")
    );
    assert!(h.events().is_empty());
    assert!(!h.status.lock().unwrap().connected);
}

#[tokio::test]
async fn pause_during_handshake_is_not_undone_by_initial_snapshot() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    h.worker.enable(false).await.unwrap();
    c.sync(MatrixState::default()).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.status.lock().unwrap().events == 1).await;
    assert!(!h.status.lock().unwrap().enabled);
    assert!(h.events().is_empty());
    c.finish().await;
}

#[tokio::test]
async fn reconnect_quarantines_held_keys_until_release() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    c.finish().await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    let mut held = MatrixState::default();
    held.set(PhysicalKey::A, true);
    c.sync(held).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::B,
        modifiers: 0,
    })
    .await;
    c.send(Message::KeyUp {
        key: PhysicalKey::B,
        modifiers: 0,
    })
    .await;
    c.send(Message::KeyUp {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    c.send(Message::KeyDown {
        key: PhysicalKey::B,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::B)))
        .await;
    assert_eq!(
        h.events()
            .iter()
            .filter(|e| **e == RecordingEvent::Press(NativeKey::A))
            .count(),
        1
    );
    assert_eq!(
        h.events()
            .iter()
            .filter(|e| **e == RecordingEvent::Press(NativeKey::B))
            .count(),
        1
    );
    c.finish().await;
}

#[tokio::test]
async fn mismatched_heartbeat_ends_session_and_releases_input() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    c.send(Message::Heartbeat(MatrixState::default())).await;
    assert!(
        c.task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("disagrees")
    );
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
}

#[tokio::test]
async fn host_repeat_stops_on_physical_release() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Repeat(NativeKey::A)))
        .await;
    h.event(PhysicalKey::A, false);
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    let count = h.events().len();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(h.events().len(), count);
}

#[tokio::test]
async fn queued_release_precedes_expired_host_repeat() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    *h.trace.block_key.lock().unwrap() = Some(NativeKey::LeftShift);
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;

    // Stall a modifier transition without replacing A's repeat deadline. Queue
    // its release while the worker is still gated and its deadline has expired.
    h.trace.block_press.store(true, Ordering::SeqCst);
    h.event(PhysicalKey::LeftShift, true);
    h.wait_for(|| {
        h.events()
            .contains(&RecordingEvent::Press(NativeKey::LeftShift))
    })
    .await;
    let mut expected = h.events();
    tokio::time::sleep(Duration::from_millis(150)).await;
    h.event(PhysicalKey::A, false);
    h.trace.block_press.store(false, Ordering::SeqCst);
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    h.event(PhysicalKey::LeftShift, false);
    h.wait_for(|| {
        h.events()
            .contains(&RecordingEvent::Release(NativeKey::LeftShift))
    })
    .await;

    expected.extend([
        RecordingEvent::Release(NativeKey::A),
        RecordingEvent::Release(NativeKey::LeftShift),
    ]);
    assert_eq!(
        h.events(),
        expected,
        "An expired repeat must not overtake an already queued key release"
    );
}

#[tokio::test]
async fn control_stop_replies_even_when_cleanup_fails() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.trace.fail_release.store(true, Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = watch::channel(false);
    let task = tokio::spawn(serve_control(
        listener,
        TOKEN,
        h.worker.clone(),
        h.status.clone(),
        tx,
    ));
    let mut socket = TcpStream::connect(address).await.unwrap();
    control::write_json(
        &mut socket,
        &ControlRequest {
            token: TOKEN.hex(),
            command: "stop".into(),
        },
    )
    .await
    .unwrap();
    let response: ControlResponse = control::read_json(&mut socket).await.unwrap();
    assert!(!response.ok);
    task.await.unwrap().unwrap();
    assert!(*rx.borrow());
    h.trace.fail_release.store(false, Ordering::SeqCst);
}

#[tokio::test]
async fn watchdog_releases_after_three_seconds_without_valid_traffic() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    // Advance the transport clock only after the real recording worker has admitted A.
    tokio::time::pause();
    tokio::time::advance(SESSION_TIMEOUT + Duration::from_millis(1)).await;
    assert!(
        c.task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
    tokio::time::resume();
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
}

#[tokio::test]
async fn disagreeing_mid_session_snapshot_ends_session_and_releases_without_presses() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    let mut held = MatrixState::default();
    held.set(PhysicalKey::B, true);
    c.sync(held).await;
    let error = c.task.await.unwrap().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("State synchronization disagrees")
    );
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    assert!(!h.events().contains(&RecordingEvent::Press(NativeKey::B)));
}

#[tokio::test]
async fn agreeing_snapshot_releases_and_quarantines_until_fresh_physical_press() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    let mut held = MatrixState::default();
    held.set(PhysicalKey::A, true);
    c.sync(held).await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    assert_eq!(
        h.events()
            .iter()
            .filter(|e| **e == RecordingEvent::Press(NativeKey::A))
            .count(),
        1
    );
    c.send(Message::KeyUp {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    c.send(Message::KeyDown {
        key: PhysicalKey::A,
        modifiers: 0,
    })
    .await;
    h.wait_for(|| {
        h.events()
            .iter()
            .filter(|e| **e == RecordingEvent::Press(NativeKey::A))
            .count()
            == 2
    })
    .await;
    c.finish().await;
}

#[tokio::test]
async fn layout_change_during_hold_releases_keys_and_disables_input() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.trace.fail_layout.store(true, Ordering::SeqCst);
    h.wait_for(|| h.events().contains(&RecordingEvent::Release(NativeKey::A)))
        .await;
    assert!(!h.status.lock().unwrap().enabled);
    assert!(h.status.lock().unwrap().detail.contains("layout change"));
}

#[tokio::test]
async fn failed_shutdown_release_returns_failure_and_preserves_diagnostic() {
    let mut h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.trace.fail_release.store(true, Ordering::SeqCst);
    h.worker.controls.send(WorkerControl::Shutdown).unwrap();
    let error = finish_worker(h.thread.take().unwrap(), Ok(())).unwrap_err();
    assert!(format!("{error:#}").contains("native keys may remain held"));
    assert!(
        h.status
            .lock()
            .unwrap()
            .detail
            .contains("Shutdown cleanup failed")
    );
    assert!(!h.status.lock().unwrap().enabled);
}

#[tokio::test]
async fn layout_failure_reports_secondary_release_failure() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.trace.fail_release.store(true, Ordering::SeqCst);
    h.trace.fail_layout.store(true, Ordering::SeqCst);
    h.wait_for(|| !h.status.lock().unwrap().enabled).await;
    let detail = h.status.lock().unwrap().detail.clone();
    assert!(detail.contains("host layout change"));
    assert!(detail.contains("cleanup failed; native keys may remain held"));
    h.trace.fail_release.store(false, Ordering::SeqCst);
}

#[test]
fn rejects_mapping_layout_that_differs_from_guarded_host_layout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mapping.toml");
    std::fs::write(
        &path,
        "name = 'UK test'\nprofile = 'modern'\nlayout = 'UK'\n",
    )
    .unwrap();
    let config = Config::from_values(
        BTreeMap::from([("MAPPINGS_FILE".into(), path.to_str().unwrap().into())]),
        BTreeMap::new(),
        crate::config::Overrides::default(),
    )
    .unwrap();
    assert!(
        mapping_profile(&config)
            .unwrap_err()
            .to_string()
            .contains("match KEYBOARD_LAYOUT")
    );
}

#[test]
fn probe_telemetry_validates_shape_and_preserves_signed_counts() {
    let mut bytes = [0; 97];
    bytes[..4].copy_from_slice(b"PB\x01\x1f");
    bytes[4..9].copy_from_slice(b"0.1.0");
    bytes[9] = 3;
    bytes[10..13].copy_from_slice(b"NET");
    for row in 0..5 {
        let entry = &mut bytes[32 + row * 13..32 + (row + 1) * 13];
        entry[0] = row as u8 + 1;
        entry[1..6].copy_from_slice(b"00,OK");
    }
    bytes[32 + 4 * 13 + 9] = 255;
    bytes[32 + 4 * 13 + 10] = 255;
    let result = probe_observations(&bytes).unwrap();
    assert_eq!(result["transactions"][4]["count_or_handle"], -1);
    assert_eq!(result["network_target_identity"], "NET");
    bytes[3] = 0;
    assert!(probe_observations(&bytes).is_err());
}

#[tokio::test]
async fn probe_failure_identifies_stage_and_partial_byte_count() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    client.write_all(b"KB").await.unwrap();
    client.shutdown().await.unwrap();
    let mut bytes = [0; 8];
    let error = probe_read(
        &mut server,
        &mut bytes,
        "HELLO header",
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("HELLO header (2/8 bytes)"));
}

#[tokio::test]
async fn fake_probe_authenticates_and_exchanges_binary_ack_idle_observations() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let token = Token([7; 16]);
    let network = NetworkConfig {
        device_ip: std::net::Ipv4Addr::LOCALHOST,
        advertise_ip: std::net::Ipv4Addr::LOCALHOST,
        bind: std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, address.port()),
        allowed_ips: vec![std::net::Ipv4Addr::LOCALHOST],
    };
    let device = DeviceInfo {
        product: "C64 Ultimate".into(),
        firmware_version: "test".into(),
        fpga_version: "test".into(),
        core_version: None,
        hostname: "fake".into(),
        unique_id: "fake".into(),
        errors: Vec::new(),
    };
    let task =
        tokio::spawn(async move { hardware_probe(listener, &network, token, &device).await });
    let mut client = TcpStream::connect(address).await.unwrap();
    let hello = Frame {
        sequence: 0,
        message: Message::Hello {
            token: [7; 16],
            device_id: "fake".into(),
            capabilities: 2,
        },
    }
    .encode()
    .unwrap();
    for chunk in hello.chunks(3) {
        client.write_all(chunk).await.unwrap();
    }
    let ack = FrameReader::default().next(&mut client).await.unwrap();
    assert_eq!(ack.message, Message::HelloAck { enabled: false });
    for chunk in PROBE_BYTES.chunks(5) {
        client.write_all(chunk).await.unwrap();
    }
    let mut ack = [0; 4];
    client.read_exact(&mut ack).await.unwrap();
    assert_eq!(&ack, b"ACK\0");
    let mut observations = [0; 97];
    observations[..4].copy_from_slice(b"PB\x01\x1f");
    observations[4..9].copy_from_slice(b"0.1.0");
    observations[9] = 3;
    observations[10..13].copy_from_slice(b"NET");
    client.write_all(b"READ_OK\nIDLE_OK\n").await.unwrap();
    for chunk in observations.chunks(7) {
        client.write_all(chunk).await.unwrap();
    }
    let mut eof = [0; 1];
    assert_eq!(client.read(&mut eof).await.unwrap(), 0);
    let evidence = task.await.unwrap().unwrap();
    assert_eq!(evidence["network_bidirectional"], "verified");
    assert_eq!(evidence["binary_payload"], "verified");
    assert_eq!(evidence["idle_read"], "verified");
    assert_eq!(
        evidence["uci_observations"]["network_target_identity"],
        "NET"
    );
}
