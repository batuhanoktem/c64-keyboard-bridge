//! Carbon input-source APIs must execute on macOS's main queue. Background
//! callers use bounded requests; unavailable/stalled queues fail closed.
use bridge_core::BackendError;
use core_foundation::runloop::{CFRunLoop, CFRunLoopRunResult, kCFRunLoopDefaultMode};
use std::{
    collections::HashMap,
    ffi::c_void,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

unsafe extern "C" {
    fn pthread_main_np() -> std::ffi::c_int;
    // dispatch_get_main_queue() is an inline accessor for this public libdispatch
    // symbol in Apple's queue.h; no private framework API is used.
    static _dispatch_main_q: u8;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
}

const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_PENDING: usize = 64;
type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Broker {
    alive: AtomicBool,
    next_id: AtomicU64,
    executed: AtomicU64,
    pending: Mutex<HashMap<u64, Job>>,
}
static BROKER: OnceLock<Mutex<Option<Arc<Broker>>>> = OnceLock::new();
fn broker_slot() -> &'static Mutex<Option<Arc<Broker>>> {
    BROKER.get_or_init(|| Mutex::new(None))
}
fn unavailable(message: &str) -> BackendError {
    BackendError::Unavailable(message.into())
}
pub(super) fn is_main_thread() -> bool {
    // SAFETY: pthread_main_np has no arguments or ownership requirements.
    unsafe { pthread_main_np() != 0 }
}

/// Own the process's read-only main-queue broker while a main run loop is being
/// serviced. This guard must remain on the main thread and is never moved into
/// a runtime/input worker. Removing it cancels pending operations.
pub struct MacOsMainThreadContext {
    broker: Arc<Broker>,
    _main_thread_only: PhantomData<Rc<()>>,
}
impl MacOsMainThreadContext {
    pub fn install() -> Result<Self, BackendError> {
        if !is_main_thread() {
            return Err(unavailable(
                "The macOS main-thread context must be installed on the main thread",
            ));
        }
        let mut slot = broker_slot().lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err(unavailable(
                "The macOS main-thread context is already installed",
            ));
        }
        let broker = Arc::new(Broker::default());
        broker.alive.store(true, Ordering::Release);
        *slot = Some(broker.clone());
        Ok(Self {
            broker,
            _main_thread_only: PhantomData,
        })
    }
    /// Number of queued operations, excluding cancelled/timed-out requests.
    pub fn pending_queries(&self) -> usize {
        self.broker
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
    /// Operations executed by this context's main queue (never input events).
    pub fn executed_queries(&self) -> u64 {
        self.broker.executed.load(Ordering::Acquire)
    }
}
impl Drop for MacOsMainThreadContext {
    fn drop(&mut self) {
        self.broker.alive.store(false, Ordering::Release);
        // Drop jobs/reply senders now. A late GCD ticket contains only a weak
        // reference and cannot retain a runtime, callback or input-source object.
        self.broker
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let mut slot = broker_slot().lock().unwrap_or_else(|e| e.into_inner());
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.broker))
        {
            *slot = None;
        }
    }
}

struct Ticket {
    broker: Weak<Broker>,
    id: u64,
}
unsafe extern "C" fn execute_ticket(context: *mut c_void) {
    // SAFETY: dispatch_async_f invokes each submitted context exactly once.
    let ticket = unsafe { Box::from_raw(context.cast::<Ticket>()) };
    if let Some(broker) = ticket.broker.upgrade() {
        let job = broker
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket.id);
        if broker.alive.load(Ordering::Acquire)
            && is_main_thread()
            && let Some(job) = job
        {
            // Jobs handle panics before returning across libdispatch's C ABI.
            broker.executed.fetch_add(1, Ordering::Release);
            job();
        }
    }
}

pub(super) fn on_main_queue<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, BackendError> + Send + 'static,
) -> Result<T, BackendError> {
    if is_main_thread() {
        return operation();
    }
    let broker = broker_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .cloned()
        .ok_or_else(|| {
            unavailable("The macOS main-thread service is unavailable; restart the frontend")
        })?;
    if !broker.alive.load(Ordering::Acquire) {
        return Err(unavailable("The macOS main-thread service is stopping"));
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let id = broker.next_id.fetch_add(1, Ordering::Relaxed);
    let job: Job = Box::new(move || {
        let result = catch_unwind(AssertUnwindSafe(operation))
            .unwrap_or_else(|_| Err(unavailable("The macOS main-thread query failed")));
        let _ = sender.send(result);
    });
    {
        let mut pending = broker.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !broker.alive.load(Ordering::Acquire)
            || pending.len() >= MAX_PENDING
            || pending.contains_key(&id)
        {
            return Err(unavailable(
                "The macOS main-thread request queue is unavailable",
            ));
        }
        pending.insert(id, job);
    }
    let ticket = Box::new(Ticket {
        broker: Arc::downgrade(&broker),
        id,
    });
    // SAFETY: The exported main-queue address is valid for this process. The
    // callback owns this heap ticket; no stack or CoreFoundation pointer crosses
    // the queue boundary. Timeout/shutdown removes the operation from pending.
    unsafe {
        dispatch_async_f(
            (&raw const _dispatch_main_q).cast(),
            Box::into_raw(ticket).cast(),
            execute_ticket,
        );
    }
    wait_for_result(&broker, id, receiver, REQUEST_TIMEOUT)
}

fn wait_for_result<T>(
    broker: &Broker,
    id: u64,
    receiver: mpsc::Receiver<Result<T, BackendError>>,
    timeout: Duration,
) -> Result<T, BackendError> {
    let result = receiver.recv_timeout(timeout);
    if result.is_err() {
        broker
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }
    result.map_err(|_| {
        unavailable("The macOS main-thread query timed out or was cancelled; input stays disabled")
    })?
}

/// CLI entrypoint adapter: keep the original main thread servicing a lightweight
/// CoreFoundation loop while an unchanged task runs in a background runtime.
/// No application/window or keyboard event capture is created.
pub fn run_with_main_thread_loop<T: Send + 'static>(
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, BackendError> {
    let context = MacOsMainThreadContext::install()?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("c64bridge-cli-runtime".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(task));
            let _ = sender.send(result);
        })
        .map_err(|_| unavailable("Cannot start the CLI runtime thread"))?;
    let result = loop {
        match receiver.try_recv() {
            Ok(result) => break result,
            Err(mpsc::TryRecvError::Disconnected) => {
                break Err(Box::new("CLI runtime stopped") as Box<dyn std::any::Any + Send>);
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        // SAFETY: Apple's default mode is a process-lifetime CFString. Running
        // this mode on the original main thread services the main dispatch queue.
        let state = unsafe {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(20), true)
        };
        if state == CFRunLoopRunResult::Finished {
            thread::sleep(Duration::from_millis(1));
        }
    };
    let _ = worker.join();
    drop(context);
    result.map_err(|_| unavailable("The CLI runtime thread failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_query_without_a_frontend_loop_fails_closed() {
        let called = Arc::new(AtomicBool::new(false));
        let operation_called = called.clone();
        let result = thread::spawn(move || {
            on_main_queue(move || {
                operation_called.store(true, Ordering::Release);
                Ok(())
            })
        })
        .join()
        .unwrap();
        assert!(result.is_err());
        assert!(!called.load(Ordering::Acquire));
    }

    #[test]
    fn timeout_removes_queued_job_and_releases_its_captured_state() {
        struct Captured(Arc<AtomicBool>);
        impl Drop for Captured {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let broker = Broker::default();
        let dropped = Arc::new(AtomicBool::new(false));
        let captured = Captured(dropped.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        broker.pending.lock().unwrap().insert(
            7,
            Box::new(move || {
                let _captured = captured;
                let _ = sender.send(Ok::<_, BackendError>(()));
            }),
        );
        let began = std::time::Instant::now();
        assert!(wait_for_result(&broker, 7, receiver, Duration::from_millis(15)).is_err());
        assert!(began.elapsed() < Duration::from_secs(1));
        assert!(broker.pending.lock().unwrap().is_empty());
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn context_drop_cancels_pending_jobs_and_late_tickets_do_nothing() {
        let broker = Arc::new(Broker::default());
        broker.alive.store(true, Ordering::Release);
        let called = Arc::new(AtomicBool::new(false));
        let job_called = called.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        broker.pending.lock().unwrap().insert(
            8,
            Box::new(move || {
                job_called.store(true, Ordering::Release);
                let _ = sender.send(());
            }),
        );
        let ticket = Box::new(Ticket {
            broker: Arc::downgrade(&broker),
            id: 8,
        });
        let context = MacOsMainThreadContext {
            broker: broker.clone(),
            _main_thread_only: PhantomData,
        };
        drop(context);
        assert!(receiver.recv_timeout(Duration::from_millis(15)).is_err());
        assert!(broker.pending.lock().unwrap().is_empty());
        // SAFETY: This simulates exactly one execution of an owned GCD ticket.
        unsafe {
            execute_ticket(Box::into_raw(ticket).cast());
        }
        assert!(!called.load(Ordering::Acquire));
        assert_eq!(broker.executed.load(Ordering::Acquire), 0);
    }
}
