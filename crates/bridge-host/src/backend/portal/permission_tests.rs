//! Actual private D-Bus portal conversations. There is no desktop portal, user
//! screen, native keyboard, credential store or network device in these tests.
use super::*;
use std::{
    io::Read,
    os::fd::{AsRawFd, OwnedFd as StdFd},
    process::{Child, Command, Stdio},
};
use zbus::zvariant::{OwnedFd as BusFd, OwnedObjectPath, OwnedValue};

struct Daemon {
    process: Child,
    _directory: tempfile::TempDir,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
fn private_bus() -> (Daemon, String) {
    let directory = tempfile::tempdir().unwrap();
    let address = format!("unix:path={}/bus", directory.path().display());
    let mut child = Daemon {
        process: Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!("--address={address}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Linux fixture requires dbus-daemon"),
        _directory: directory,
    };
    let mut output = child.process.stdout.take().unwrap();
    let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert!(
        unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    loop {
        let mut data = [0; 1024];
        match output.read(&mut data) {
            Ok(0) => {}
            Ok(count) => bytes.extend_from_slice(&data[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => panic!("Private bus startup failed"),
        }
        if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            let address = String::from_utf8(bytes[..end].to_vec()).unwrap();
            assert!(address.starts_with("unix:"));
            return (child, address);
        }
        assert!(
            bytes.len() <= 4096 && std::time::Instant::now() < deadline,
            "Private bus startup deadline exceeded"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Grant,
    Deny,
    Pending,
}
struct Evidence {
    calls: Mutex<Vec<&'static str>>,
    closed: std::sync::atomic::AtomicUsize,
    started: tokio::sync::Notify,
    peer: Mutex<Option<UnixStream>>,
}
struct MockPortal {
    mode: Mode,
    evidence: Arc<Evidence>,
    version: u32,
}
struct MockSession(Arc<Evidence>);

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl MockSession {
    fn close(&self) {
        self.0
            .closed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}
type Options = HashMap<String, OwnedValue>;
fn path(
    header: &zbus::message::Header<'_>,
    options: &Options,
    token: &str,
    kind: &str,
) -> OwnedObjectPath {
    let sender = header
        .sender()
        .unwrap()
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let token = <&str>::try_from(options.get(token).unwrap()).unwrap();
    format!("/org/freedesktop/portal/desktop/{kind}/{sender}/{token}")
        .try_into()
        .unwrap()
}
async fn respond(
    connection: &zbus::Connection,
    request: &OwnedObjectPath,
    code: u32,
    values: Options,
) -> zbus::fdo::Result<()> {
    connection
        .emit_signal(
            None::<&str>,
            request,
            "org.freedesktop.portal.Request",
            "Response",
            &(code, values),
        )
        .await
        .map_err(|_| zbus::fdo::Error::Failed("Private fixture response failed".into()))
}
#[zbus::interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl MockPortal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        self.version
    }
    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        1
    }

    async fn create_session(
        &self,
        options: Options,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.evidence.calls.lock().unwrap().push("create");
        let request = path(&header, &options, "handle_token", "request");
        let session = path(&header, &options, "session_handle_token", "session");
        connection
            .object_server()
            .at(&session, MockSession(self.evidence.clone()))
            .await
            .map_err(|_| zbus::fdo::Error::Failed("Private fixture session failed".into()))?;
        respond(
            connection,
            &request,
            0,
            HashMap::from([(
                "session_handle".into(),
                zbus::zvariant::ObjectPath::from(session).into(),
            )]),
        )
        .await?;
        Ok(request)
    }
    async fn select_devices(
        &self,
        _session: OwnedObjectPath,
        options: Options,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.evidence
            .calls
            .lock()
            .unwrap()
            .push("keyboard_only_no_persistence");
        assert_eq!(u32::try_from(options.get("types").unwrap()).unwrap(), 1);
        assert_eq!(
            u32::try_from(options.get("persist_mode").unwrap()).unwrap(),
            0
        );
        assert!(!options.contains_key("restore_token"));
        let request = path(&header, &options, "handle_token", "request");
        respond(connection, &request, 0, HashMap::new()).await?;
        Ok(request)
    }
    async fn start(
        &self,
        _session: OwnedObjectPath,
        _parent_window: String,
        options: Options,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.evidence.calls.lock().unwrap().push("start");
        let request = path(&header, &options, "handle_token", "request");
        match self.mode {
            Mode::Grant => {
                respond(
                    connection,
                    &request,
                    0,
                    HashMap::from([("devices".into(), 1_u32.into())]),
                )
                .await?
            }
            Mode::Deny => respond(connection, &request, 1, HashMap::new()).await?,
            Mode::Pending => {}
        }
        self.evidence.started.notify_one();
        Ok(request)
    }
    #[zbus(name = "ConnectToEIS")]
    async fn connect_to_eis(
        &self,
        _session: OwnedObjectPath,
        _options: Options,
    ) -> zbus::fdo::Result<BusFd> {
        self.evidence.calls.lock().unwrap().push("eis");
        let (client, peer) = UnixStream::pair().unwrap();
        *self.evidence.peer.lock().unwrap() = Some(peer);
        Ok(BusFd::from(StdFd::from(client)))
    }
}

struct Fixture {
    _daemon: Daemon,
    _server: zbus::Connection,
    client: zbus::Connection,
    evidence: Arc<Evidence>,
}
impl Fixture {
    async fn new(mode: Mode, version: u32) -> Self {
        let (daemon, address) = private_bus();
        let evidence = Arc::new(Evidence {
            calls: Mutex::new(Vec::new()),
            closed: std::sync::atomic::AtomicUsize::new(0),
            started: tokio::sync::Notify::new(),
            peer: Mutex::new(None),
        });
        let server = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.freedesktop.portal.Desktop")
            .unwrap()
            .serve_at(
                "/org/freedesktop/portal/desktop",
                MockPortal {
                    mode,
                    evidence: evidence.clone(),
                    version,
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let client = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        Self {
            _daemon: daemon,
            _server: server,
            client,
            evidence,
        }
    }
}

#[tokio::test]
async fn permission_grant_requests_keyboard_only_and_closes_owned_session() {
    let fixture = Fixture::new(Mode::Grant, 2).await;
    let (_cancel, mut cancellation) = watch::channel(false);
    let (mut owner, _stream) = authorize_on_bus(&mut cancellation, fixture.client.clone())
        .await
        .unwrap();
    assert_eq!(
        *fixture.evidence.calls.lock().unwrap(),
        ["create", "keyboard_only_no_persistence", "start", "eis"]
    );
    assert!(owner.close().await);
    assert_eq!(
        fixture
            .evidence
            .closed
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn denied_permission_closes_without_opening_eis() {
    let fixture = Fixture::new(Mode::Deny, 2).await;
    let (_cancel, mut cancellation) = watch::channel(false);
    assert!(matches!(
        authorize_on_bus(&mut cancellation, fixture.client.clone()).await,
        Err(Failure::Consent)
    ));
    assert_eq!(
        *fixture.evidence.calls.lock().unwrap(),
        ["create", "keyboard_only_no_persistence", "start"]
    );
    assert_eq!(
        fixture
            .evidence
            .closed
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn pending_permission_cancellation_closes_dialog_session_and_bus() {
    let fixture = Fixture::new(Mode::Pending, 2).await;
    let (cancel, mut cancellation) = watch::channel(false);
    let grant = authorize_on_bus(&mut cancellation, fixture.client.clone());
    let interruption = async {
        fixture.evidence.started.notified().await;
        cancel.send(true).unwrap();
    };
    let (result, ()) = timeout(Duration::from_secs(3), async {
        tokio::join!(grant, interruption)
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(Failure::Cancelled)));
    assert_eq!(
        fixture
            .evidence
            .closed
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        *fixture.evidence.calls.lock().unwrap(),
        ["create", "keyboard_only_no_persistence", "start"]
    );
    assert!(fixture.client.is_closed());
}

#[tokio::test]
async fn readonly_version_probe_never_creates_permission_session() {
    let fixture = Fixture::new(Mode::Grant, 1).await;
    let portal = RemoteDesktop::with_connection(fixture.client.clone())
        .await
        .unwrap();
    assert_eq!(check_portal(&portal).await, Err(Failure::Unsupported));
    assert!(fixture.evidence.calls.lock().unwrap().is_empty());
}
