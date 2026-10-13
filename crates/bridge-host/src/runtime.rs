use crate::{
    backend,
    config::{Config, NetworkConfig},
    control::{
        self, AuthorizationPhase, AuthorizationStatus, ControlRequest, ControlResponse,
        RuntimeRecord, Status,
    },
    pairing::{RuntimeGuard, Token},
    rest::DeviceInfo,
    session_diagnostics::{CleanupOutcome, CurrentSession, FailureReason, SessionPhase},
};
use anyhow::{Context, Result, bail};
use bridge_core::{HostPlatform, KeyboardBackend, KeyboardLayout, Mapper, MappingProfile};
use bridge_protocol::{Decoder, Frame, KeyEvent, MatrixState, Message, Sequencer};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
};

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(3);

pub fn platform() -> HostPlatform {
    #[cfg(target_os = "macos")]
    {
        HostPlatform::MacOS
    }
    #[cfg(target_os = "windows")]
    {
        HostPlatform::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        HostPlatform::Linux
    }
}
pub fn layout(config: &Config) -> KeyboardLayout {
    if config.layout == "UK" {
        KeyboardLayout::Uk
    } else {
        KeyboardLayout::Us
    }
}
pub fn mapping_profile(config: &Config) -> Result<MappingProfile> {
    let profile = if let Some(path) = &config.mappings_file {
        MappingProfile::from_toml(
            &std::fs::read_to_string(path).context("Cannot read MAPPINGS_FILE")?,
        )
        .context("Invalid custom mapping profile")
    } else {
        MappingProfile::builtin(&config.profile, &config.layout).context("Invalid mapping profile")
    }?;
    if profile.layout != layout(config) {
        bail!("Mapping profile layout must match KEYBOARD_LAYOUT");
    }
    Ok(profile)
}

type SharedStatus = Arc<Mutex<Status>>;
type SharedMatrix = Arc<Mutex<MatrixState>>;
type InputThread = thread::JoinHandle<std::result::Result<(), String>>;

fn finish_worker(thread: InputThread, result: Result<()>) -> Result<()> {
    let cleanup = thread
        .join()
        .map_err(|_| anyhow::anyhow!("Input worker panicked during shutdown"))?
        .map_err(anyhow::Error::msg)
        .context("Shutdown cleanup failed; native keys may remain held");
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => Err(error.context(format!("{cleanup:#}"))),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), cleanup) => cleanup,
    }
}

fn input_failure(
    reason: &str,
    error: impl std::fmt::Display,
    cleanup: std::result::Result<(), String>,
) -> String {
    let mut detail = format!("Input disabled after {reason}: {error}");
    if let Err(cleanup) = cleanup {
        detail.push_str(&format!(
            "; cleanup failed; native keys may remain held: {cleanup}"
        ));
    }
    detail
}
fn cleanup_outcome(result: &std::result::Result<(), String>) -> CleanupOutcome {
    if result.is_ok() {
        CleanupOutcome::Succeeded
    } else {
        CleanupOutcome::Failed
    }
}
struct InputEvent {
    generation: u64,
    event: KeyEvent,
    context: CurrentSession,
}
enum WorkerControl {
    Enable {
        enabled: bool,
        authorize_only: bool,
        generation: u64,
        reply: oneshot::Sender<std::result::Result<(), String>>,
        context: CurrentSession,
    },
    Reset {
        generation: u64,
        failure: Option<u64>,
        context: CurrentSession,
    },
    Synchronize {
        generation: u64,
        matrix: MatrixState,
        context: CurrentSession,
    },
    Prepared {
        authorization: u64,
        result: std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>,
    },
    Shutdown,
}
#[derive(Clone)]
struct WorkerHandle {
    events: mpsc::SyncSender<InputEvent>,
    controls: mpsc::Sender<WorkerControl>,
    generation: Arc<AtomicU64>,
    control_order: Arc<Mutex<()>>,
    status: SharedStatus,
}
impl WorkerHandle {
    async fn enable(&self, enabled: bool) -> Result<()> {
        self.request_input(enabled, false).await
    }
    async fn authorize(&self) -> Result<()> {
        self.request_input(false, true).await
    }
    async fn request_input(&self, enabled: bool, authorize_only: bool) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        {
            let _order = self.control_order.lock().unwrap();
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            self.controls
                .send(WorkerControl::Enable {
                    enabled,
                    authorize_only,
                    generation,
                    reply: tx,
                    context: self.status.lock().unwrap().diagnostics.current.clone(),
                })
                .map_err(|_| anyhow::anyhow!("Input worker stopped"))?;
        }
        rx.await
            .context("Input worker stopped")?
            .map_err(anyhow::Error::msg)
    }
    #[cfg(test)]
    fn reset(&self) {
        let context = self.status.lock().unwrap().diagnostics.current.clone();
        self.reset_after_failure(None, context);
    }
    fn reset_after_failure(&self, failure: Option<u64>, context: CurrentSession) -> bool {
        let _order = self.control_order.lock().unwrap();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.controls
            .send(WorkerControl::Reset {
                generation,
                failure,
                context,
            })
            .is_ok()
    }
    fn synchronize(&self, matrix: MatrixState) {
        let _order = self.control_order.lock().unwrap();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.controls.send(WorkerControl::Synchronize {
            generation,
            matrix,
            context: self.status.lock().unwrap().diagnostics.current.clone(),
        });
    }
    fn event(&self, event: KeyEvent) -> Result<()> {
        let _order = self.control_order.lock().unwrap();
        self.events
            .try_send(InputEvent {
                generation: self.generation.load(Ordering::SeqCst),
                event,
                context: self.status.lock().unwrap().diagnostics.current.clone(),
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => classified_failure(
                    FailureReason::QueueOverflow,
                    anyhow::anyhow!("Input queue overflow; disconnecting to preserve key state"),
                ),
                mpsc::TrySendError::Disconnected(_) => classified_failure(
                    FailureReason::Input,
                    anyhow::anyhow!("Input worker stopped before processing transition"),
                ),
            })
    }
}
fn spawn_worker(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
) -> Result<(WorkerHandle, InputThread)> {
    spawn_native_worker(config, status, physical, None)
}
fn spawn_worker_guarded(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    guard: Arc<AtomicBool>,
) -> Result<(WorkerHandle, InputThread)> {
    spawn_native_worker(
        config,
        status,
        physical,
        Some(InputGuard {
            allow: guard,
            focus_allowed: None,
        }),
    )
}
fn spawn_native_worker(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    guard: Option<InputGuard>,
) -> Result<(WorkerHandle, InputThread)> {
    let selection = config.backend.clone();
    let keyboard_layout = layout(config);
    let factory = if backend::requires_authorization(&selection)? {
        BackendFactory::Prepared {
            runtime: tokio::runtime::Handle::current(),
            prepare: Arc::new(move |cancellation| {
                let selection = selection.clone();
                Box::pin(async move {
                    backend::prepare_backend(&selection, keyboard_layout, cancellation).await
                })
            }),
        }
    } else {
        BackendFactory::Immediate(Box::new(move || {
            backend::create_backend(&selection, keyboard_layout)
        }))
    };
    spawn_worker_with_backend_factory(config, status, physical, factory, guard)
}

#[derive(Clone)]
struct InputGuard {
    allow: Arc<AtomicBool>,
    focus_allowed: Option<Arc<AtomicBool>>,
}
impl InputGuard {
    fn open(&self) -> bool {
        self.allow.load(Ordering::SeqCst)
            && self
                .focus_allowed
                .as_ref()
                .is_none_or(|focus| focus.load(Ordering::SeqCst))
    }
}

fn spawn_worker_guarded_with_focus(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    allow: Arc<AtomicBool>,
    focus_allowed: Arc<AtomicBool>,
) -> Result<(WorkerHandle, InputThread)> {
    spawn_native_worker(
        config,
        status,
        physical,
        Some(InputGuard {
            allow,
            focus_allowed: Some(focus_allowed),
        }),
    )
}

/// A GUI gate never prevents releases. Check every concrete output, including
/// modifier presses inside a resolved chord, rather than just its first event.
pub(crate) struct GuardedBackend {
    inner: Box<dyn KeyboardBackend>,
    guard: InputGuard,
}
impl GuardedBackend {
    pub(crate) fn new(inner: Box<dyn KeyboardBackend>, guard: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            guard: InputGuard {
                allow: guard,
                focus_allowed: None,
            },
        }
    }
    pub(crate) fn new_with_focus(
        inner: Box<dyn KeyboardBackend>,
        allow: Arc<AtomicBool>,
        focus_allowed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            inner,
            guard: InputGuard {
                allow,
                focus_allowed: Some(focus_allowed),
            },
        }
    }
    fn check(&self) -> std::result::Result<(), bridge_core::BackendError> {
        if self.guard.open() {
            Ok(())
        } else {
            Err(bridge_core::BackendError::Unavailable(
                "GUI input guard is closed".into(),
            ))
        }
    }
}
impl KeyboardBackend for GuardedBackend {
    fn capabilities(&self) -> bridge_core::BackendCapabilities {
        self.inner.capabilities()
    }
    fn validate_layout(&mut self) -> std::result::Result<(), bridge_core::BackendError> {
        self.inner.validate_layout()
    }
    fn press(
        &mut self,
        key: bridge_core::NativeKey,
    ) -> std::result::Result<(), bridge_core::BackendError> {
        self.check()?;
        self.inner.press(key)
    }
    fn release(
        &mut self,
        key: bridge_core::NativeKey,
    ) -> std::result::Result<(), bridge_core::BackendError> {
        self.inner.release(key)
    }
    fn repeat(
        &mut self,
        key: bridge_core::NativeKey,
    ) -> std::result::Result<(), bridge_core::BackendError> {
        self.check()?;
        self.inner.repeat(key)
    }
    fn type_text(&mut self, text: &str) -> std::result::Result<(), bridge_core::BackendError> {
        self.check()?;
        self.inner.type_text(text)
    }
    fn cleanup_pending_text(&mut self) -> std::result::Result<(), bridge_core::BackendError> {
        self.inner.cleanup_pending_text()
    }
}

type PreparationFuture = Pin<
    Box<
        dyn Future<
                Output = std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>,
            > + Send,
    >,
>;
type Preparer = Arc<dyn Fn(watch::Receiver<bool>) -> PreparationFuture + Send + Sync>;
enum BackendFactory {
    Immediate(
        Box<
            dyn Fn() -> std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>
                + Send,
        >,
    ),
    Prepared {
        runtime: tokio::runtime::Handle,
        prepare: Preparer,
    },
}
struct PendingPreparation {
    cancellation: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(test)]
fn spawn_worker_with_factory(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    factory: impl Fn() -> std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>
    + Send
    + 'static,
) -> Result<(WorkerHandle, InputThread)> {
    spawn_worker_with_factory_guarded(config, status, physical, factory, None)
}

#[cfg(test)]
fn spawn_worker_with_factory_guarded(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    factory: impl Fn() -> std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>
    + Send
    + 'static,
    guard: Option<Arc<AtomicBool>>,
) -> Result<(WorkerHandle, InputThread)> {
    spawn_worker_with_factory_gate(
        config,
        status,
        physical,
        factory,
        guard.map(|allow| InputGuard {
            allow,
            focus_allowed: None,
        }),
    )
}

#[cfg(test)]
fn spawn_worker_with_factory_gate(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    factory: impl Fn() -> std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>
    + Send
    + 'static,
    guard: Option<InputGuard>,
) -> Result<(WorkerHandle, InputThread)> {
    spawn_worker_with_backend_factory(
        config,
        status,
        physical,
        BackendFactory::Immediate(Box::new(factory)),
        guard,
    )
}
fn spawn_worker_with_backend_factory(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    factory: BackendFactory,
    guard: Option<InputGuard>,
) -> Result<(WorkerHandle, InputThread)> {
    let profile = mapping_profile(config)?;
    let (events_tx, events_rx) = mpsc::sync_channel::<InputEvent>(256);
    let (controls_tx, controls_rx) = mpsc::channel::<WorkerControl>();
    let generation = Arc::new(AtomicU64::new(0));
    let selection = config.backend.clone();
    let delay = config.repeat_delay;
    let interval = config.repeat_interval;
    let handle_status = status.clone();
    let prepared_tx = controls_tx.clone();
    let control_order = Arc::new(Mutex::new(()));
    let worker_order = control_order.clone();
    let worker_generation = generation.clone();
    let thread=thread::Builder::new().name("c64bridge-input".into()).spawn(move || {
        let mut mapper=Mapper::new(profile,platform());
        let mut preparations = BTreeMap::<u64, PendingPreparation>::new();
        let mut active_preparation: Option<(u64, bool)> = None;
        let mut authorization_sequence = 0_u64;
        let mut backend:Option<Box<dyn KeyboardBackend>>=None;
        let mut enabled=false;let mut epoch=0;let mut repeat_at:Option<Instant>=None;let mut pending:Option<InputEvent>=None;let mut layout_check_at=Instant::now();let mut worker_context=CurrentSession::default();
        let release=|mapper:&mut Mapper,backend:&mut Option<Box<dyn KeyboardBackend>>| ->std::result::Result<(),String> {
            if let Some(backend)=backend.as_mut() {mapper.release_all(backend.as_mut()).map_err(|e|e.to_string())}else{Ok(())}
        };
        let allowed=||guard.as_ref().is_none_or(InputGuard::open);
        loop {
            if enabled && !allowed() {
                enabled=false;repeat_at=None;
                let cleanup=release(&mut mapper,&mut backend);
                let mut state=status.lock().unwrap();state.enabled=false;
                if cleanup.is_err(){state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}
                state.detail=if let Err(error)=cleanup{format!("Input guard paused output; cleanup failed; ownership retained for retry: {error}")}else{"Input paused by GUI safety guard; fresh explicit enable required".into()};
            }
            while let Ok(control)=controls_rx.try_recv() {
                match control {
                    WorkerControl::Shutdown=>{
                        // Cancellation closes portal sessions, then completion is joined.
                        // No consent task or late grant is detached during shutdown.
                        for pending in preparations.values() { pending.cancellation.send_replace(true); }
                        if let BackendFactory::Prepared {runtime,..} = &factory {
                            for (_, pending) in std::mem::take(&mut preparations) { let _ = runtime.block_on(pending.task); }
                        }
                        while let Ok(control) = controls_rx.try_recv() { drop(control); }
                        let cleanup=release(&mut mapper,&mut backend);
                        if let Err(error)=&cleanup {
                            tracing::error!(%error,"Shutdown cleanup failed; native keys may remain held");
                            let mut state=status.lock().unwrap();state.enabled=false;state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);state.detail=format!("Shutdown cleanup failed; native keys may remain held: {error}");
                        }
                        return cleanup;
                    },
                    WorkerControl::Reset {generation,failure,context}=>{
                        worker_context=context;epoch=generation;repeat_at=None;
                        if let Some((id,_)) = active_preparation.take() {
                            if let Some(pending) = preparations.get(&id) { pending.cancellation.send_replace(true); }
                            status.lock().unwrap().authorization = AuthorizationStatus {phase:AuthorizationPhase::Cancelled,enable_requested:false};
                        }
                        let cleanup=release(&mut mapper,&mut backend);
                        let mut state=status.lock().unwrap();
                        if let Some(failure)=failure {state.diagnostics.complete_cleanup(failure,cleanup.is_ok());}else if cleanup.is_err(){state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}
                        state.detail=if let Err(error)=cleanup{enabled=false;state.enabled=false;format!("Input disabled; native key release failed: {error}")}else{"C64 disconnected; all bridge-owned keys released; waiting for reconnect".into()};
                    },
                    WorkerControl::Synchronize {generation,matrix,context}=>{
                        worker_context=context;epoch=generation;repeat_at=None;
                        let cleanup=release(&mut mapper,&mut backend);
                        let cleanup_failed=cleanup.is_err();
                        let result=cleanup.and_then(|()| {
                            if enabled {mapper.synchronize(matrix,backend.as_mut().unwrap().as_mut()).map_err(|e|e.to_string())}else{Ok(())}
                        });
                        let failure_cleanup=if result.is_err() && !cleanup_failed {Some(release(&mut mapper,&mut backend))}else{None};
                        let mut state=status.lock().unwrap();
                        if cleanup_failed {state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}else if let Some(cleanup)=failure_cleanup {state.diagnostics.record_worker_failure(FailureReason::Input,cleanup_outcome(&cleanup),&worker_context);}
                        state.detail=if let Err(error)=result{enabled=false;state.enabled=false;format!("Input disabled after synchronization failure: {error}")}else if enabled{"C64 synchronized; input enabled; held ordinary keys require release before typing".into()}else{"C64 synchronized; input disabled; receiving physical transitions".into()};
                    },
                    WorkerControl::Enable {enabled:requested,authorize_only,generation,reply,context}=>{
                        worker_context=context;epoch=generation;repeat_at=None;
                        if let Some((id,_)) = active_preparation.take() {
                            if let Some(pending) = preparations.get(&id) { pending.cancellation.send_replace(true); }
                            status.lock().unwrap().authorization = AuthorizationStatus {phase:AuthorizationPhase::Cancelled,enable_requested:false};
                        }
                        let cleanup=release(&mut mapper,&mut backend);
                        let cleanup_failed=cleanup.is_err();
                        enabled=false;status.lock().unwrap().enabled=false;
                        if !cleanup_failed && matches!(factory,BackendFactory::Prepared {..}) && status.lock().unwrap().authorization.phase==AuthorizationPhase::Failed {backend=None;}
                        if !cleanup_failed && (requested || authorize_only) && backend.is_none()
                            && let BackendFactory::Prepared {runtime,prepare} = &factory
                        {
                            if requested && !allowed() {
                                let _ = reply.send(Err("GUI input guard is closed; input remains disabled".into()));
                                continue;
                            }
                            if preparations.len()>=4 {
                                let _=reply.send(Err("Previous desktop authorization is still closing; retry after cleanup".into()));
                                continue;
                            }
                            authorization_sequence = authorization_sequence.wrapping_add(1);
                            let authorization = authorization_sequence;
                            let (cancel, cancel_rx) = watch::channel(false);
                            let future = prepare(cancel_rx);
                            let controls = prepared_tx.clone();
                            let task = runtime.spawn(async move {
                                let result = future.await;
                                let _ = controls.send(WorkerControl::Prepared { authorization, result });
                            });
                            preparations.insert(authorization, PendingPreparation { cancellation:cancel, task });
                            active_preparation=Some((authorization,requested));
                            let mut state=status.lock().unwrap();
                            state.authorization=AuthorizationStatus {phase:AuthorizationPhase::Pending,enable_requested:requested};
                            state.detail="Desktop keyboard authorization pending; input remains disabled. Pause cancels the request".into();
                            drop(state);
                            let _=reply.send(Ok(()));
                            continue;
                        }
                        let mut result=if cleanup_failed{cleanup}else if authorize_only && matches!(factory,BackendFactory::Immediate(_)){Err("The running host backend does not use desktop portal authorization; input is paused".into())}else if requested && !allowed() {Err("GUI input guard is closed; input remains disabled".into())}else if requested {
                            if backend.is_none() {match &factory {BackendFactory::Immediate(factory)=>match factory() {Ok(b)=>backend=Some(if let Some(guard)=&guard {Box::new(GuardedBackend {inner:b,guard:guard.clone()})}else{b}),Err(e)=>{let mut state=status.lock().unwrap();state.diagnostics.record_worker_failure(FailureReason::Input,CleanupOutcome::Succeeded,&worker_context);state.detail=e.to_string();drop(state);let _=reply.send(Err(e.to_string()));continue;}},BackendFactory::Prepared {..}=>unreachable!()}}
                            if !allowed() {Err("GUI input guard closed during authorization; input remains disabled".into())}
                            else {let output=backend.as_mut().unwrap().as_mut();output.validate_layout().map_err(|error|error.to_string()).and_then(|()|mapper.synchronize(*physical.lock().unwrap(),output).map_err(|error|error.to_string()))}
                        } else {Ok(())};
                        if requested && result.is_ok() && !allowed() {result=Err("GUI input guard closed before enable acknowledgment; input remains disabled".into());}
                        let failure_cleanup=if result.is_err() && !cleanup_failed {Some(release(&mut mapper,&mut backend))}else{None};
                        let discard_failed_portal=matches!(factory,BackendFactory::Prepared {..}) && result.is_err() && allowed() && !cleanup_failed && failure_cleanup.as_ref().is_some_and(|cleanup|cleanup.is_ok());
                        if discard_failed_portal {backend=None;}
                        enabled=requested && result.is_ok();
                        {let mut state=status.lock().unwrap();
                        state.authorization=AuthorizationStatus {phase:if discard_failed_portal{AuthorizationPhase::Failed}else if backend.is_some() && matches!(factory,BackendFactory::Prepared {..}){AuthorizationPhase::Ready}else{state.authorization.phase},enable_requested:false};
                        if cleanup_failed {state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}else if let Some(cleanup)=failure_cleanup {
                            if allowed() {state.diagnostics.record_worker_failure(FailureReason::Input,cleanup_outcome(&cleanup),&worker_context);}
                            else if cleanup.is_err() {state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}
                        }
                        state.enabled=enabled;state.backend=if backend.is_some(){selection.clone()}else{"disabled".into()};state.detail=if let Err(error)=&result{format!("Input disabled; cleanup/authorization failed: {error}")}else if enabled{"Input enabled; held ordinary keys require release before typing".into()}else{"Input paused; all bridge-owned keys released".into()};}
                        let _=reply.send(result);
                    },
                    WorkerControl::Prepared {authorization,result}=>{
                        if let Some(pending)=preparations.remove(&authorization)
                            && let BackendFactory::Prepared {runtime,..}=&factory
                        { let _=runtime.block_on(pending.task); }
                        let Some((active,requested))=active_preparation else { drop(result); continue; };
                        if authorization!=active { drop(result); continue; }
                        // A safety/snapshot control may already be queued with a
                        // newer event generation. Process it first; never lower
                        // the input epoch after installing a prepared backend.
                        let order=worker_order.lock().unwrap();
                        if worker_generation.load(Ordering::SeqCst)!=epoch {
                            drop(order);
                            let _=prepared_tx.send(WorkerControl::Prepared {authorization,result});
                            continue;
                        }
                        active_preparation=None;
                        if requested && !allowed() {
                            drop(order);drop(result);
                            let mut state=status.lock().unwrap();
                            state.authorization=AuthorizationStatus {phase:AuthorizationPhase::Cancelled,enable_requested:false};
                            state.detail="Desktop authorization cancelled by the GUI safety guard; input remains disabled".into();
                            continue;
                        }
                        let result=match result {
                            Ok(output)=>{
                                // Invalidate every event queued before consent, then quarantine
                                // the latest physical baseline. Held ordinary keys never replay.
                                epoch=worker_generation.fetch_add(1,Ordering::SeqCst)+1;
                                backend=Some(if let Some(guard)=&guard {Box::new(GuardedBackend {inner:output,guard:guard.clone()})}else{output});
                                let output=backend.as_mut().unwrap().as_mut();
                                output.validate_layout().map_err(|error|error.to_string()).and_then(|()|if requested && allowed(){mapper.synchronize(*physical.lock().unwrap(),output).map_err(|error|error.to_string())}else{Ok(())})
                            },
                            Err(error)=>{
                                let mut state=status.lock().unwrap();
                                state.authorization=AuthorizationStatus {phase:if matches!(error,bridge_core::BackendError::PermissionDenied(_)){AuthorizationPhase::Denied}else{AuthorizationPhase::Failed},enable_requested:false};
                                state.diagnostics.record_worker_failure(FailureReason::Input,CleanupOutcome::Succeeded,&worker_context);
                                state.detail="Desktop keyboard authorization failed or was denied; input remains disabled".into();
                                continue;
                            }
                        };
                        drop(order);
                        enabled=requested && result.is_ok() && allowed();
                        let cleanup=if result.is_err(){Some(release(&mut mapper,&mut backend))}else{None};
                        if result.is_err() && cleanup.as_ref().is_some_and(|cleanup|cleanup.is_ok()) {backend=None;}
                        let mut state=status.lock().unwrap();
                        if let Some(cleanup)=cleanup {state.diagnostics.record_worker_failure(FailureReason::Layout,cleanup_outcome(&cleanup),&worker_context);}
                        state.enabled=enabled;
                        state.backend=if backend.is_some(){selection.clone()}else{"disabled".into()};
                        state.authorization=AuthorizationStatus {phase:if result.is_ok(){AuthorizationPhase::Ready}else{AuthorizationPhase::Failed},enable_requested:false};
                        state.detail=if result.is_err(){"Desktop keyboard layout validation failed; input remains disabled".into()}else if enabled{"Input enabled after desktop authorization; held ordinary keys require release before typing".into()}else{"Desktop keyboard authorization ready; input remains paused".into()};
                    }
                }
            }
            let input=if let Some(input)=pending.take(){Ok(input)}else{events_rx.recv_timeout(Duration::from_millis(5))};
            let queue_idle=matches!(&input,Err(mpsc::RecvTimeoutError::Timeout));
            match input {
                Ok(input) if input.generation>epoch=>{pending=Some(input);thread::sleep(Duration::from_millis(1));continue;},
                Ok(input) if enabled && input.generation==epoch=>{
                    worker_context=input.context.clone();
                    let blocked_before=mapper.blocked_combinations();
                    let output=backend.as_mut().unwrap().as_mut();
                    let validation=output.validate_layout();
                    let failure_reason=if validation.is_err(){FailureReason::Layout}else{FailureReason::Input};
                    let result=validation.map_err(bridge_core::MappingError::Backend).and_then(|()|mapper.transition(input.event,output));
                    if let Err(error)=result {
                        tracing::debug!(key=?input.event.key,pressed=input.event.pressed,modifiers=input.event.modifiers,generation=input.generation,"Input worker rejected physical transition");
                        // A mapping rejection must not leave native keys held. Core also guards this.
                        let cleanup=release(&mut mapper,&mut backend);enabled=false;repeat_at=None;
                        let mut state=status.lock().unwrap();state.enabled=false;
                        if allowed() {state.diagnostics.record_worker_failure(failure_reason,cleanup_outcome(&cleanup),&worker_context);state.detail=input_failure("mapping/backend failure",error,cleanup);}
                        else {if cleanup.is_err(){state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}state.detail=if let Err(error)=cleanup{format!("Input guard paused output; cleanup failed; ownership retained for retry: {error}")}else{"Input paused by GUI safety guard; fresh explicit enable required".into()};}
                    } else {
                        tracing::debug!(key=?input.event.key,pressed=input.event.pressed,modifiers=input.event.modifiers,generation=input.generation,admitted=mapper.is_key_admitted(input.event.key),blocked=mapper.blocked_combinations()>blocked_before,quarantined=mapper.is_quarantined(),"Input worker processed physical transition");
                        if input.event.pressed && !input.event.key.is_modifier() && mapper.is_key_admitted(input.event.key) {repeat_at=Some(Instant::now()+delay);}
                        if mapper.blocked_combinations()>blocked_before {let mut state=status.lock().unwrap();state.blocked_combinations=mapper.blocked_combinations();state.detail="Incompatible key combination blocked; release that physical key before retrying".into();}
                    }
                }
                Ok(input)=>{tracing::debug!(key=?input.event.key,pressed=input.event.pressed,generation=input.generation,worker_generation=epoch,input_enabled=enabled,"Input worker skipped physical transition");},
                Err(mpsc::RecvTimeoutError::Disconnected)=>{return release(&mut mapper,&mut backend);},
                Err(mpsc::RecvTimeoutError::Timeout)=>{},
            }
            let monitor_authorization=matches!(factory,BackendFactory::Prepared {..}) && backend.is_some()
                && status.lock().unwrap().authorization.phase==AuthorizationPhase::Ready;
            if (enabled || monitor_authorization) && Instant::now()>=layout_check_at {
                layout_check_at=Instant::now()+Duration::from_millis(100);
                if let Err(error)=backend.as_mut().unwrap().validate_layout() {
                    let cleanup=release(&mut mapper,&mut backend);enabled=false;repeat_at=None;
                    let mut state=status.lock().unwrap();state.enabled=false;if matches!(factory,BackendFactory::Prepared {..}){state.authorization=AuthorizationStatus {phase:AuthorizationPhase::Failed,enable_requested:false};}state.diagnostics.record_worker_failure(FailureReason::Layout,cleanup_outcome(&cleanup),&worker_context);let released=cleanup.is_ok();state.detail=input_failure(if monitor_authorization{"host layout or authorization change"}else{"host layout change"},error,cleanup);drop(state);if monitor_authorization && released {backend=None;}
                }
            }
            if queue_idle && enabled && repeat_at.is_some_and(|t|Instant::now()>=t) {
                // A queued release must take precedence over an expired repeat,
                // including transitions received during the native layout check.
                match events_rx.try_recv() {
                    Ok(input)=>{pending=Some(input);continue;},
                    Err(mpsc::TryRecvError::Empty)=>{},
                    Err(mpsc::TryRecvError::Disconnected)=>{return release(&mut mapper,&mut backend);},
                }
                if let Err(error)=mapper.repeat(backend.as_mut().unwrap().as_mut()) {
                    let cleanup=release(&mut mapper,&mut backend);enabled=false;
                    let mut state=status.lock().unwrap();state.enabled=false;
                    if allowed(){state.diagnostics.record_worker_failure(FailureReason::Repeat,cleanup_outcome(&cleanup),&worker_context);state.detail=input_failure("repeat failure",error,cleanup);}
                    else {if cleanup.is_err(){state.diagnostics.record_worker_failure(FailureReason::Cleanup,CleanupOutcome::Failed,&worker_context);}state.detail=if let Err(error)=cleanup{format!("Input guard paused output; cleanup failed; ownership retained for retry: {error}")}else{"Input paused by GUI safety guard; fresh explicit enable required".into()};}
                }
                repeat_at=Some(Instant::now()+interval);
            }
        }
    }).context("Cannot start input worker")?;
    Ok((
        WorkerHandle {
            events: events_tx,
            controls: controls_tx,
            generation,
            control_order,
            status: handle_status,
        },
        thread,
    ))
}

pub struct FrameReader {
    decoder: Decoder,
    pending: VecDeque<Frame>,
}
impl Default for FrameReader {
    fn default() -> Self {
        Self {
            decoder: Decoder::new(),
            pending: VecDeque::new(),
        }
    }
}
impl FrameReader {
    pub async fn next(&mut self, socket: &mut TcpStream) -> Result<Frame> {
        loop {
            if let Some(frame) = self.pending.pop_front() {
                return Ok(frame);
            }
            let mut bytes = [0; 512];
            let n = socket.read(&mut bytes).await?;
            if n == 0 {
                bail!("C64 disconnected");
            }
            self.pending.extend(
                self.decoder
                    .push(&bytes[..n])
                    .context("Malformed bridge frame")?,
            );
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
struct SessionFailure {
    reason: FailureReason,
    #[source]
    source: anyhow::Error,
}
fn classified_failure(reason: FailureReason, source: anyhow::Error) -> anyhow::Error {
    SessionFailure { reason, source }.into()
}
fn protocol_failure(source: anyhow::Error) -> anyhow::Error {
    classified_failure(FailureReason::Protocol, source)
}
fn read_failure(source: anyhow::Error) -> anyhow::Error {
    let reason = if source
        .downcast_ref::<bridge_protocol::ProtocolError>()
        .is_some()
    {
        FailureReason::Protocol
    } else {
        FailureReason::Transport
    };
    classified_failure(reason, source)
}
pub async fn write_frame(
    socket: &mut TcpStream,
    sequence: &mut Sequencer,
    message: Message,
) -> Result<()> {
    let bytes = Frame {
        sequence: sequence.next_sequence(),
        message,
    }
    .encode()
    .map_err(|error| protocol_failure(error.into()))?;
    match tokio::time::timeout(SESSION_TIMEOUT, socket.write_all(&bytes)).await {
        Err(error) => Err(classified_failure(
            FailureReason::WriteTimeout,
            anyhow::Error::new(error).context("TCP write timed out"),
        )),
        Ok(Err(error)) => Err(classified_failure(FailureReason::Transport, error.into())),
        Ok(Ok(())) => Ok(()),
    }
}

// All callers, including the recording harness, use this termination path.
// Record the failure before asynchronous reset can replace the human-readable detail.
async fn session(
    socket: TcpStream,
    token: Token,
    worker: WorkerHandle,
    status: SharedStatus,
    physical: SharedMatrix,
    stopping: watch::Receiver<bool>,
) -> Result<()> {
    status.lock().unwrap().diagnostics.begin();
    let result = session_inner(
        socket,
        token,
        worker.clone(),
        status.clone(),
        physical.clone(),
        stopping,
    )
    .await;
    let (failure, context) = {
        let mut state = status.lock().unwrap();
        let context = state.diagnostics.current.clone();
        let failure = result.as_ref().err().map(|error| {
            let reason = error
                .downcast_ref::<SessionFailure>()
                .map(|error| error.reason)
                .unwrap_or(FailureReason::Transport);
            state
                .diagnostics
                .record_failure(reason, CleanupOutcome::Pending)
        });
        state.diagnostics.end();
        state.connected = false;
        state.device = None;
        state.detail = "C64 session ended; releasing bridge-owned keys".into();
        (failure, context)
    };
    *physical.lock().unwrap() = MatrixState::default();
    if !worker.reset_after_failure(failure, context.clone()) {
        let mut state = status.lock().unwrap();
        state.enabled = false;
        if let Some(failure) = failure {
            state.diagnostics.complete_cleanup(failure, false);
        } else {
            state.diagnostics.record_worker_failure(
                FailureReason::Cleanup,
                CleanupOutcome::Failed,
                &context,
            );
        }
        state.detail =
            "Input worker stopped before session cleanup; native keys may remain held".into();
    }
    result
}

async fn session_inner(
    mut socket: TcpStream,
    token: Token,
    worker: WorkerHandle,
    status: SharedStatus,
    physical: SharedMatrix,
    mut stopping: watch::Receiver<bool>,
) -> Result<()> {
    socket
        .set_nodelay(true)
        .map_err(|error| classified_failure(FailureReason::Transport, error.into()))?;
    let mut reader = FrameReader::default();
    let mut incoming = Sequencer::new();
    let mut outgoing = Sequencer::new();
    let hello = tokio::select! {
        biased;
        _=cancellation(&mut stopping)=>return Ok(()),
        hello=tokio::time::timeout(Duration::from_secs(5),reader.next(&mut socket))=>{
            hello.map_err(|error|classified_failure(FailureReason::HelloTimeout,anyhow::Error::new(error).context("HELLO timed out")))?.map_err(read_failure)?
        },
    };
    incoming
        .check(hello.sequence)
        .map_err(|error| protocol_failure(error.into()))?;
    let Message::Hello {
        token: offered,
        device_id,
        capabilities,
    } = hello.message
    else {
        return Err(protocol_failure(anyhow::anyhow!(
            "HELLO required before keyboard events"
        )));
    };
    if !token.matches(&offered) {
        return Err(classified_failure(
            FailureReason::Authentication,
            anyhow::anyhow!("Client authentication failed"),
        ));
    }
    if capabilities & 1 == 0 {
        return Err(protocol_failure(anyhow::anyhow!(
            "Client does not advertise keyboard capability"
        )));
    }
    {
        let mut state = status.lock().unwrap();
        state.diagnostics.authenticated();
        state.diagnostics.valid_frame();
        state.connected = true;
        state.device = Some(device_id);
        state.detail = "Authenticated C64; waiting for state synchronization".into();
    }
    let enabled = status.lock().unwrap().enabled;
    write_frame(&mut socket, &mut outgoing, Message::HelloAck { enabled }).await?;
    let first = tokio::select! {
        biased;
        _=cancellation(&mut stopping)=>return Ok(()),
        first=tokio::time::timeout(SESSION_TIMEOUT,reader.next(&mut socket))=>{
            first.map_err(|error|classified_failure(FailureReason::SyncTimeout,anyhow::Error::new(error).context("Initial state synchronization timed out")))?.map_err(read_failure)?
        },
    };
    incoming
        .check(first.sequence)
        .map_err(|error| protocol_failure(error.into()))?;
    let Message::StateSync(matrix) = first.message else {
        return Err(protocol_failure(anyhow::anyhow!(
            "Initial STATE_SYNC required"
        )));
    };
    *physical.lock().unwrap() = matrix;
    {
        let mut state = status.lock().unwrap();
        state.diagnostics.valid_frame();
        state.diagnostics.phase(SessionPhase::Active);
    }
    worker.synchronize(matrix);
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_received = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _=cancellation(&mut stopping)=>{
                status.lock().unwrap().diagnostics.phase(SessionPhase::Closing);
                let _=write_frame(&mut socket,&mut outgoing,Message::Disconnect {reason:0}).await;
                return Ok(());
            }
            _=tokio::time::sleep_until(last_received+SESSION_TIMEOUT)=>return Err(classified_failure(FailureReason::HeartbeatTimeout,anyhow::anyhow!("C64 heartbeat timed out"))),
            _=heartbeat.tick()=>{
                if last_received.elapsed()>=SESSION_TIMEOUT {return Err(classified_failure(FailureReason::HeartbeatTimeout,anyhow::anyhow!("C64 heartbeat timed out")));}
                let enabled=status.lock().unwrap().enabled;
                write_frame(&mut socket,&mut outgoing,Message::HostHeartbeat {enabled}).await?;
            }
            frame=reader.next(&mut socket)=>{
                let frame=frame.map_err(read_failure)?;
                incoming.check(frame.sequence).map_err(|error|protocol_failure(error.into()))?;
                let mut synchronization=None;
                let event=match frame.message {
                    message @ (Message::KeyDown {..}|Message::KeyUp {..})=>{
                        let event=message.key_event().expect("key message");
                        {
                            let mut matrix=physical.lock().unwrap();
                            if matrix.get(event.key)==event.pressed {return Err(protocol_failure(anyhow::anyhow!("Duplicate physical transition")));}
                            matrix.set(event.key,event.pressed);
                            if matrix.modifiers()!=event.modifiers {return Err(protocol_failure(anyhow::anyhow!("Modifier mask disagrees with physical state")));}
                        }
                        let mut state=status.lock().unwrap();
                        state.events=state.events.saturating_add(1);
                        tracing::debug!(key=?event.key,pressed=event.pressed,modifiers=event.modifiers,"Physical C64 transition");
                        Some(event)
                    }
                    Message::Heartbeat(snapshot)=>{
                        if snapshot != *physical.lock().unwrap(){return Err(protocol_failure(anyhow::anyhow!("Heartbeat state disagrees with ordered transitions")));}
                        None
                    }
                    Message::StateSync(snapshot)=>{
                        if snapshot != *physical.lock().unwrap(){return Err(protocol_failure(anyhow::anyhow!("State synchronization disagrees with ordered transitions")));}
                        synchronization=Some(snapshot);
                        None
                    },
                    Message::Disconnect {..}=>{
                        status.lock().unwrap().diagnostics.valid_frame();
                        return Ok(());
                    },
                    Message::Error {code,..}=>{
                        if code!=0 {return Err(protocol_failure(anyhow::anyhow!("C64 reported client error code {code}")));}
                        None
                    },
                    _=>return Err(protocol_failure(anyhow::anyhow!("Unexpected message after session initialization"))),
                };
                status.lock().unwrap().diagnostics.valid_frame();
                last_received=tokio::time::Instant::now();
                if let Some(snapshot)=synchronization {worker.synchronize(snapshot);}
                if let Some(event)=event {
                    worker.event(event)?;
                }
            }
        }
    }
}

pub struct StartOptions {
    pub enable_input: bool,
    pub deploy: bool,
    pub allow_reset: bool,
    pub template: Vec<u8>,
    pub fake_device: bool,
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut termination =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("Cannot listen for SIGTERM")?;
        tokio::select! {
            result=tokio::signal::ctrl_c()=>result.context("Cannot listen for interrupt"),
            _=termination.recv()=>Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .context("Cannot listen for interrupt")
}

/// A process-local observation of managed startup and graceful completion.
/// Readiness means startup (including any requested deployment) has finished,
/// not that a C64 is authenticated or that native input is authorized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Starting,
    Ready,
    Finished(std::result::Result<(), String>),
}

/// Owns cancellation of a bridge started by a frontend. Dropping this handle
/// requests a graceful stop; the detached task still performs owned-key cleanup.
/// The frontend should wait for completion before exiting its Tokio runtime.
pub struct ManagedRuntime {
    stopping: watch::Sender<bool>,
    state: watch::Receiver<RuntimeState>,
    state_dir: std::path::PathBuf,
}
impl ManagedRuntime {
    /// Must be called on an active Tokio runtime. Startup performs no native
    /// injection unless `enable_input` was deliberately requested.
    pub fn spawn(config: Config, options: StartOptions) -> Self {
        Self::spawn_with_worker(config, options, spawn_worker)
    }
    /// A GUI-owned runtime additionally gates every new native output against
    /// a process-local safety flag. Reopening it never resumes input implicitly.
    pub fn spawn_guarded(config: Config, options: StartOptions, guard: Arc<AtomicBool>) -> Self {
        Self::spawn_with_worker(config, options, move |config, status, physical| {
            spawn_worker_guarded(config, status, physical, guard)
        })
    }
    /// Keep focus permission independent from asynchronous arming. A stale
    /// countdown may reopen `allow`, but cannot overwrite GUI-owned focus state.
    pub fn spawn_guarded_with_focus(
        config: Config,
        options: StartOptions,
        allow: Arc<AtomicBool>,
        focus_allowed: Arc<AtomicBool>,
    ) -> Self {
        Self::spawn_with_worker(config, options, move |config, status, physical| {
            spawn_worker_guarded_with_focus(config, status, physical, allow, focus_allowed)
        })
    }
    fn spawn_with_worker(
        config: Config,
        options: StartOptions,
        factory: impl FnOnce(&Config, SharedStatus, SharedMatrix) -> Result<(WorkerHandle, InputThread)>
        + Send
        + 'static,
    ) -> Self {
        let state_dir = config.state_dir.clone();
        let (stopping, stop) = watch::channel(false);
        let (state_tx, state) = watch::channel(RuntimeState::Starting);
        let task_stop = stopping.clone();
        tokio::spawn(async move {
            let result = run_managed(config, options, task_stop, stop, &state_tx, factory)
                .await
                .map_err(|error| format!("{error:#}"));
            state_tx.send_replace(RuntimeState::Finished(result));
        });
        Self {
            stopping,
            state,
            state_dir,
        }
    }
    /// Stop an owned session, preserving a live input worker for retry if the
    /// initial pause cannot release its keys. GUI Quit/restart should use this
    /// rather than dropping the handle or cancelling a ready runtime directly.
    pub async fn stop(&mut self) -> Result<()> {
        let ready = matches!(*self.state.borrow(), RuntimeState::Ready);
        if ready {
            let response = control::send(&self.state_dir, "pause").await?;
            if !response.ok {
                bail!(
                    "{}",
                    response.error.unwrap_or_else(|| {
                        "Native key cleanup failed; retry Stop after restoring backend access"
                            .into()
                    })
                );
            }
        }
        self.request_stop();
        self.wait().await
    }
    pub fn request_stop(&self) {
        self.stopping.send_replace(true);
    }
    pub fn subscribe(&self) -> watch::Receiver<RuntimeState> {
        self.state.clone()
    }
    pub fn is_finished(&self) -> bool {
        self.completion().is_some()
    }
    pub fn completion(&self) -> Option<std::result::Result<(), String>> {
        match &*self.state.borrow() {
            RuntimeState::Finished(result) => Some(result.clone()),
            _ => None,
        }
    }
    pub async fn ready(&mut self) -> Result<()> {
        loop {
            match self.state.borrow().clone() {
                RuntimeState::Ready => return Ok(()),
                RuntimeState::Finished(Ok(())) => bail!("Bridge stopped before startup completed"),
                RuntimeState::Finished(Err(error)) => return Err(anyhow::Error::msg(error)),
                RuntimeState::Starting => {}
            }
            self.state
                .changed()
                .await
                .context("Managed runtime task ended before reporting startup")?;
        }
    }
    pub async fn wait(&mut self) -> Result<()> {
        loop {
            if let Some(result) = self.completion() {
                return result.map_err(anyhow::Error::msg);
            }
            self.state
                .changed()
                .await
                .context("Managed runtime task ended before reporting completion")?;
        }
    }
}
impl Drop for ManagedRuntime {
    fn drop(&mut self) {
        self.request_stop();
    }
}

async fn cancellation(stopping: &mut watch::Receiver<bool>) {
    loop {
        if *stopping.borrow_and_update() {
            return;
        }
        if stopping.changed().await.is_err() {
            return;
        }
    }
}

/// The command-line wrapper alone installs process signal handlers.
/// GUI frontends use `ManagedRuntime` and cancel their owned session explicitly.
pub async fn start(config: Config, options: StartOptions) -> Result<()> {
    let mut runtime = ManagedRuntime::spawn(config, options);
    let signal = tokio::select! {
        result=runtime.wait()=>return result,
        result=shutdown_signal()=>result,
    };
    runtime.request_stop();
    let completion = runtime.wait().await;
    signal.and(completion)
}

async fn run_managed(
    config: Config,
    options: StartOptions,
    stopping_tx: watch::Sender<bool>,
    mut stopping_rx: watch::Receiver<bool>,
    state: &watch::Sender<RuntimeState>,
    factory: impl FnOnce(&Config, SharedStatus, SharedMatrix) -> Result<(WorkerHandle, InputThread)>
    + Send
    + 'static,
) -> Result<()> {
    // Cancel before creating private state, and while DNS or listener setup waits.
    let setup = async {
        let network = config.network().await?;
        let token = crate::pairing::load_or_create_token(&config.state_dir)?;
        let listener = TcpListener::bind(network.bind).await.map_err(|error| {
            let message = if error.kind() == std::io::ErrorKind::AddrInUse {
                "Bridge TCP port is already in use. Stop the conflicting listener or explicitly change BRIDGE_PORT. Changing the port requires redeploying the C64 client; no alternative port is selected automatically."
            } else {
                "Cannot bind bridge TCP listener; check the bind interface and local permissions"
            };
            anyhow::Error::new(error).context(message)
        })?;
        let control_listener = TcpListener::bind("127.0.0.1:0").await?;
        let guard = RuntimeGuard::create(
            &config.state_dir,
            &RuntimeRecord {
                pid: std::process::id(),
                port: control_listener.local_addr()?.port(),
            },
        )?;
        Ok::<_, anyhow::Error>((network, token, listener, control_listener, guard))
    };
    let (network, token, listener, control_listener, _guard) = tokio::select! {
        biased;
        _=cancellation(&mut stopping_rx)=>return Ok(()),
        result=setup=>result?,
    };
    let status = Arc::new(Mutex::new(Status::default()));
    let physical = Arc::new(Mutex::new(MatrixState::default()));
    let (worker, thread) = factory(&config, status.clone(), physical.clone())?;
    let control_task = tokio::spawn(serve_control(
        control_listener,
        token,
        worker.clone(),
        status.clone(),
        stopping_tx.clone(),
    ));
    let setup = async {
        if options.enable_input {
            worker.enable(true).await?;
        }
        let device = if !options.fake_device
            && (config.auto_connect || options.deploy || config.auto_deploy)
        {
            Some(crate::rest::RestClient::new(&config)?.info().await?)
        } else {
            None
        };
        if options.deploy || config.auto_deploy {
            if options.fake_device {
                bail!("Cannot deploy in fake-device mode");
            }
            if !options.allow_reset {
                bail!("Automatic deployment resets the C64; provide --allow-reset explicitly");
            }
            let identity = device
                .as_ref()
                .map(|d| d.unique_id.as_str())
                .unwrap_or("C64-Ultimate");
            let patched = crate::prg::patch(
                &options.template,
                network.advertise_ip,
                config.bridge_port,
                token,
                identity,
            )?;
            tracing::warn!(
                "Deploying and running the client resets the C64 and interrupts current software"
            );
            crate::rest::RestClient::new(&config)?
                .upload(patched, true, options.allow_reset)
                .await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    let mut stop = stopping_rx.clone();
    let setup = tokio::select! {
        biased;
        _=cancellation(&mut stop)=>Ok(false),
        result=setup=>result.map(|()|true),
    };
    let mut active: Option<tokio::task::JoinHandle<()>> = None;
    let result = if matches!(setup, Ok(true)) {
        tracing::info!(address=%network.bind,advertise=%network.advertise_ip,input_enabled=status.lock().unwrap().enabled,"Bridge ready");
        state.send_replace(RuntimeState::Ready);
        loop {
            tokio::select! {
                biased;
                _=cancellation(&mut stop)=>break Ok(()),
                accepted=listener.accept()=>{
                    let (socket,address)=match accepted {Ok(pair)=>pair,Err(error)=>break Err(error.into())};
                    if !matches!(address.ip(),std::net::IpAddr::V4(ip) if network.allowed_ips.contains(&ip)) {tracing::warn!("Rejected client outside the C64 IP allowlist");continue;}
                    if active.as_ref().is_some_and(|task|!task.is_finished()) {tracing::warn!("Rejected second client while a session is active");continue;}
                    if let Some(task)=active.take(){let _=task.await;}
                    let worker=worker.clone();let state=status.clone();let matrix=physical.clone();let stopping=stopping_rx.clone();
                    active=Some(tokio::spawn(async move {
                        let result=session(socket,token,worker,state,matrix,stopping).await;
                        if let Err(error)=result{tracing::warn!(%error,"C64 session ended after a network or protocol failure");}
                    }));
                }
            }
        }
    } else {
        setup.map(|_| ())
    };
    stopping_tx.send_replace(true);
    // Session operations are already bounded. Never abort a task before its
    // session-reset acknowledgment or the ordered input worker's final cleanup.
    if let Some(task) = active {
        let _ = task.await;
    }
    let _ = control_task.await;
    let _ = worker.controls.send(WorkerControl::Shutdown);
    drop(worker);
    tokio::task::spawn_blocking(move || finish_worker(thread, result))
        .await
        .context("Input cleanup task failed")?
}
async fn serve_control(
    listener: TcpListener,
    token: Token,
    worker: WorkerHandle,
    status: SharedStatus,
    stop: watch::Sender<bool>,
) -> Result<()> {
    let mut stopping = stop.subscribe();
    loop {
        let (mut socket, _) = tokio::select! {
            biased;
            _ = cancellation(&mut stopping) => return Ok(()),
            socket = listener.accept() => socket?,
        };
        let request = tokio::time::timeout(
            Duration::from_secs(2),
            control::read_json::<ControlRequest>(&mut socket),
        )
        .await;
        let Ok(Ok(request)) = request else {
            continue;
        };
        if !control::authorized(token, &request) {
            continue;
        }
        let stop_after_reply = request.command == "stop";
        let result = match request.command.as_str() {
            "status" => Ok(()),
            "pause" => worker.enable(false).await,
            "resume" => worker.enable(true).await,
            "authorize" => worker.authorize().await,
            "stop" => worker.enable(false).await,
            _ => Err(anyhow::anyhow!("Unknown control command")),
        };
        let response = ControlResponse {
            ok: result.is_ok(),
            status: status.lock().unwrap().snapshot(),
            error: result.err().map(|e| e.to_string()),
        };
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            control::write_json(&mut socket, &response),
        )
        .await;
        if stop_after_reply {
            let _ = stop.send(true);
            return Ok(());
        }
    }
}

pub const PROBE_BYTES: &[u8; 32] = b"HELLO FROM C64 ULTIMATE\n\x00\x01\x7f\x80\xffKB\x00";
async fn probe_read(
    socket: &mut TcpStream,
    bytes: &mut [u8],
    stage: &str,
    timeout: Duration,
) -> Result<()> {
    let read = async {
        let mut received = 0;
        while received < bytes.len() {
            let count = socket
                .read(&mut bytes[received..])
                .await
                .with_context(|| format!("Probe read failed during {stage}"))?;
            if count == 0 {
                bail!(
                    "C64 disconnected during {stage} ({received}/{} bytes)",
                    bytes.len()
                );
            }
            received += count;
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(timeout, read)
        .await
        .with_context(|| format!("Probe timed out during {stage}"))??;
    tracing::info!(stage, bytes = bytes.len(), "Probe stage received");
    Ok(())
}
fn probe_observations(bytes: &[u8; 97]) -> Result<serde_json::Value> {
    if &bytes[..3] != b"PB\x01" || bytes[3] != 31 || bytes[9] > 22 {
        bail!("Invalid C64 probe observation record");
    }
    let ascii = |value: &[u8]| -> String {
        value
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .map(|b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '?'
                }
            })
            .collect()
    };
    let labels = ["identify", "open", "binary_write", "ack_read", "idle_read"];
    let rows: Vec<_> = labels.iter().enumerate().map(|(row,label)| {
        let entry=&bytes[32+row*13..32+(row+1)*13];
        serde_json::json!({"stage":label,"operation":entry[0],"status_prefix":ascii(&entry[1..9]),"count_or_handle":i16::from_le_bytes([entry[9],entry[10]]),"elapsed_jiffies":u16::from_le_bytes([entry[11],entry[12]])})
    }).collect();
    Ok(
        serde_json::json!({"build_version":ascii(&bytes[4..9]),"network_target_identity":ascii(&bytes[10..10+bytes[9] as usize]),"transactions":rows,"timing_unit":"KERNAL jiffies; hardware cadence not yet measured","partial_transfers":"each write/read row records the last relevant transaction count, not aggregate bytes"}),
    )
}
pub async fn hardware_probe(
    listener: TcpListener,
    network: &NetworkConfig,
    token: Token,
    device: &DeviceInfo,
) -> Result<serde_json::Value> {
    let (mut socket, peer) = tokio::time::timeout(Duration::from_secs(30), listener.accept())
        .await
        .context("No probe connection within 30 seconds; inspect the C64 display")??;
    if !matches!(peer.ip(),std::net::IpAddr::V4(ip) if network.allowed_ips.contains(&ip)) {
        bail!("Probe peer is outside C64 allowlist");
    }
    socket.set_nodelay(true)?;
    // Read exactly one HELLO so subsequent raw probe bytes remain in the socket.
    let mut header = [0; 8];
    probe_read(
        &mut socket,
        &mut header,
        "HELLO header",
        Duration::from_secs(5),
    )
    .await?;
    let length = u16::from_le_bytes([header[4], header[5]]) as usize;
    if length > 64 {
        bail!("Invalid probe HELLO");
    }
    let mut bytes = header.to_vec();
    bytes.resize(8 + length, 0);
    probe_read(
        &mut socket,
        &mut bytes[8..],
        "HELLO payload",
        Duration::from_secs(5),
    )
    .await?;
    let frames = Decoder::new().push(&bytes)?;
    let frame = frames.first().context("Incomplete probe HELLO")?;
    let Message::Hello {
        token: offered,
        capabilities,
        ..
    } = &frame.message
    else {
        bail!("Probe omitted HELLO");
    };
    if frame.sequence != 0 || !token.matches(offered) || capabilities & 2 == 0 {
        bail!("Probe authentication/capability failed");
    }
    write_frame(
        &mut socket,
        &mut Sequencer::new(),
        Message::HelloAck { enabled: false },
    )
    .await?;
    tracing::info!("Probe authenticated; HELLO_ACK sent without enabling input");
    let mut payload = [0; 32];
    probe_read(
        &mut socket,
        &mut payload,
        "binary payload",
        Duration::from_secs(10),
    )
    .await?;
    if &payload != PROBE_BYTES {
        bail!("Probe binary payload differs from expected bytes");
    }
    socket.write_all(b"ACK\0").await?;
    let mut confirmation = [0; 8];
    probe_read(
        &mut socket,
        &mut confirmation,
        "ACK confirmation",
        Duration::from_secs(5),
    )
    .await?;
    if &confirmation != b"READ_OK\n" {
        bail!("Probe ACK confirmation mismatch");
    }
    let idle_confirmed = matches!(
        probe_read(
            &mut socket,
            &mut confirmation,
            "idle confirmation",
            Duration::from_secs(5)
        )
        .await,
        Ok(())
    ) && &confirmation == b"IDLE_OK\n";
    let observations = if idle_confirmed {
        let mut bytes = [0; 97];
        probe_read(
            &mut socket,
            &mut bytes,
            "UCI observations",
            Duration::from_secs(5),
        )
        .await?;
        Some(probe_observations(&bytes)?)
    } else {
        None
    };
    tokio::time::sleep(Duration::from_secs(2)).await;
    socket.shutdown().await?;
    Ok(
        serde_json::json!({"device":device,"network_bidirectional":"verified","binary_payload":"verified","idle_read":if idle_confirmed{"verified"}else{"not verified"},"uci_observations":observations,"peer_close":"inspect C64 screen; not host-verified","verified_unix_seconds":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),"host_version":env!("CARGO_PKG_VERSION")}),
    )
}

#[cfg(test)]
mod tests;
