use super::*;
use crate::session_diagnostics::{CleanupOutcome, FailureReason, SessionPhase};
use bridge_core::{BackendCapabilities, BackendError, NativeKey, RecordingEvent};
use bridge_protocol::PhysicalKey;
use std::{collections::BTreeMap, sync::atomic::AtomicBool};

#[derive(Default)]
struct Trace {
    events: Mutex<Vec<RecordingEvent>>,
    block_press: AtomicBool,
    block_key: Mutex<Option<NativeKey>>,
    entered: AtomicBool,
    block_layout: AtomicBool,
    layout_entered: AtomicBool,
    fail_press: AtomicBool,
    fail_factory: AtomicBool,
    fail_release: AtomicBool,
    fail_repeat: AtomicBool,
    fail_layout: AtomicBool,
}
struct TestBackend(Arc<Trace>);
impl KeyboardBackend for TestBackend {
    fn validate_layout(&mut self) -> std::result::Result<(), BackendError> {
        // Capture the result at entry so tests can let a successful idle guard
        // finish, then exercise the queued transition's own validation.
        let failed = self.0.fail_layout.load(Ordering::SeqCst);
        if self.0.block_layout.load(Ordering::SeqCst) {
            self.0.layout_entered.store(true, Ordering::SeqCst);
            let until = Instant::now() + Duration::from_secs(2);
            while self.0.block_layout.load(Ordering::SeqCst) && Instant::now() < until {
                thread::sleep(Duration::from_millis(1));
            }
        }
        if failed {
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
        if self.0.fail_press.load(Ordering::SeqCst) {
            Err(BackendError::Injection("recorded press failure".into()))
        } else {
            Ok(())
        }
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
        if self.0.fail_repeat.load(Ordering::SeqCst) {
            Err(BackendError::Injection("recorded repeat failure".into()))
        } else {
            Ok(())
        }
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
        Self::with_guard(None)
    }
    fn with_guard(guard: Option<Arc<AtomicBool>>) -> Self {
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
        let (worker, thread) = spawn_worker_with_factory_guarded(
            &config,
            status.clone(),
            physical.clone(),
            move || {
                if output.fail_factory.load(Ordering::SeqCst) {
                    Err(BackendError::Injection(
                        "recorded permission failure".into(),
                    ))
                } else {
                    Ok(Box::new(TestBackend(output.clone())))
                }
            },
            guard,
        )
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
        let task =
            tokio::spawn(
                async move { session(server, token, worker, status, physical, stop_rx).await },
            );
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
        self.trace.block_layout.store(false, Ordering::SeqCst);
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
    async fn hold_control_a(&mut self) {
        self.send(Message::KeyDown {
            key: PhysicalKey::Control,
            modifiers: 2,
        })
        .await;
        self.send(Message::KeyDown {
            key: PhysicalKey::A,
            modifiers: 2,
        })
        .await;
    }
    async fn finish(mut self) {
        self.send(Message::Disconnect { reason: 0 }).await;
        self.task.await.unwrap().unwrap();
    }
}
const TOKEN: Token = Token([42; 16]);

// Failure assertions may panic. Restore the recording backend first so the
// harness can join its worker without a second panic during unwinding.
struct FailureGuard(Arc<Trace>);
impl Drop for FailureGuard {
    fn drop(&mut self) {
        self.0.fail_repeat.store(false, Ordering::SeqCst);
        self.0.fail_release.store(false, Ordering::SeqCst);
        self.0.fail_press.store(false, Ordering::SeqCst);
        self.0.fail_factory.store(false, Ordering::SeqCst);
        self.0.fail_layout.store(false, Ordering::SeqCst);
        self.0.block_layout.store(false, Ordering::SeqCst);
    }
}

async fn assert_control_a_released(h: &Harness) {
    h.wait_for(|| {
        h.events()
            .contains(&RecordingEvent::Release(NativeKey::Control))
            && h.status
                .lock()
                .unwrap()
                .diagnostics
                .last_failure
                .as_ref()
                .is_none_or(|failure| failure.cleanup != CleanupOutcome::Pending)
    })
    .await;
    assert_control_a_release_order(h);
    assert_eq!(*h.physical.lock().unwrap(), MatrixState::default());
    assert!(!h.status.lock().unwrap().connected);
}

fn assert_control_a_release_order(h: &Harness) {
    let events = h.events();
    let ordinary = events
        .iter()
        .position(|event| *event == RecordingEvent::Release(NativeKey::A))
        .expect("Session cleanup must release the held ordinary key");
    let modifier = events
        .iter()
        .position(|event| *event == RecordingEvent::Release(NativeKey::Control))
        .unwrap();
    assert!(ordinary < modifier);
}

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
    let status = h.status.lock().unwrap();
    assert!(!status.connected);
    assert_eq!(status.diagnostics.attempts, 1);
    assert_eq!(status.diagnostics.authenticated_sessions, 0);
    assert_eq!(status.diagnostics.failures.authentication, 1);
    assert_eq!(
        status.diagnostics.last_failure.as_ref().unwrap().reason,
        FailureReason::Authentication
    );
}

#[tokio::test]
async fn handshake_timeouts_have_distinct_retained_reasons() {
    for synchronized_hello in [false, true] {
        let h = Harness::new();
        let mut c = h.client(TOKEN).await;
        let (phase, duration, reason) = if synchronized_hello {
            c.hello(TOKEN).await;
            (
                SessionPhase::Synchronizing,
                SESSION_TIMEOUT,
                FailureReason::SyncTimeout,
            )
        } else {
            (
                SessionPhase::Hello,
                Duration::from_secs(5),
                FailureReason::HelloTimeout,
            )
        };
        h.wait_for(|| h.status.lock().unwrap().diagnostics.current.phase == phase)
            .await;
        tokio::time::pause();
        tokio::time::advance(duration + Duration::from_millis(1)).await;
        tokio::time::timeout(Duration::from_millis(100), c.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        tokio::time::resume();
        h.wait_for(|| {
            h.status
                .lock()
                .unwrap()
                .diagnostics
                .last_failure
                .as_ref()
                .unwrap()
                .cleanup
                == CleanupOutcome::Succeeded
        })
        .await;
        assert!(h.events().is_empty());
        let status = h.status.lock().unwrap();
        assert_eq!(status.diagnostics.current.phase, SessionPhase::Idle);
        let failure = status.diagnostics.last_failure.as_ref().unwrap();
        assert_eq!(failure.reason, reason);
        assert_eq!(failure.phase, phase);
        assert!(failure.elapsed_ms >= duration.as_millis() as u64);
        assert_eq!(
            status.diagnostics.failures.hello_timeout,
            u64::from(!synchronized_hello)
        );
        assert_eq!(
            status.diagnostics.failures.sync_timeout,
            u64::from(synchronized_hello)
        );
    }
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
async fn partial_byte_trickle_does_not_extend_valid_frame_watchdog() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.hold_control_a().await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;

    // None of these bytes completes a frame. Advancing only after A is
    // admitted separates the transport watchdog from the native worker clock.
    tokio::time::pause();
    let partial = Frame {
        sequence: c.sequence.expected(),
        message: Message::Heartbeat(*h.physical.lock().unwrap()),
    }
    .encode()
    .unwrap();
    for &byte in &partial[..5] {
        tokio::time::advance(Duration::from_millis(500)).await;
        c.socket.write_all(&[byte]).await.unwrap();
        tokio::task::yield_now().await;
        assert!(!c.task.is_finished(), "Watchdog fired before its deadline");
    }
    tokio::time::advance(Duration::from_millis(501)).await;
    let result = tokio::time::timeout(Duration::from_millis(100), c.task)
        .await
        .expect("Incomplete traffic must not restart the three-second deadline")
        .unwrap();
    assert!(result.unwrap_err().to_string().contains("timed out"));
    tokio::time::resume();
    assert_control_a_released(&h).await;
    let status = h.status.lock().unwrap();
    assert_eq!(status.diagnostics.failures.heartbeat_timeout, 1);
    let failure = status.diagnostics.last_failure.as_ref().unwrap();
    assert_eq!(failure.reason, FailureReason::HeartbeatTimeout);
    assert_eq!(failure.phase, SessionPhase::Active);
    assert_eq!(failure.cleanup, CleanupOutcome::Succeeded);
    assert_eq!(
        failure.validated_frames, 4,
        "Partial bytes are not validated frames"
    );
    assert!(failure.valid_frame_age_ms.unwrap() >= 3000);
}

#[tokio::test]
async fn sequence_gap_and_replay_release_held_keys_in_order() {
    for replay in [false, true] {
        let h = Harness::new();
        h.worker.enable(true).await.unwrap();
        let mut c = h.client(TOKEN).await;
        c.hello(TOKEN).await;
        c.sync(MatrixState::default()).await;
        c.hold_control_a().await;
        h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
            .await;
        let expected = c.sequence.expected();
        let sequence = if replay { expected - 1 } else { expected + 1 };
        let frame = Frame {
            sequence,
            message: Message::Heartbeat(*h.physical.lock().unwrap()),
        }
        .encode()
        .unwrap();
        c.socket.write_all(&frame).await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(1), c.task)
            .await
            .expect("Invalid sequencing must terminate immediately")
            .unwrap()
            .unwrap_err();
        assert!(format!("{error:#}").contains("sequence mismatch"));
        assert_control_a_released(&h).await;
        assert!(
            h.events()
                .iter()
                .all(|event| !matches!(event, RecordingEvent::Press(NativeKey::B)))
        );
        let status = h.status.lock().unwrap();
        assert_eq!(status.diagnostics.failures.protocol, 1);
        let failure = status.diagnostics.last_failure.as_ref().unwrap();
        assert_eq!(failure.reason, FailureReason::Protocol);
        assert_eq!(failure.cleanup, CleanupOutcome::Succeeded);
        assert_eq!(
            failure.validated_frames, 4,
            "A gap or replay must not become valid traffic"
        );
    }
}

#[tokio::test]
async fn real_socket_queue_overflow_automatically_ends_session_and_cleans_up() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    *h.trace.block_key.lock().unwrap() = Some(NativeKey::A);
    h.trace.block_press.store(true, Ordering::SeqCst);
    c.hold_control_a().await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;

    // A is admitted but its native press is gated. Fill the bounded queue
    // through TCP using valid alternating edges, with a consistent modifier.
    // Do not reset manually: the session failure itself must own cleanup.
    let mut bytes = Vec::new();
    for index in 0..257 {
        let message = if index % 2 == 0 {
            Message::KeyDown {
                key: PhysicalKey::B,
                modifiers: 2,
            }
        } else {
            Message::KeyUp {
                key: PhysicalKey::B,
                modifiers: 2,
            }
        };
        bytes.extend(
            Frame {
                sequence: c.sequence.next_sequence(),
                message,
            }
            .encode()
            .unwrap(),
        );
    }
    c.socket.write_all(&bytes).await.unwrap();
    h.wait_for(|| h.status.lock().unwrap().diagnostics.failures.queue_overflow == 1)
        .await;
    h.trace.block_press.store(false, Ordering::SeqCst);
    let error = tokio::time::timeout(Duration::from_secs(1), c.task)
        .await
        .expect("Overflow must terminate the network session")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("overflow"));
    assert_control_a_released(&h).await;
    assert!(!h.events().contains(&RecordingEvent::Press(NativeKey::B)));
    assert_eq!(h.status.lock().unwrap().events, 259);
    let status = h.status.lock().unwrap();
    let failure = status.diagnostics.last_failure.as_ref().unwrap();
    assert_eq!(failure.reason, FailureReason::QueueOverflow);
    assert_eq!(failure.cleanup, CleanupOutcome::Succeeded);
}

#[tokio::test]
async fn repeat_failure_disables_input_and_retains_secondary_cleanup_failure() {
    let h = Harness::new();
    let _guard = FailureGuard(h.trace.clone());
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    // Configure failures before admitting A so this cannot race its short
    // recording-backend repeat deadline on a slow CI machine.
    h.trace.fail_repeat.store(true, Ordering::SeqCst);
    h.trace.fail_release.store(true, Ordering::SeqCst);
    c.hold_control_a().await;
    h.wait_for(|| h.status.lock().unwrap().diagnostics.failures.repeat == 1)
        .await;
    let failure = {
        let status = h.status.lock().unwrap();
        assert!(!status.enabled);
        assert!(status.detail.contains("recorded repeat failure"));
        assert!(
            status
                .detail
                .contains("cleanup failed; native keys may remain held")
        );
        assert_eq!(status.diagnostics.failures.cleanup, 1);
        status.diagnostics.last_failure.as_ref().unwrap().clone()
    };
    assert_eq!(failure.reason, FailureReason::Repeat);
    assert_eq!(failure.cleanup, CleanupOutcome::Failed);
    assert!(h.events().contains(&RecordingEvent::Repeat(NativeKey::A)));

    h.trace.fail_release.store(false, Ordering::SeqCst);
    h.worker.enable(false).await.unwrap();
    assert_control_a_release_order(&h);
    assert_eq!(
        h.status
            .lock()
            .unwrap()
            .diagnostics
            .last_failure
            .as_ref()
            .unwrap(),
        &failure
    );
    c.finish().await;
    assert_eq!(
        h.status
            .lock()
            .unwrap()
            .diagnostics
            .last_failure
            .as_ref()
            .unwrap(),
        &failure
    );
}

#[tokio::test]
async fn queued_transition_layout_failure_is_classified_as_layout() {
    let h = Harness::new();
    let _guard = FailureGuard(h.trace.clone());
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    c.hold_control_a().await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;

    // Gate the next successful idle validation. Queue B and change the layout
    // while it is blocked, so B's transition check discovers the failure.
    // This avoids relying on beating the worker's 100ms idle guard on slow CI.
    h.trace.block_layout.store(true, Ordering::SeqCst);
    h.wait_for(|| h.trace.layout_entered.load(Ordering::SeqCst))
        .await;
    c.send(Message::KeyDown {
        key: PhysicalKey::B,
        modifiers: 2,
    })
    .await;
    h.wait_for(|| h.status.lock().unwrap().events == 3).await;
    h.trace.fail_layout.store(true, Ordering::SeqCst);
    h.trace.block_layout.store(false, Ordering::SeqCst);
    h.wait_for(|| h.status.lock().unwrap().diagnostics.failures.layout == 1)
        .await;
    assert_control_a_release_order(&h);
    assert!(!h.events().contains(&RecordingEvent::Press(NativeKey::B)));
    {
        let status = h.status.lock().unwrap();
        assert!(!status.enabled);
        assert_eq!(status.diagnostics.failures.input, 0);
        assert!(status.detail.contains("mapping/backend failure"));
        let failure = status.diagnostics.last_failure.as_ref().unwrap();
        assert_eq!(failure.reason, FailureReason::Layout);
        assert_eq!(failure.phase, SessionPhase::Active);
        assert_eq!(failure.cleanup, CleanupOutcome::Succeeded);
    }
    h.trace.fail_layout.store(false, Ordering::SeqCst);
    c.finish().await;
}

#[tokio::test]
async fn stale_native_failure_preserves_timeout_or_newer_session_evidence() {
    // Exercise ended-session Idle, a replacement HELLO, and a replacement
    // session that has already failed before the old native call completes.
    for replacement in 0..=2 {
        let h = Harness::new();
        let _guard = FailureGuard(h.trace.clone());
        h.worker.enable(true).await.unwrap();
        let mut c = h.client(TOKEN).await;
        c.hello(TOKEN).await;
        c.sync(MatrixState::default()).await;
        *h.trace.block_key.lock().unwrap() = Some(NativeKey::A);
        h.trace.block_press.store(true, Ordering::SeqCst);
        c.hold_control_a().await;
        h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
            .await;
        let old_session = h.status.lock().unwrap().diagnostics.current.generation;
        h.trace.fail_press.store(true, Ordering::SeqCst);
        h.trace.fail_release.store(true, Ordering::SeqCst);

        tokio::time::pause();
        tokio::time::advance(SESSION_TIMEOUT + Duration::from_millis(1)).await;
        let error = tokio::time::timeout(Duration::from_millis(100), c.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("heartbeat timed out"));
        tokio::time::resume();
        let timeout = h
            .status
            .lock()
            .unwrap()
            .diagnostics
            .last_failure
            .clone()
            .unwrap();
        assert_eq!(timeout.reason, FailureReason::HeartbeatTimeout);
        assert_eq!(timeout.cleanup, CleanupOutcome::Pending);
        assert_eq!(timeout.session_generation, old_session);
        assert_eq!(timeout.phase, SessionPhase::Active);
        assert_eq!(timeout.validated_frames, 4);

        let mut next = if replacement != 0 {
            let mut client = h.client(TOKEN).await;
            client.hello(TOKEN).await;
            assert_eq!(
                h.status.lock().unwrap().diagnostics.current.generation,
                old_session + 1
            );
            Some(client)
        } else {
            None
        };
        let retained = if replacement == 2 {
            let mut client = next.take().unwrap();
            client
                .send(Message::Error {
                    code: 31,
                    text: "replacement fault".into(),
                })
                .await;
            client.task.await.unwrap().unwrap_err();
            let failure = h
                .status
                .lock()
                .unwrap()
                .diagnostics
                .last_failure
                .clone()
                .unwrap();
            assert_eq!(failure.reason, FailureReason::Protocol);
            assert_eq!(failure.session_generation, old_session + 1);
            assert_eq!(failure.phase, SessionPhase::Synchronizing);
            assert_eq!(failure.cleanup, CleanupOutcome::Pending);
            failure
        } else {
            timeout
        };

        // The old press now reports failure and its cleanup also fails. Neither
        // may attribute the fault to the new HELLO or replace retained evidence.
        h.trace.block_press.store(false, Ordering::SeqCst);
        h.wait_for(|| {
            let status = h.status.lock().unwrap();
            status.diagnostics.failures.input == 1
                && status.diagnostics.last_failure.as_ref().unwrap().cleanup
                    == CleanupOutcome::Failed
        })
        .await;
        let mut expected = retained;
        expected.cleanup = CleanupOutcome::Failed;
        {
            let status = h.status.lock().unwrap();
            assert!(!status.enabled);
            assert_eq!(status.diagnostics.last_failure.as_ref(), Some(&expected));
            assert_eq!(status.diagnostics.failures.heartbeat_timeout, 1);
            assert_eq!(
                status.diagnostics.failures.protocol,
                u64::from(replacement == 2)
            );
            assert_eq!(
                status.diagnostics.failures.cleanup,
                2 + u64::from(replacement == 2)
            );
            assert_eq!(
                status.diagnostics.current.phase,
                if replacement == 1 {
                    SessionPhase::Synchronizing
                } else {
                    SessionPhase::Idle
                }
            );
        }
        h.trace.fail_press.store(false, Ordering::SeqCst);
        h.trace.fail_release.store(false, Ordering::SeqCst);
        h.worker.enable(false).await.unwrap();
        assert_control_a_release_order(&h);
        if let Some(mut client) = next {
            client.sync(MatrixState::default()).await;
            client.finish().await;
        }
        assert_eq!(
            h.status.lock().unwrap().diagnostics.last_failure.as_ref(),
            Some(&expected)
        );
    }
}

#[tokio::test]
async fn failed_idle_resume_replaces_retained_network_failure() {
    let h = Harness::new();
    let _guard = FailureGuard(h.trace.clone());
    // No backend has been created. A real authenticated session can time out
    // with input disabled, then a fresh idle resume can fail authorization.
    let mut c = h.client(TOKEN).await;
    c.hello(TOKEN).await;
    c.sync(MatrixState::default()).await;
    h.wait_for(|| h.status.lock().unwrap().diagnostics.current.phase == SessionPhase::Active)
        .await;
    tokio::time::pause();
    tokio::time::advance(SESSION_TIMEOUT + Duration::from_millis(1)).await;
    tokio::time::timeout(Duration::from_millis(100), c.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    tokio::time::resume();
    h.wait_for(|| {
        h.status
            .lock()
            .unwrap()
            .diagnostics
            .last_failure
            .as_ref()
            .unwrap()
            .cleanup
            == CleanupOutcome::Succeeded
    })
    .await;
    let timeout = h
        .status
        .lock()
        .unwrap()
        .diagnostics
        .last_failure
        .clone()
        .unwrap();
    h.trace.fail_factory.store(true, Ordering::SeqCst);
    assert!(
        h.worker
            .enable(true)
            .await
            .unwrap_err()
            .to_string()
            .contains("recorded permission failure")
    );
    let status = h.status.lock().unwrap();
    assert!(!status.enabled);
    assert_eq!(status.diagnostics.failures.heartbeat_timeout, 1);
    assert_eq!(status.diagnostics.failures.input, 1);
    assert_eq!(status.diagnostics.failures.cleanup, 0);
    let failure = status.diagnostics.last_failure.as_ref().unwrap();
    assert_eq!(failure.reason, FailureReason::Input);
    assert_eq!(failure.phase, SessionPhase::Idle);
    assert_eq!(failure.cleanup, CleanupOutcome::Succeeded);
    assert_eq!(failure.session_generation, timeout.session_generation);
    assert!(failure.generation > timeout.generation);
    assert_eq!(failure.validated_frames, 0);
    assert_eq!(failure.valid_frame_age_ms, None);
    assert!(h.events().is_empty());
}

#[tokio::test]
async fn diagnostics_survive_reconnect_and_pause_without_retaining_peer_secrets() {
    const PRIVATE_TOKEN: Token = Token(*b"SECRET_TOKEN_123");
    const PRIVATE_IDENTITY: &str = "SENTINEL-IDENTITY-PRIVATE";
    const PRIVATE_TEXT: &str = "SENTINEL-PEER-TEXT-PRIVATE";
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    let mut c = h.client(PRIVATE_TOKEN).await;
    c.send(Message::Hello {
        token: PRIVATE_TOKEN.0,
        capabilities: 1,
        device_id: PRIVATE_IDENTITY.into(),
    })
    .await;
    assert!(matches!(
        FrameReader::default()
            .next(&mut c.socket)
            .await
            .unwrap()
            .message,
        Message::HelloAck { .. }
    ));
    c.sync(MatrixState::default()).await;
    c.hold_control_a().await;
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    c.send(Message::Error {
        code: 17,
        text: PRIVATE_TEXT.into(),
    })
    .await;
    c.task.await.unwrap().unwrap_err();
    assert_control_a_released(&h).await;
    let retained = h
        .status
        .lock()
        .unwrap()
        .diagnostics
        .last_failure
        .clone()
        .unwrap();
    assert_eq!(retained.reason, FailureReason::Protocol);
    h.worker.enable(false).await.unwrap();
    let mut c = h.client(PRIVATE_TOKEN).await;
    c.hello(PRIVATE_TOKEN).await;
    c.sync(MatrixState::default()).await;
    h.wait_for(|| h.status.lock().unwrap().diagnostics.current.phase == SessionPhase::Active)
        .await;
    {
        let status = h.status.lock().unwrap().snapshot();
        assert_eq!(status.diagnostics.attempts, 2);
        assert_eq!(status.diagnostics.authenticated_sessions, 2);
        assert_eq!(status.diagnostics.reconnects, 1);
        assert_eq!(status.diagnostics.failures.protocol, 1);
        assert_eq!(status.diagnostics.last_failure.as_ref(), Some(&retained));
        let diagnostics = serde_json::to_string(&status.diagnostics).unwrap();
        for sentinel in [
            PRIVATE_IDENTITY,
            PRIVATE_TEXT,
            "SECRET_TOKEN_123",
            &PRIVATE_TOKEN.hex(),
            "127.0.0.1",
        ] {
            assert!(!diagnostics.contains(sentinel));
        }
        let response = ControlResponse {
            ok: true,
            status,
            error: None,
        };
        assert!(serde_json::to_vec(&response).unwrap().len() < 4096);
    }
    c.finish().await;
    let status = h.status.lock().unwrap().snapshot();
    let json = serde_json::to_string(&ControlResponse {
        ok: true,
        status,
        error: None,
    })
    .unwrap();
    for sentinel in [
        PRIVATE_IDENTITY,
        PRIVATE_TEXT,
        "SECRET_TOKEN_123",
        &PRIVATE_TOKEN.hex(),
    ] {
        assert!(!json.contains(sentinel));
    }
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

fn managed_config(state: &std::path::Path) -> Config {
    #[cfg(unix)]
    if state.exists() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    Config::from_values(
        BTreeMap::from([
            ("C64_HOST".into(), "127.0.0.1".into()),
            ("BRIDGE_BIND_ADDRESS".into(), "127.0.0.1".into()),
            ("HOST_ADVERTISE_IP".into(), "127.0.0.1".into()),
            ("BRIDGE_PORT".into(), port.to_string()),
            ("BRIDGE_STATE_DIR".into(), state.display().to_string()),
            ("INPUT_BACKEND".into(), "recording".into()),
            ("AUTO_CONNECT".into(), "false".into()),
        ]),
        BTreeMap::new(),
        crate::config::Overrides::default(),
    )
    .unwrap()
}
fn managed_options() -> StartOptions {
    StartOptions {
        enable_input: false,
        deploy: false,
        allow_reset: false,
        template: Vec::new(),
        fake_device: true,
    }
}

#[tokio::test]
async fn managed_cancel_before_startup_creates_no_runtime_or_pairing_state() {
    let parent = tempfile::tempdir().unwrap();
    let state = parent.path().join("never-created");
    let mut config = managed_config(&state);
    // Without cancellation, startup would fail while trying to resolve this
    // missing host. No setup work should happen after an already issued stop.
    config.device_host = None;
    let mut runtime = ManagedRuntime::spawn(config, managed_options());
    runtime.request_stop();
    assert!(runtime.ready().await.is_err());
    runtime.wait().await.unwrap();
    assert_eq!(runtime.completion(), Some(Ok(())));
    assert!(!state.exists());
}

#[tokio::test]
async fn managed_ready_remains_compatible_with_authenticated_cli_control() {
    let parent = tempfile::tempdir().unwrap();
    let config = managed_config(parent.path());
    let mut runtime = ManagedRuntime::spawn(config.clone(), managed_options());
    runtime.ready().await.unwrap();
    assert_eq!(*runtime.subscribe().borrow(), RuntimeState::Ready);
    let status = control::send(parent.path(), "status").await.unwrap();
    assert!(status.ok && status.status.running && !status.status.enabled);
    let resume = control::send(parent.path(), "resume").await.unwrap();
    assert!(resume.ok && resume.status.enabled);
    let pause = control::send(parent.path(), "pause").await.unwrap();
    assert!(pause.ok && !pause.status.enabled);
    let stop = control::send(parent.path(), "stop").await.unwrap();
    assert!(stop.ok && !stop.status.enabled);
    runtime.wait().await.unwrap();
    assert!(runtime.is_finished());
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn dropping_managed_owner_requests_graceful_completion() {
    let parent = tempfile::tempdir().unwrap();
    let mut runtime = ManagedRuntime::spawn(managed_config(parent.path()), managed_options());
    runtime.ready().await.unwrap();
    let mut state = runtime.subscribe();
    drop(runtime);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(*state.borrow(), RuntimeState::Finished(Ok(()))) {
                break;
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn managed_cancellation_during_rest_startup_does_not_report_ready() {
    let parent = tempfile::tempdir().unwrap();
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = server.local_addr().unwrap().port();
    let (entered_tx, entered) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        let (mut socket, _) = server.accept().await.unwrap();
        let mut request = [0; 1024];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        entered_tx.send(()).unwrap();
        assert_eq!(socket.read(&mut request).await.unwrap(), 0);
    });
    let mut config = managed_config(parent.path());
    config.device_port = port;
    config.auto_connect = true;
    let mut options = managed_options();
    options.fake_device = false;
    let mut runtime = ManagedRuntime::spawn(config, options);
    let mut events = runtime.subscribe();
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*events.borrow(), RuntimeState::Starting);
    runtime.request_stop();
    tokio::time::timeout(Duration::from_secs(2), runtime.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*events.borrow_and_update(), RuntimeState::Finished(Ok(())));
    server_task.await.unwrap();
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn managed_cancellation_while_deployment_waits_never_retries_upload() {
    let parent = tempfile::tempdir().unwrap();
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = server.local_addr().unwrap().port();
    let (uploaded_tx, uploaded) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        let (mut info, _) = server.accept().await.unwrap();
        let mut request = Vec::new();
        let mut bytes = [0; 1024];
        while !request.windows(4).any(|value| value == b"\r\n\r\n") {
            let count = info.read(&mut bytes).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&bytes[..count]);
        }
        let body = r#"{"product":"C64 Ultimate","firmware_version":"simulated","fpga_version":"simulated","hostname":"fixture","unique_id":"fake-c64","errors":[]}"#;
        info.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        info.shutdown().await.unwrap();
        drop(info);
        let (mut upload, _) = server.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let count = upload.read(&mut bytes).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&bytes[..count]);
            if let Some(end) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        assert!(request.starts_with(b"POST /v1/runners:run_prg "));
        uploaded_tx.send(()).unwrap();
        assert_eq!(upload.read(&mut bytes).await.unwrap(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), server.accept())
                .await
                .is_err()
        );
    });
    let mut config = managed_config(parent.path());
    config.device_port = port;
    let mut options = managed_options();
    options.fake_device = false;
    options.deploy = true;
    options.allow_reset = true;
    options.template = vec![1, 8];
    options.template.extend(b"KBCFGv1\0");
    options.template.extend([1, 64]);
    options.template.resize(66, 0);
    let mut runtime = ManagedRuntime::spawn(config, options);
    tokio::time::timeout(Duration::from_secs(2), uploaded)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*runtime.subscribe().borrow(), RuntimeState::Starting);
    runtime.request_stop();
    tokio::time::timeout(Duration::from_secs(2), runtime.wait())
        .await
        .unwrap()
        .unwrap();
    server_task.await.unwrap();
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn managed_stop_waits_for_recorded_held_key_cleanup_and_reports_failure() {
    for fail_cleanup in [false, true] {
        let parent = tempfile::tempdir().unwrap();
        let config = managed_config(parent.path());
        let address = (std::net::Ipv4Addr::LOCALHOST, config.bridge_port);
        let trace = Arc::new(Trace::default());
        let output = trace.clone();
        let mut options = managed_options();
        options.enable_input = true;
        let mut runtime =
            ManagedRuntime::spawn_with_worker(config, options, move |config, status, physical| {
                spawn_worker_with_factory(config, status, physical, move || {
                    Ok(Box::new(TestBackend(output.clone())))
                })
            });
        runtime.ready().await.unwrap();
        let token = crate::pairing::read_token(parent.path()).unwrap();
        let mut socket = TcpStream::connect(address).await.unwrap();
        let mut sequence = Sequencer::new();
        write_frame(
            &mut socket,
            &mut sequence,
            Message::Hello {
                token: token.0,
                device_id: "recorded".into(),
                capabilities: 1,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            FrameReader::default()
                .next(&mut socket)
                .await
                .unwrap()
                .message,
            Message::HelloAck { enabled: true }
        ));
        write_frame(
            &mut socket,
            &mut sequence,
            Message::StateSync(MatrixState::default()),
        )
        .await
        .unwrap();
        let mut matrix = MatrixState::default();
        for key in [PhysicalKey::LeftShift, PhysicalKey::A] {
            matrix.set(key, true);
            write_frame(
                &mut socket,
                &mut sequence,
                Message::KeyDown {
                    key,
                    modifiers: matrix.modifiers(),
                },
            )
            .await
            .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while !trace
                .events
                .lock()
                .unwrap()
                .contains(&RecordingEvent::Press(NativeKey::A))
            {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        trace.fail_release.store(fail_cleanup, Ordering::SeqCst);
        runtime.request_stop();
        let result = tokio::time::timeout(Duration::from_secs(2), runtime.wait())
            .await
            .unwrap();
        assert_eq!(result.is_err(), fail_cleanup);
        assert_eq!(runtime.completion().unwrap().is_err(), fail_cleanup);
        if fail_cleanup {
            assert!(format!("{:#}", result.unwrap_err()).contains("native keys may remain held"));
        } else {
            let events = trace.events.lock().unwrap();
            assert!(events.ends_with(&[
                RecordingEvent::Release(NativeKey::A),
                RecordingEvent::Release(NativeKey::LeftShift)
            ]));
        }
        assert!(!parent.path().join("runtime.json").exists());
    }
}

#[tokio::test]
async fn managed_stop_preserves_failed_cleanup_for_retry_before_destroying_worker() {
    let parent = tempfile::tempdir().unwrap();
    let trace = Arc::new(Trace::default());
    let output = trace.clone();
    let (worker_tx, worker_rx) = mpsc::channel();
    let mut options = managed_options();
    options.enable_input = true;
    let mut runtime = ManagedRuntime::spawn_with_worker(
        managed_config(parent.path()),
        options,
        move |config, status, physical| {
            let pair = spawn_worker_with_factory(config, status, physical, move || {
                Ok(Box::new(TestBackend(output.clone())))
            })?;
            worker_tx.send(pair.0.clone()).unwrap();
            Ok(pair)
        },
    );
    runtime.ready().await.unwrap();
    let worker = worker_rx.recv().unwrap();
    for key in [PhysicalKey::LeftShift, PhysicalKey::A] {
        worker
            .event(KeyEvent {
                key,
                pressed: true,
                modifiers: bridge_protocol::MOD_SHIFT,
            })
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while !trace
            .events
            .lock()
            .unwrap()
            .contains(&RecordingEvent::Press(NativeKey::A))
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    trace.fail_release.store(true, Ordering::SeqCst);
    assert!(runtime.stop().await.is_err());
    assert!(!runtime.is_finished());
    assert!(parent.path().join("runtime.json").exists());
    let paused = control::send(parent.path(), "status").await.unwrap();
    assert!(!paused.status.enabled);
    assert_eq!(
        paused.status.diagnostics.last_failure.unwrap().cleanup,
        CleanupOutcome::Failed
    );
    trace.fail_release.store(false, Ordering::SeqCst);
    runtime.stop().await.unwrap();
    assert_eq!(runtime.completion(), Some(Ok(())));
    assert!(trace.events.lock().unwrap().ends_with(&[
        RecordingEvent::Release(NativeKey::A),
        RecordingEvent::Release(NativeKey::LeftShift)
    ]));
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn gui_guard_releases_held_keys_and_reopening_requires_explicit_fresh_resume() {
    let guard = Arc::new(AtomicBool::new(true));
    let h = Harness::with_guard(Some(guard.clone()));
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::LeftShift, true);
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    guard.store(false, Ordering::SeqCst);
    h.wait_for(|| !h.status.lock().unwrap().enabled).await;
    assert!(h.events().ends_with(&[
        RecordingEvent::Release(NativeKey::A),
        RecordingEvent::Release(NativeKey::LeftShift)
    ]));
    let paused_events = h.events();
    assert_eq!(h.status.lock().unwrap().diagnostics.failures.input, 0);
    guard.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(!h.status.lock().unwrap().enabled);
    assert_eq!(h.events(), paused_events);
    h.worker.enable(true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        h.events(),
        paused_events,
        "held ordinary keys must remain quarantined after explicit resume"
    );
    h.event(PhysicalKey::LeftShift, false);
    h.event(PhysicalKey::A, false);
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().len() > paused_events.len()).await;
    assert_eq!(
        h.events().last(),
        Some(&RecordingEvent::Press(NativeKey::A))
    );
    assert!(
        !h.events()[paused_events.len()..].contains(&RecordingEvent::Press(NativeKey::LeftShift))
    );
}

#[tokio::test]
async fn gui_guard_blocked_enable_never_constructs_native_backend() {
    let parent = tempfile::tempdir().unwrap();
    let config = managed_config(parent.path());
    let status = Arc::new(Mutex::new(Status::default()));
    let physical = Arc::new(Mutex::new(MatrixState::default()));
    let constructions = Arc::new(AtomicU64::new(0));
    let count = constructions.clone();
    let (worker, thread) = spawn_worker_with_factory_guarded(
        &config,
        status.clone(),
        physical,
        move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(bridge_core::RecordingBackend::default()))
        },
        Some(Arc::new(AtomicBool::new(false))),
    )
    .unwrap();
    assert!(worker.enable(true).await.is_err());
    assert_eq!(constructions.load(Ordering::SeqCst), 0);
    assert!(!status.lock().unwrap().enabled);
    assert_eq!(status.lock().unwrap().diagnostics.failures.input, 0);
    worker.controls.send(WorkerControl::Shutdown).unwrap();
    finish_worker(thread, Ok(())).unwrap();
}

#[tokio::test]
async fn gui_guard_closing_during_backend_creation_prevents_late_enable_ack() {
    let parent = tempfile::tempdir().unwrap();
    let config = managed_config(parent.path());
    let status = Arc::new(Mutex::new(Status::default()));
    let physical = Arc::new(Mutex::new(MatrixState::default()));
    let guard = Arc::new(AtomicBool::new(true));
    let entered = Arc::new(AtomicBool::new(false));
    let factory_guard = guard.clone();
    let factory_entered = entered.clone();
    let (worker, thread) = spawn_worker_with_factory_guarded(
        &config,
        status.clone(),
        physical,
        move || {
            factory_entered.store(true, Ordering::SeqCst);
            let until = Instant::now() + Duration::from_secs(2);
            while factory_guard.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(Box::new(bridge_core::RecordingBackend::default()))
        },
        Some(guard.clone()),
    )
    .unwrap();
    let request = worker.clone();
    let enable = tokio::spawn(async move { request.enable(true).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    guard.store(false, Ordering::SeqCst);
    assert!(enable.await.unwrap().is_err());
    assert!(!status.lock().unwrap().enabled);
    assert_eq!(status.lock().unwrap().diagnostics.failures.input, 0);
    worker.controls.send(WorkerControl::Shutdown).unwrap();
    finish_worker(thread, Ok(())).unwrap();
}

#[tokio::test]
async fn gui_guard_cleanup_failure_is_retained_without_polling_retry_or_counter_spam() {
    let guard = Arc::new(AtomicBool::new(true));
    let h = Harness::with_guard(Some(guard.clone()));
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().contains(&RecordingEvent::Press(NativeKey::A)))
        .await;
    h.trace.fail_release.store(true, Ordering::SeqCst);
    guard.store(false, Ordering::SeqCst);
    h.wait_for(|| !h.status.lock().unwrap().enabled).await;
    assert_eq!(h.status.lock().unwrap().diagnostics.failures.cleanup, 1);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(h.status.lock().unwrap().diagnostics.failures.cleanup, 1);
    assert!(!h.events().contains(&RecordingEvent::Release(NativeKey::A)));
    h.trace.fail_release.store(false, Ordering::SeqCst);
    h.worker.enable(false).await.unwrap();
    assert_eq!(
        h.events().last(),
        Some(&RecordingEvent::Release(NativeKey::A))
    );
}

#[test]
fn gui_guard_checks_each_native_output_but_always_allows_releases() {
    let trace = Arc::new(Trace::default());
    let guard = Arc::new(AtomicBool::new(true));
    let mut backend = GuardedBackend::new(Box::new(TestBackend(trace.clone())), guard.clone());
    backend.press(NativeKey::LeftShift).unwrap();
    guard.store(false, Ordering::SeqCst);
    assert!(backend.press(NativeKey::A).is_err());
    assert!(backend.repeat(NativeKey::A).is_err());
    assert!(backend.type_text("sentinel text must never post").is_err());
    backend.release(NativeKey::LeftShift).unwrap();
    assert_eq!(
        *trace.events.lock().unwrap(),
        [
            RecordingEvent::Press(NativeKey::LeftShift),
            RecordingEvent::Release(NativeKey::LeftShift)
        ]
    );
}

#[tokio::test]
async fn managed_gui_guard_remains_compatible_with_external_control_without_late_resume() {
    let parent = tempfile::tempdir().unwrap();
    let guard = Arc::new(AtomicBool::new(false));
    let mut runtime = ManagedRuntime::spawn_guarded(
        managed_config(parent.path()),
        managed_options(),
        guard.clone(),
    );
    runtime.ready().await.unwrap();
    let blocked = control::send(parent.path(), "resume").await.unwrap();
    assert!(!blocked.ok && !blocked.status.enabled);
    assert_eq!(blocked.status.backend, "disabled");
    guard.store(true, Ordering::SeqCst);
    assert!(
        !control::send(parent.path(), "status")
            .await
            .unwrap()
            .status
            .enabled
    );
    let enabled = control::send(parent.path(), "resume").await.unwrap();
    assert!(enabled.ok && enabled.status.enabled);
    guard.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(2), async {
        while control::send(parent.path(), "status")
            .await
            .unwrap()
            .status
            .enabled
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    runtime.stop().await.unwrap();
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn independent_focus_guard_prevents_stale_arming_during_factory_and_output() {
    let parent = tempfile::tempdir().unwrap();
    let config = managed_config(parent.path());
    let status = Arc::new(Mutex::new(Status::default()));
    let physical = Arc::new(Mutex::new(MatrixState::default()));
    let allow = Arc::new(AtomicBool::new(true));
    let focus = Arc::new(AtomicBool::new(true));
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let factory_entered = entered.clone();
    let factory_release = release.clone();
    let (worker, thread) = spawn_worker_with_factory_gate(
        &config,
        status.clone(),
        physical,
        move || {
            factory_entered.store(true, Ordering::SeqCst);
            let until = Instant::now() + Duration::from_secs(2);
            while !factory_release.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(Box::new(bridge_core::RecordingBackend::default()))
        },
        Some(InputGuard {
            allow: allow.clone(),
            focus_allowed: Some(focus.clone()),
        }),
    )
    .unwrap();
    let request = worker.clone();
    let enable = tokio::spawn(async move { request.enable(true).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    focus.store(false, Ordering::SeqCst);
    allow.store(false, Ordering::SeqCst);
    // A stale countdown cannot undo the independently recorded focus change.
    allow.store(true, Ordering::SeqCst);
    release.store(true, Ordering::SeqCst);
    assert!(enable.await.unwrap().is_err());
    assert!(!status.lock().unwrap().enabled);
    worker.controls.send(WorkerControl::Shutdown).unwrap();
    finish_worker(thread, Ok(())).unwrap();
    let trace = Arc::new(Trace::default());
    let mut backend = GuardedBackend::new_with_focus(
        Box::new(TestBackend(trace.clone())),
        allow.clone(),
        focus.clone(),
    );
    assert!(backend.press(NativeKey::A).is_err());
    assert!(backend.repeat(NativeKey::A).is_err());
    assert!(backend.type_text("do not post").is_err());
    assert!(trace.events.lock().unwrap().is_empty());
}

struct PreparedRequest {
    grant: oneshot::Sender<std::result::Result<(), BackendError>>,
    cancelled: watch::Receiver<bool>,
}
struct PreparedBackend {
    trace: Arc<Trace>,
    drops: Arc<AtomicU64>,
}
impl Drop for PreparedBackend {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl KeyboardBackend for PreparedBackend {
    fn capabilities(&self) -> BackendCapabilities {
        TestBackend(self.trace.clone()).capabilities()
    }
    fn validate_layout(&mut self) -> std::result::Result<(), BackendError> {
        TestBackend(self.trace.clone()).validate_layout()
    }
    fn press(&mut self, key: NativeKey) -> std::result::Result<(), BackendError> {
        TestBackend(self.trace.clone()).press(key)
    }
    fn release(&mut self, key: NativeKey) -> std::result::Result<(), BackendError> {
        TestBackend(self.trace.clone()).release(key)
    }
    fn repeat(&mut self, key: NativeKey) -> std::result::Result<(), BackendError> {
        TestBackend(self.trace.clone()).repeat(key)
    }
}
struct AuthorizationHarness {
    inner: Harness,
    requests: tokio::sync::mpsc::UnboundedReceiver<PreparedRequest>,
    drops: Arc<AtomicU64>,
    completed: Arc<AtomicU64>,
}
impl AuthorizationHarness {
    fn new(cooperative_cancel: bool) -> Self {
        Self::with_guard(cooperative_cancel, None)
    }
    fn with_guard(cooperative_cancel: bool, guard: Option<Arc<AtomicBool>>) -> Self {
        let config = Config::from_values(
            BTreeMap::from([("INPUT_BACKEND".into(), "recording".into())]),
            BTreeMap::new(),
            crate::config::Overrides::default(),
        )
        .unwrap();
        let status = Arc::new(Mutex::new(Status::default()));
        let physical = Arc::new(Mutex::new(MatrixState::default()));
        let trace = Arc::new(Trace::default());
        let drops = Arc::new(AtomicU64::new(0));
        let completed = Arc::new(AtomicU64::new(0));
        let (requests_tx, requests) = tokio::sync::mpsc::unbounded_channel();
        let output = trace.clone();
        let backend_drops = drops.clone();
        let tasks_completed = completed.clone();
        let prepare: Preparer = Arc::new(move |mut cancellation| {
            let (grant, mut receiver) = oneshot::channel();
            requests_tx
                .send(PreparedRequest {
                    grant,
                    cancelled: cancellation.clone(),
                })
                .unwrap();
            let output = output.clone();
            let drops = backend_drops.clone();
            let completed = tasks_completed.clone();
            Box::pin(async move {
                let result = if cooperative_cancel {
                    tokio::select! {biased;_=super::cancellation(&mut cancellation)=>Err(BackendError::Unavailable("cancelled private fixture".into())),result=&mut receiver=>result.unwrap_or_else(|_|Err(BackendError::Unavailable("fixture closed".into())))}
                } else {
                    receiver
                        .await
                        .unwrap_or_else(|_| Err(BackendError::Unavailable("fixture closed".into())))
                };
                completed.fetch_add(1, Ordering::SeqCst);
                result?;
                Ok(Box::new(PreparedBackend {
                    trace: output,
                    drops,
                }) as Box<dyn KeyboardBackend>)
            })
        });
        let (worker, thread) = spawn_worker_with_backend_factory(
            &config,
            status.clone(),
            physical.clone(),
            BackendFactory::Prepared {
                runtime: tokio::runtime::Handle::current(),
                prepare,
            },
            guard.map(|allow| InputGuard {
                allow,
                focus_allowed: None,
            }),
        )
        .unwrap();
        Self {
            inner: Harness {
                worker,
                thread: Some(thread),
                status,
                physical,
                trace,
            },
            requests,
            drops,
            completed,
        }
    }
    async fn next(&mut self) -> PreparedRequest {
        tokio::time::timeout(Duration::from_secs(1), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn phase(&self, phase: AuthorizationPhase) {
        self.inner
            .wait_for(|| self.inner.status.lock().unwrap().authorization.phase == phase)
            .await;
    }
    async fn shutdown(mut self) {
        self.inner
            .worker
            .controls
            .send(WorkerControl::Shutdown)
            .unwrap();
        let thread = self.inner.thread.take().unwrap();
        tokio::task::spawn_blocking(move || thread.join().unwrap().unwrap())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn consent_pending_responds_quickly_drains_disabled_events_and_pause_cancels() {
    let mut h = AuthorizationHarness::new(true);
    tokio::time::timeout(Duration::from_millis(250), h.inner.worker.enable(true))
        .await
        .unwrap()
        .unwrap();
    h.phase(AuthorizationPhase::Pending).await;
    let mut request = h.next().await;
    assert!(!h.inner.status.lock().unwrap().enabled);
    for _ in 0..150 {
        h.inner.event(PhysicalKey::A, true);
        h.inner.event(PhysicalKey::A, false);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(h.inner.events().is_empty());
    tokio::time::timeout(Duration::from_millis(250), h.inner.worker.enable(false))
        .await
        .unwrap()
        .unwrap();
    request.cancelled.changed().await.unwrap();
    assert!(*request.cancelled.borrow());
    h.phase(AuthorizationPhase::Cancelled).await;
    h.inner
        .wait_for(|| h.completed.load(Ordering::SeqCst) == 1)
        .await;
    assert!(h.inner.events().is_empty());
    h.shutdown().await;
}
#[tokio::test]
async fn late_consent_after_pause_is_closed_without_overwriting_newer_authorization() {
    let mut h = AuthorizationHarness::new(false);
    h.inner.worker.enable(true).await.unwrap();
    let stale = h.next().await;
    h.inner.worker.enable(false).await.unwrap();
    h.inner.worker.authorize().await.unwrap();
    let current = h.next().await;
    current.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    stale.grant.send(Ok(())).unwrap();
    h.inner
        .wait_for(|| h.drops.load(Ordering::SeqCst) == 1)
        .await;
    assert_eq!(
        h.inner.status.lock().unwrap().authorization.phase,
        AuthorizationPhase::Ready
    );
    assert!(!h.inner.status.lock().unwrap().enabled);
    assert!(h.inner.events().is_empty());
    let drops = h.drops.clone();
    h.shutdown().await;
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn successful_consent_quarantines_held_ordinary_keys_and_retains_session_on_pause() {
    let mut h = AuthorizationHarness::new(true);
    h.inner.worker.enable(true).await.unwrap();
    let request = h.next().await;
    h.inner.event(PhysicalKey::LeftShift, true);
    h.inner.event(PhysicalKey::A, true);
    request.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    assert!(h.inner.status.lock().unwrap().enabled);
    assert!(h.inner.events().is_empty());
    h.inner.event(PhysicalKey::LeftShift, false);
    h.inner.event(PhysicalKey::A, false);
    h.inner.event(PhysicalKey::A, true);
    h.inner.event(PhysicalKey::A, false);
    h.inner
        .wait_for(|| {
            h.inner
                .events()
                .iter()
                .any(|event| matches!(event, RecordingEvent::Release(NativeKey::A)))
        })
        .await;
    assert_eq!(
        h.inner.events(),
        vec![
            RecordingEvent::Press(NativeKey::A),
            RecordingEvent::Release(NativeKey::A)
        ]
    );
    h.inner.worker.enable(false).await.unwrap();
    h.inner.worker.enable(true).await.unwrap();
    assert!(h.requests.try_recv().is_err());
    assert_eq!(h.completed.load(Ordering::SeqCst), 1);
    h.shutdown().await;
}
#[tokio::test]
async fn authorize_only_never_posts_even_held_modifiers_and_shutdown_joins_pending_cleanup() {
    let mut h = AuthorizationHarness::new(true);
    h.inner.event(PhysicalKey::LeftShift, true);
    h.inner.worker.authorize().await.unwrap();
    let request = h.next().await;
    request.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    assert!(!h.inner.status.lock().unwrap().enabled);
    assert!(h.inner.events().is_empty());
    h.shutdown().await;
    let mut h = AuthorizationHarness::new(true);
    h.inner.worker.authorize().await.unwrap();
    let mut request = h.next().await;
    let completed = h.completed.clone();
    h.shutdown().await;
    assert!(*request.cancelled.borrow_and_update());
    assert_eq!(completed.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn denied_authorization_is_typed_and_does_not_copy_backend_supplied_secrets() {
    let mut h = AuthorizationHarness::new(true);
    h.inner.worker.enable(true).await.unwrap();
    let request = h.next().await;
    request
        .grant
        .send(Err(BackendError::PermissionDenied(
            "PORTAL_SECRET_TOKEN_KEYMAP_SENTINEL".into(),
        )))
        .unwrap();
    h.phase(AuthorizationPhase::Denied).await;
    let status = h.inner.status.lock().unwrap().snapshot();
    assert!(!status.enabled);
    assert!(!serde_json::to_string(&status).unwrap().contains("SENTINEL"));
    h.shutdown().await;
}

// Deliberately hold the worker's ordering barrier while its independent consent
// future runs, to force the queued-control/result race without wall-clock sleeps.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn queued_pause_precedes_prepared_generation_and_fresh_resume_keeps_processing_events() {
    let mut h = AuthorizationHarness::new(false);
    h.inner.worker.enable(true).await.unwrap();
    let request = h.next().await;
    let order = h.inner.worker.control_order.lock().unwrap();
    request.grant.send(Ok(())).unwrap();
    // The preparation task completes without awaits between this marker and its
    // ordered result send. Holding control_order blocks result installation.
    while h.completed.load(Ordering::SeqCst) != 1 {
        tokio::task::yield_now().await;
    }
    let generation = h.inner.worker.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let (reply, response) = oneshot::channel();
    h.inner
        .worker
        .controls
        .send(WorkerControl::Enable {
            enabled: false,
            authorize_only: false,
            generation,
            reply,
            context: CurrentSession::default(),
        })
        .unwrap();
    drop(order);
    response.await.unwrap().unwrap();
    h.inner
        .wait_for(|| h.drops.load(Ordering::SeqCst) == 1)
        .await;
    assert!(!h.inner.status.lock().unwrap().enabled);
    h.inner.worker.enable(true).await.unwrap();
    let fresh = h.next().await;
    fresh.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    h.inner.event(PhysicalKey::A, true);
    h.inner.event(PhysicalKey::A, false);
    h.inner.wait_for(|| h.inner.events().len() == 2).await;
    h.shutdown().await;
}

#[tokio::test]
async fn paused_authorization_revocation_disables_and_closes_session_then_can_prepare_again() {
    let mut h = AuthorizationHarness::new(true);
    h.inner.worker.authorize().await.unwrap();
    let request = h.next().await;
    request.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    h.inner.trace.fail_layout.store(true, Ordering::SeqCst);
    h.phase(AuthorizationPhase::Failed).await;
    h.inner
        .wait_for(|| h.drops.load(Ordering::SeqCst) == 1)
        .await;
    assert!(!h.inner.status.lock().unwrap().enabled);
    assert!(h.inner.events().is_empty());
    h.inner.trace.fail_layout.store(false, Ordering::SeqCst);
    h.inner.worker.enable(true).await.unwrap();
    let request = h.next().await;
    request.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    assert!(h.inner.status.lock().unwrap().enabled);
    h.shutdown().await;
}
#[tokio::test]
async fn foreground_enable_pending_consent_keeps_control_ready_and_runtime_cancel_joins_cleanup() {
    let parent = tempfile::tempdir().unwrap();
    let config = managed_config(parent.path());
    let mut options = managed_options();
    options.enable_input = true;
    let completed = Arc::new(AtomicBool::new(false));
    let finished = completed.clone();
    let prepare: Preparer = Arc::new(move |mut cancel| {
        let completed = finished.clone();
        Box::pin(async move {
            super::cancellation(&mut cancel).await;
            completed.store(true, Ordering::SeqCst);
            Err(BackendError::Unavailable(
                "cancelled private fixture".into(),
            ))
        })
    });
    let mut runtime =
        ManagedRuntime::spawn_with_worker(config, options, move |config, status, physical| {
            spawn_worker_with_backend_factory(
                config,
                status,
                physical,
                BackendFactory::Prepared {
                    runtime: tokio::runtime::Handle::current(),
                    prepare,
                },
                None,
            )
        });
    runtime.ready().await.unwrap();
    let response = control::send(parent.path(), "status").await.unwrap();
    assert!(response.ok);
    assert_eq!(
        response.status.authorization.phase,
        AuthorizationPhase::Pending
    );
    assert!(!response.status.enabled);
    runtime.request_stop();
    tokio::time::timeout(Duration::from_secs(2), runtime.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(completed.load(Ordering::SeqCst));
    assert!(!parent.path().join("runtime.json").exists());
}

#[tokio::test]
async fn gui_safety_guard_closing_before_consent_result_closes_stale_grant() {
    let guard = Arc::new(AtomicBool::new(true));
    let mut h = AuthorizationHarness::with_guard(false, Some(guard.clone()));
    h.inner.worker.enable(true).await.unwrap();
    let request = h.next().await;
    guard.store(false, Ordering::SeqCst);
    request.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Cancelled).await;
    h.inner
        .wait_for(|| h.drops.load(Ordering::SeqCst) == 1)
        .await;
    assert!(!h.inner.status.lock().unwrap().enabled);
    assert!(h.inner.events().is_empty());
    h.shutdown().await;
}

#[tokio::test]
async fn invalid_prepared_backend_is_closed_after_cleanup_and_retry_requires_fresh_consent() {
    let mut h = AuthorizationHarness::new(true);
    h.inner.trace.fail_layout.store(true, Ordering::SeqCst);
    h.inner.worker.enable(true).await.unwrap();
    let request = h.next().await;
    request.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Failed).await;
    h.inner
        .wait_for(|| h.drops.load(Ordering::SeqCst) == 1)
        .await;
    assert!(!h.inner.status.lock().unwrap().enabled);
    assert!(h.inner.events().is_empty());
    h.inner.trace.fail_layout.store(false, Ordering::SeqCst);
    h.inner.worker.enable(true).await.unwrap();
    assert_eq!(
        h.inner.status.lock().unwrap().authorization.phase,
        AuthorizationPhase::Pending
    );
    assert!(!h.inner.status.lock().unwrap().enabled);
    let fresh = h.next().await;
    fresh.grant.send(Ok(())).unwrap();
    h.phase(AuthorizationPhase::Ready).await;
    assert!(h.inner.status.lock().unwrap().enabled);
    h.shutdown().await;
}

#[tokio::test]
async fn wrong_backend_authorization_request_cannot_discard_a_held_keys_release() {
    let h = Harness::new();
    h.worker.enable(true).await.unwrap();
    h.event(PhysicalKey::A, true);
    h.wait_for(|| h.events().len() == 1).await;
    assert!(
        h.worker
            .authorize()
            .await
            .unwrap_err()
            .to_string()
            .contains("does not use desktop portal")
    );
    assert!(!h.status.lock().unwrap().enabled);
    assert_eq!(
        h.events(),
        vec![
            RecordingEvent::Press(NativeKey::A),
            RecordingEvent::Release(NativeKey::A)
        ]
    );
    h.event(PhysicalKey::A, false);
}
