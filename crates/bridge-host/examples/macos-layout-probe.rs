//! Native, read-only regression for service/worker input-source queries.
//! It never requests permission, posts input, loads .env or contacts a device.
#[cfg(target_os = "macos")]
fn main() {
    use bridge_core::KeyboardLayout;
    use bridge_host::backend::{self, MacOsMainThreadContext};
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
    use std::{
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    fn pump() {
        // SAFETY: Apple's default mode is a process-lifetime string.
        unsafe {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(20), true);
        }
    }
    fn wait_for_pending(context: &MacOsMainThreadContext) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while context.pending_queries() == 0 {
            assert!(
                Instant::now() < deadline,
                "worker did not enqueue its native query"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
    let strict = std::env::args()
        .skip(1)
        .any(|arg| arg == "--require-layout");

    let result = backend::run_with_main_thread_loop(move || {
        let service = std::thread::Builder::new()
            .name("c64-keyboard-bridge-gui-service".into())
            .spawn(|| backend::probe_backend("macos", KeyboardLayout::Us))
            .expect("create read-only service probe");
        let probe = service.join().expect("service probe must not crash");
        assert_eq!(probe.name, "macos");
        assert!(probe.available);
        let layout_known = probe.layout_matches.is_some();
        if strict {
            assert!(
                layout_known,
                "local strict regression requires a standard US/UK source"
            );
        }
        let worker = std::thread::Builder::new()
            .name("c64-keyboard-bridge-input-worker".into())
            .spawn(move || {
                if probe.authorized && layout_known {
                    let actual = if probe.layout_matches == Some(true) {
                        KeyboardLayout::Us
                    } else {
                        KeyboardLayout::Uk
                    };
                    let mut backend = backend::create_backend("macos", actual)
                        .expect("read-only backend construction");
                    backend
                        .validate_layout()
                        .expect("read-only worker layout validation");
                    // No key/text methods are invoked; dropping an empty backend
                    // cannot issue a release either.
                    true
                } else {
                    false
                }
            })
            .expect("create read-only worker probe");
        let constructed = worker.join().expect("worker probe must not crash");
        (layout_known, constructed)
    });
    let (layout_known, constructed) = result.expect("native main-queue regression must finish");

    // Headless runners may have no selected source, but their actual native
    // query must execute on the main queue rather than silently timing out.
    let context = MacOsMainThreadContext::install().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        sender
            .send(backend::probe_backend("macos", KeyboardLayout::Us))
            .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match receiver.try_recv() {
            Ok(probe) => {
                assert!(probe.available);
                break;
            }
            Err(mpsc::TryRecvError::Empty) => {
                assert!(Instant::now() < deadline);
                pump();
            }
            Err(error) => panic!("native service probe failed: {error}"),
        }
    }
    worker.join().unwrap();
    assert_eq!(context.executed_queries(), 1);
    assert_eq!(context.pending_queries(), 0);
    drop(context);

    // Stall the original main thread until the fixed background timeout. A
    // subsequent drain of the real GCD ticket must not execute cancelled work.
    let context = MacOsMainThreadContext::install().unwrap();
    let worker = thread::spawn(|| backend::probe_backend("macos", KeyboardLayout::Us));
    wait_for_pending(&context);
    let began = Instant::now();
    let probe = worker.join().unwrap();
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(probe.layout_matches, None);
    assert_eq!(context.pending_queries(), 0);
    for _ in 0..3 {
        pump();
    }
    assert_eq!(
        context.executed_queries(),
        0,
        "timed-out work must not run later"
    );
    drop(context);

    // Drop cancels a queued request and wakes its caller without a running
    // loop. Late callbacks and subsequent background queries fail closed.
    let context = MacOsMainThreadContext::install().unwrap();
    let worker = thread::spawn(|| backend::probe_backend("macos", KeyboardLayout::Us));
    wait_for_pending(&context);
    let began = Instant::now();
    drop(context);
    assert_eq!(worker.join().unwrap().layout_matches, None);
    assert!(began.elapsed() < Duration::from_secs(1));
    for _ in 0..3 {
        pump();
    }
    let probe = thread::spawn(|| backend::probe_backend("macos", KeyboardLayout::Us))
        .join()
        .unwrap();
    assert_eq!(probe.layout_matches, None);
    println!(
        "{{\"service_native_layout_probe\":true,\"layout_known\":{layout_known},\"worker_construction_verified\":{constructed},\"bounded_timeout_cancelled\":true,\"late_callbacks_skipped\":true,\"context_drop_cancelled\":true,\"native_input\":false,\"permission_requested\":false,\"device_contacted\":false}}"
    );
}
#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS native layout regression is not applicable on this target");
}
