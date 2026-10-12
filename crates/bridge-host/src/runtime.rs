use crate::{
    backend,
    config::{Config, NetworkConfig},
    control::{self, ControlRequest, ControlResponse, RuntimeRecord, Status},
    pairing::{RuntimeGuard, Token},
    rest::DeviceInfo,
};
use anyhow::{Context, Result, bail};
use bridge_core::{HostPlatform, KeyboardBackend, KeyboardLayout, Mapper, MappingProfile};
use bridge_protocol::{Decoder, Frame, KeyEvent, MatrixState, Message, Sequencer};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
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
struct InputEvent {
    generation: u64,
    event: KeyEvent,
}
enum WorkerControl {
    Enable {
        enabled: bool,
        generation: u64,
        reply: oneshot::Sender<std::result::Result<(), String>>,
    },
    Reset {
        generation: u64,
    },
    Synchronize {
        generation: u64,
        matrix: MatrixState,
    },
    Shutdown,
}
#[derive(Clone)]
struct WorkerHandle {
    events: mpsc::SyncSender<InputEvent>,
    controls: mpsc::Sender<WorkerControl>,
    generation: Arc<AtomicU64>,
    control_order: Arc<Mutex<()>>,
}
impl WorkerHandle {
    async fn enable(&self, enabled: bool) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        {
            let _order = self.control_order.lock().unwrap();
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            self.controls
                .send(WorkerControl::Enable {
                    enabled,
                    generation,
                    reply: tx,
                })
                .context("Input worker stopped")?;
        }
        rx.await
            .context("Input worker stopped")?
            .map_err(anyhow::Error::msg)
    }
    fn reset(&self) {
        let _order = self.control_order.lock().unwrap();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.controls.send(WorkerControl::Reset { generation });
    }
    fn synchronize(&self, matrix: MatrixState) {
        let _order = self.control_order.lock().unwrap();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self
            .controls
            .send(WorkerControl::Synchronize { generation, matrix });
    }
    fn event(&self, event: KeyEvent) -> Result<()> {
        let _order = self.control_order.lock().unwrap();
        self.events
            .try_send(InputEvent {
                generation: self.generation.load(Ordering::SeqCst),
                event,
            })
            .map_err(|_| {
                anyhow::anyhow!("Input queue overflow; disconnecting to preserve key state")
            })
    }
}
fn spawn_worker(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
) -> Result<(WorkerHandle, InputThread)> {
    let selection = config.backend.clone();
    let keyboard_layout = layout(config);
    spawn_worker_with_factory(config, status, physical, move || {
        backend::create_backend(&selection, keyboard_layout)
    })
}

fn spawn_worker_with_factory(
    config: &Config,
    status: SharedStatus,
    physical: SharedMatrix,
    factory: impl Fn() -> std::result::Result<Box<dyn KeyboardBackend>, bridge_core::BackendError>
    + Send
    + 'static,
) -> Result<(WorkerHandle, InputThread)> {
    let profile = mapping_profile(config)?;
    let (events_tx, events_rx) = mpsc::sync_channel::<InputEvent>(256);
    let (controls_tx, controls_rx) = mpsc::channel::<WorkerControl>();
    let generation = Arc::new(AtomicU64::new(0));
    let selection = config.backend.clone();
    let delay = config.repeat_delay;
    let interval = config.repeat_interval;
    let thread=thread::Builder::new().name("c64bridge-input".into()).spawn(move || {
        let mut mapper=Mapper::new(profile,platform());
        let mut backend:Option<Box<dyn KeyboardBackend>>=None;
        let mut enabled=false;let mut epoch=0;let mut repeat_at:Option<Instant>=None;let mut pending:Option<InputEvent>=None;let mut layout_check_at=Instant::now();
        let release=|mapper:&mut Mapper,backend:&mut Option<Box<dyn KeyboardBackend>>| ->std::result::Result<(),String> {
            if let Some(backend)=backend.as_mut() {mapper.release_all(backend.as_mut()).map_err(|e|e.to_string())}else{Ok(())}
        };
        loop {
            while let Ok(control)=controls_rx.try_recv() {
                match control {
                    WorkerControl::Shutdown=>{
                        let cleanup=release(&mut mapper,&mut backend);
                        if let Err(error)=&cleanup {
                            tracing::error!(%error,"Shutdown cleanup failed; native keys may remain held");
                            let mut state=status.lock().unwrap();state.enabled=false;state.detail=format!("Shutdown cleanup failed; native keys may remain held: {error}");
                        }
                        return cleanup;
                    },
                    WorkerControl::Reset {generation}=>{
                        epoch=generation;repeat_at=None;
                        let cleanup=release(&mut mapper,&mut backend);
                        let mut state=status.lock().unwrap();
                        state.detail=if let Err(error)=cleanup{enabled=false;state.enabled=false;format!("Input disabled; native key release failed: {error}")}else{"C64 disconnected; all bridge-owned keys released; waiting for reconnect".into()};
                    },
                    WorkerControl::Synchronize {generation,matrix}=>{
                        epoch=generation;repeat_at=None;
                        let result=release(&mut mapper,&mut backend).and_then(|()| {
                            if enabled {mapper.synchronize(matrix,backend.as_mut().unwrap().as_mut()).map_err(|e|e.to_string())}else{Ok(())}
                        });
                        let mut state=status.lock().unwrap();
                        state.detail=if let Err(error)=result{enabled=false;state.enabled=false;format!("Input disabled after synchronization failure: {error}")}else if enabled{"C64 synchronized; input enabled; held ordinary keys require release before typing".into()}else{"C64 synchronized; input disabled; receiving physical transitions".into()};
                    },
                    WorkerControl::Enable {enabled:requested,generation,reply}=>{
                        epoch=generation;repeat_at=None;
                        let cleanup=release(&mut mapper,&mut backend);
                        enabled=false;status.lock().unwrap().enabled=false;
                        let result=if cleanup.is_err(){cleanup}else if requested {
                            if backend.is_none() {match factory() {Ok(b)=>backend=Some(b),Err(e)=>{let _=reply.send(Err(e.to_string()));status.lock().unwrap().detail=e.to_string();continue;}}}
                            mapper.synchronize(*physical.lock().unwrap(),backend.as_mut().unwrap().as_mut()).map_err(|e|e.to_string())
                        } else {Ok(())};
                        enabled=requested && result.is_ok();
                        {let mut state=status.lock().unwrap();state.enabled=enabled;state.backend=if backend.is_some(){selection.clone()}else{"disabled".into()};state.detail=if let Err(error)=&result{format!("Input disabled; cleanup/authorization failed: {error}")}else if enabled{"Input enabled; held ordinary keys require release before typing".into()}else{"Input paused; all bridge-owned keys released".into()};}
                        let _=reply.send(result);
                    }
                }
            }
            let input=if let Some(input)=pending.take(){Ok(input)}else{events_rx.recv_timeout(Duration::from_millis(5))};
            let queue_idle=matches!(&input,Err(mpsc::RecvTimeoutError::Timeout));
            match input {
                Ok(input) if input.generation>epoch=>{pending=Some(input);thread::sleep(Duration::from_millis(1));continue;},
                Ok(input) if enabled && input.generation==epoch=>{
                    let blocked_before=mapper.blocked_combinations();
                    let output=backend.as_mut().unwrap().as_mut();
                    let result=output.validate_layout().map_err(bridge_core::MappingError::Backend).and_then(|()|mapper.transition(input.event,output));
                    if let Err(error)=result {
                        // A mapping rejection must not leave native keys held. Core also guards this.
                        let cleanup=release(&mut mapper,&mut backend);enabled=false;repeat_at=None;
                        let mut state=status.lock().unwrap();state.enabled=false;state.detail=input_failure("mapping/backend failure",error,cleanup);
                    } else {
                        if input.event.pressed && !input.event.key.is_modifier() && mapper.is_key_admitted(input.event.key) {repeat_at=Some(Instant::now()+delay);}
                        if mapper.blocked_combinations()>blocked_before {let mut state=status.lock().unwrap();state.blocked_combinations=mapper.blocked_combinations();state.detail="Incompatible key combination blocked; release that physical key before retrying".into();}
                    }
                }
                Ok(_)=>{},
                Err(mpsc::RecvTimeoutError::Disconnected)=>{return release(&mut mapper,&mut backend);},
                Err(mpsc::RecvTimeoutError::Timeout)=>{},
            }
            if enabled && Instant::now()>=layout_check_at {
                layout_check_at=Instant::now()+Duration::from_millis(100);
                if let Err(error)=backend.as_mut().unwrap().validate_layout() {
                    let cleanup=release(&mut mapper,&mut backend);enabled=false;repeat_at=None;
                    let mut state=status.lock().unwrap();state.enabled=false;state.detail=input_failure("host layout change",error,cleanup);
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
                    let mut state=status.lock().unwrap();state.enabled=false;state.detail=input_failure("repeat failure",error,cleanup);
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
            control_order: Arc::new(Mutex::new(())),
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
pub async fn write_frame(
    socket: &mut TcpStream,
    sequence: &mut Sequencer,
    message: Message,
) -> Result<()> {
    let bytes = Frame {
        sequence: sequence.next_sequence(),
        message,
    }
    .encode()?;
    tokio::time::timeout(SESSION_TIMEOUT, socket.write_all(&bytes))
        .await
        .context("TCP write timed out")??;
    Ok(())
}

async fn session(
    mut socket: TcpStream,
    token: Token,
    worker: WorkerHandle,
    status: SharedStatus,
    physical: SharedMatrix,
    mut stopping: watch::Receiver<bool>,
) -> Result<()> {
    socket.set_nodelay(true)?;
    let mut reader = FrameReader::default();
    let mut incoming = Sequencer::new();
    let mut outgoing = Sequencer::new();
    let hello = tokio::select! {
        _=stopping.changed()=>return Ok(()),
        hello=tokio::time::timeout(Duration::from_secs(5),reader.next(&mut socket))=>hello.context("HELLO timed out")??,
    };
    incoming.check(hello.sequence)?;
    let Message::Hello {
        token: offered,
        device_id,
        capabilities,
    } = hello.message
    else {
        bail!("HELLO required before keyboard events");
    };
    if !token.matches(&offered) {
        bail!("Client authentication failed");
    }
    if capabilities & 1 == 0 {
        bail!("Client does not advertise keyboard capability");
    }
    {
        let mut state = status.lock().unwrap();
        state.connected = true;
        state.device = Some(device_id);
        state.detail = "Authenticated C64; waiting for state synchronization".into();
    }
    let enabled = status.lock().unwrap().enabled;
    write_frame(&mut socket, &mut outgoing, Message::HelloAck { enabled }).await?;
    let first = tokio::select! {
        _=stopping.changed()=>return Ok(()),
        first=tokio::time::timeout(SESSION_TIMEOUT,reader.next(&mut socket))=>first.context("Initial state synchronization timed out")??,
    };
    incoming.check(first.sequence)?;
    let Message::StateSync(matrix) = first.message else {
        bail!("Initial STATE_SYNC required");
    };
    *physical.lock().unwrap() = matrix;
    worker.synchronize(matrix);
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_received = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _=tokio::time::sleep_until(last_received+SESSION_TIMEOUT)=>bail!("C64 heartbeat timed out"),
            changed=stopping.changed()=>{
                if changed.is_err() || *stopping.borrow() {
                    let _=write_frame(&mut socket,&mut outgoing,Message::Disconnect {reason:0}).await;
                    return Ok(());
                }
            }
            _=heartbeat.tick()=>{
                if last_received.elapsed()>=SESSION_TIMEOUT {bail!("C64 heartbeat timed out");}
                let enabled=status.lock().unwrap().enabled;
                write_frame(&mut socket,&mut outgoing,Message::HostHeartbeat {enabled}).await?;
            }
            frame=reader.next(&mut socket)=>{
                let frame=frame?;incoming.check(frame.sequence)?;last_received=tokio::time::Instant::now();
                match frame.message {
                    message @ (Message::KeyDown {..}|Message::KeyUp {..})=>{
                        let event=message.key_event().expect("key message");
                        {
                            let mut matrix=physical.lock().unwrap();
                            if matrix.get(event.key)==event.pressed {bail!("Duplicate physical transition");}
                            matrix.set(event.key,event.pressed);
                            if matrix.modifiers()!=event.modifiers {bail!("Modifier mask disagrees with physical state");}
                        }
                        status.lock().unwrap().events+=1;
                        tracing::debug!(key=?event.key,pressed=event.pressed,modifiers=event.modifiers,"Physical C64 transition");
                        worker.event(event)?;
                    }
                    Message::Heartbeat(snapshot)=>{if snapshot != *physical.lock().unwrap(){bail!("Heartbeat state disagrees with ordered transitions");}}
                    Message::StateSync(snapshot)=>{
                        if snapshot != *physical.lock().unwrap(){bail!("State synchronization disagrees with ordered transitions");}
                        worker.synchronize(snapshot);
                    },
                    Message::Disconnect {..}=>return Ok(()),
                    Message::Error {code,..}=>{if code!=0 {bail!("C64 reported client error code {code}");}},
                    _=>bail!("Unexpected message after session initialization"),
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

pub async fn start(config: Config, options: StartOptions) -> Result<()> {
    let network = config.network().await?;
    let token = crate::pairing::load_or_create_token(&config.state_dir)?;
    let listener = TcpListener::bind(network.bind)
        .await
        .context("Cannot bind bridge TCP listener")?;
    let control_listener = TcpListener::bind("127.0.0.1:0").await?;
    let _guard = RuntimeGuard::create(
        &config.state_dir,
        &RuntimeRecord {
            pid: std::process::id(),
            port: control_listener.local_addr()?.port(),
        },
    )?;
    let status = Arc::new(Mutex::new(Status::default()));
    let physical = Arc::new(Mutex::new(MatrixState::default()));
    let (worker, thread) = spawn_worker(&config, status.clone(), physical.clone())?;
    if options.enable_input
        && let Err(error) = worker.enable(true).await
    {
        worker.controls.send(WorkerControl::Shutdown)?;
        return finish_worker(thread, Err(error));
    }
    let (stopping_tx, stopping_rx) = watch::channel(false);
    let control_task = tokio::spawn(serve_control(
        control_listener,
        token,
        worker.clone(),
        status.clone(),
        stopping_tx.clone(),
    ));
    let setup = async {
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
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let mut stop = stopping_rx.clone();
    let setup = tokio::select! {
        result=setup=>result.map(|()|true),
        _=stop.changed()=>Ok(false),
        result=&mut shutdown=>result.map(|()|false),
    };
    if !matches!(setup, Ok(true)) {
        let _ = stopping_tx.send(true);
        control_task.abort();
        let _ = worker.controls.send(WorkerControl::Shutdown);
        return finish_worker(thread, setup.map(|_| ()));
    }
    tracing::info!(address=%listener.local_addr()?,advertise=%network.advertise_ip,input_enabled=options.enable_input,"Bridge ready");
    let mut active: Option<tokio::task::JoinHandle<()>> = None;
    let result = loop {
        tokio::select! {
            signal=&mut shutdown=>{break signal;}
            _=stop.changed()=>{if *stop.borrow(){break Ok(());}}
            accepted=listener.accept()=>{
                let (socket,address)=match accepted {Ok(pair)=>pair,Err(error)=>break Err(error.into())};
                if !matches!(address.ip(),std::net::IpAddr::V4(ip) if network.allowed_ips.contains(&ip)) {tracing::warn!("Rejected client outside the C64 IP allowlist");continue;}
                if active.as_ref().is_some_and(|task|!task.is_finished()) {tracing::warn!("Rejected second client while a session is active");continue;}
                if let Some(task)=active.take(){let _=task.await;}
                let worker=worker.clone();let state=status.clone();let matrix=physical.clone();let stopping=stopping_rx.clone();
                active=Some(tokio::spawn(async move {
                    let result=session(socket,token,worker.clone(),state.clone(),matrix.clone(),stopping).await;
                    *matrix.lock().unwrap()=MatrixState::default();
                    let mut status=state.lock().unwrap();status.connected=false;status.device=None;
                    // The worker confirms cleanup asynchronously and retains any release failure.
                    status.detail="C64 session ended; releasing bridge-owned keys".into();
                    drop(status);worker.reset();
                    if let Err(error)=result{tracing::warn!(%error,"C64 session ended after a network or protocol failure");}
                }));
            }
        }
    };
    let _ = stopping_tx.send(true);
    if let Some(mut task) = active
        && tokio::time::timeout(Duration::from_secs(4), &mut task)
            .await
            .is_err()
    {
        task.abort();
        let _ = task.await;
    }
    control_task.abort();
    let _ = worker.controls.send(WorkerControl::Shutdown);
    drop(worker);
    finish_worker(thread, result)
}
async fn serve_control(
    listener: TcpListener,
    token: Token,
    worker: WorkerHandle,
    status: SharedStatus,
    stop: watch::Sender<bool>,
) -> Result<()> {
    loop {
        let (mut socket, _) = listener.accept().await?;
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
            "stop" => worker.enable(false).await,
            _ => Err(anyhow::anyhow!("Unknown control command")),
        };
        let response = ControlResponse {
            ok: result.is_ok(),
            status: status.lock().unwrap().clone(),
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
