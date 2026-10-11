//! Bounded transport for local X11. x11rb's default socket is nonblocking,
//! but its default poll has no deadline; socket receive timeouts cannot fix it.
use bridge_core::BackendError;
use std::{
    cell::Cell,
    io::{self, IoSlice},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    time::{Duration, Instant},
};
use x11rb::{
    reexports::x11rb_protocol::{
        parse_display::{ConnectAddress, parse_display},
        xauth::get_auth,
    },
    rust_connection::{DefaultStream, PollMode, RustConnection, Stream},
    utils::RawFdContainer,
};

const IO_TIMEOUT: Duration = Duration::from_millis(500);

fn timeout() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "X11 operation exceeded 500 ms")
}

fn poll_until(fd: libc::c_int, mode: PollMode, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(timeout)?;
        let mut poll_fd = libc::pollfd {
            fd,
            events: if mode.readable() { libc::POLLIN } else { 0 }
                | if mode.writable() { libc::POLLOUT } else { 0 },
            revents: 0,
        };
        let milliseconds = remaining
            .as_nanos()
            .div_ceil(1_000_000)
            .min(i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(&mut poll_fd, 1, milliseconds) };
        if ready > 0 {
            // HUP/ERR are consumed by x11rb's subsequent nonblocking read/write.
            return Ok(());
        }
        if ready == 0 {
            // Recheck Instant so an early poll wakeup cannot report timeout
            // while a subsequent nonblocking read still sees a live deadline.
            continue;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[derive(Debug)]
pub(super) struct BoundedStream {
    inner: DefaultStream,
    deadline: Cell<Instant>,
}

impl BoundedStream {
    pub(super) fn begin_operation(&self) {
        self.deadline.set(Instant::now() + IO_TIMEOUT);
    }

    fn check_deadline(&self) -> io::Result<()> {
        if Instant::now() >= self.deadline.get() {
            Err(timeout())
        } else {
            Ok(())
        }
    }
}

impl Stream for BoundedStream {
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        poll_until(self.inner.as_raw_fd(), mode, self.deadline.get())
    }

    fn read(&self, buffer: &mut [u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.check_deadline()?;
        self.inner.read(buffer, fds)
    }

    fn write(&self, buffer: &[u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.check_deadline()?;
        self.inner.write(buffer, fds)
    }

    fn write_vectored(
        &self,
        buffers: &[IoSlice<'_>],
        fds: &mut Vec<RawFdContainer>,
    ) -> io::Result<usize> {
        self.check_deadline()?;
        self.inner.write_vectored(buffers, fds)
    }
}

fn unix_connect(path: &str, abstract_name: bool, deadline: Instant) -> io::Result<UnixStream> {
    if Instant::now() >= deadline {
        return Err(timeout());
    }
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() + 1 > address.sun_path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid local X11 socket path",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let start = usize::from(abstract_name);
    for (destination, source) in address.sun_path[start..].iter_mut().zip(bytes) {
        *destination = *source as libc::c_char;
    }
    let address_length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    let raw_fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if raw_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    if unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            address_length as libc::socklen_t,
        )
    } < 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            // Nonblocking AF_UNIX EAGAIN (a full backlog) is an immediate error,
            // not a connection-in-progress that can safely be waited on.
            return Err(error);
        }
        poll_until(fd.as_raw_fd(), PollMode::Writable, deadline)?;
        let mut socket_error: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&socket_error) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut socket_error as *mut libc::c_int).cast(),
                &mut length,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        if socket_error != 0 {
            return Err(io::Error::from_raw_os_error(socket_error));
        }
    }
    Ok(UnixStream::from(fd))
}

pub(super) fn connect() -> Result<RustConnection<BoundedStream>, BackendError> {
    let parsed = parse_display(None).map_err(|error| {
        BackendError::Unavailable(format!("Cannot parse local X11 display: {error}"))
    })?;
    let path = parsed.connect_instruction().find_map(|address| match address {
        ConnectAddress::Socket(path) => Some(path),
        _ => None,
    }).ok_or_else(|| BackendError::Unsupported("XTEST requires a local Unix-socket DISPLAY; remote/TCP and SSH-forwarded displays are unsupported".into()))?;
    let deadline = Instant::now() + IO_TIMEOUT;
    // Linux X servers commonly use the abstract namespace. Both attempts and
    // the setup handshake share one deadline and never use blocking connect.
    let socket = unix_connect(&path, true, deadline)
        .or_else(|_| unix_connect(&path, false, deadline))
        .map_err(|error| {
            BackendError::Unavailable(format!("Cannot connect to local X11 socket: {error}"))
        })?;
    let (inner, (family, address)) = DefaultStream::from_unix_stream(socket).map_err(|error| {
        BackendError::Unavailable(format!("Cannot initialize X11 transport: {error}"))
    })?;
    let (name, data) = get_auth(family, &address, parsed.display)
        .unwrap_or(None)
        .unwrap_or_default();
    RustConnection::connect_to_stream_with_auth_info(
        BoundedStream {
            inner,
            deadline: Cell::new(deadline),
        },
        usize::from(parsed.screen),
        name,
        data,
    )
    .map_err(|error| {
        BackendError::Unavailable(format!("Cannot authenticate local X11 display: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn socket_pair_poll_and_reads_share_deadline_without_x11_input() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let (inner, _) = DefaultStream::from_unix_stream(reader).unwrap();
        let stream = BoundedStream {
            inner,
            deadline: Cell::new(Instant::now() + Duration::from_millis(40)),
        };
        let started = Instant::now();
        assert_eq!(
            stream.poll(PollMode::Readable).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            stream.read(&mut [0], &mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        stream.begin_operation();
        writer.write_all(b"x").unwrap();
        stream.poll(PollMode::Readable).unwrap();
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte, &mut Vec::new()).unwrap(), 1);
        assert_eq!(byte, [b'x']);
        drop(writer);
        stream.poll(PollMode::Readable).unwrap();
        assert_eq!(stream.read(&mut byte, &mut Vec::new()).unwrap(), 0);
    }

    #[test]
    fn remote_display_has_no_supported_socket_transport() {
        let parsed = parse_display(Some("localhost:10.0")).unwrap();
        assert!(
            !parsed
                .connect_instruction()
                .any(|address| matches!(address, ConnectAddress::Socket(_)))
        );
    }

    #[test]
    fn setup_handshake_times_out_on_a_silent_socket_pair() {
        let (reader, _silent_peer) = UnixStream::pair().unwrap();
        let (inner, _) = DefaultStream::from_unix_stream(reader).unwrap();
        let stream = BoundedStream {
            inner,
            deadline: Cell::new(Instant::now() + Duration::from_millis(40)),
        };
        let started = Instant::now();
        let error = RustConnection::connect_to_stream(stream, 0).unwrap_err();
        assert!(error.to_string().contains("exceeded 500 ms"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn nonblocking_named_connect_uses_only_a_mock_unix_listener() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mock-x11.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let client =
            unix_connect(path.to_str().unwrap(), false, Instant::now() + IO_TIMEOUT).unwrap();
        let (_mock_peer, _) = listener.accept().unwrap();
        let flags = unsafe { libc::fcntl(client.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0 && flags & libc::O_NONBLOCK != 0);
        assert!(unix_connect(&"x".repeat(108), false, Instant::now() + IO_TIMEOUT).is_err());
    }
}
