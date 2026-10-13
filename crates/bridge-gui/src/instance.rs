use crate::config::{atomic_write, read_regular, validate_regular};
use anyhow::{Context, Result, bail};
use bridge_host::pairing::Token;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Discovery {
    port: u16,
    token: String,
}
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    command: String,
}

pub enum Instance {
    Primary(InstanceGuard),
    Existing,
}
pub struct InstanceGuard {
    _lock: File,
    discovery: PathBuf,
    listener: Option<TcpListener>,
    token: Token,
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.discovery);
    }
}
impl InstanceGuard {
    pub fn take_listener(&mut self) -> TcpListener {
        self.listener.take().expect("instance listener taken once")
    }
    pub fn token(&self) -> Token {
        self.token
    }
}

pub fn acquire(state_dir: &Path) -> Result<Instance> {
    bridge_host::pairing::prepare_directory(state_dir)?;
    let lock_path = state_dir.join("gui.lock");
    if lock_path.exists() {
        // Windows range locks prohibit reading the locked bytes; validate metadata only.
        validate_regular(&lock_path, true)?;
    } else if std::fs::symlink_metadata(&lock_path).is_ok() {
        bail!("GUI lock cannot be a symlink");
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            // A primary may still be creating its discovery record. Retry only this harmless Show.
            for _ in 0..20 {
                if show_existing(state_dir).is_ok() {
                    return Ok(Instance::Existing);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            bail!("Another GUI is running but cannot be contacted; its lock was preserved");
        }
        Err(error) => return Err(error.into()),
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("Randomness unavailable"))?;
    let token = Token(bytes);
    let discovery = state_dir.join("gui-runtime.json");
    atomic_write(
        &discovery,
        &serde_json::to_vec(&Discovery {
            port: listener.local_addr()?.port(),
            token: token.hex(),
        })?,
    )?;
    Ok(Instance::Primary(InstanceGuard {
        _lock: lock,
        discovery,
        listener: Some(listener),
        token,
    }))
}
fn show_existing(state_dir: &Path) -> Result<()> {
    let record: Discovery =
        serde_json::from_str(&read_regular(&state_dir.join("gui-runtime.json"), true)?)?;
    if record.port == 0 {
        bail!("Invalid GUI discovery record");
    }
    let _ = Token::from_hex(&record.token)?;
    let mut socket = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, record.port)),
        Duration::from_millis(500),
    )?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    socket.set_write_timeout(Some(Duration::from_millis(500)))?;
    let data = serde_json::to_vec(&Request {
        token: record.token,
        command: "show".into(),
    })?;
    socket.write_all(&(data.len() as u16).to_le_bytes())?;
    socket.write_all(&data)?;
    let mut size = [0; 2];
    socket.read_exact(&mut size)?;
    let size = u16::from_le_bytes(size) as usize;
    if size == 0 || size > 4096 {
        bail!("Invalid GUI response");
    }
    let mut reply = vec![0; size];
    socket.read_exact(&mut reply)?;
    if serde_json::from_slice::<serde_json::Value>(&reply)?["ok"] != true {
        bail!("GUI Show was rejected");
    }
    Ok(())
}
pub async fn serve(
    listener: TcpListener,
    token: Token,
    show: Arc<dyn Fn() + Send + Sync>,
    mut quit: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let listener =
        tokio::net::TcpListener::from_std(listener).context("Cannot listen for GUI activation")?;
    loop {
        tokio::select! {
            changed=quit.changed()=>{ if changed.is_err() || *quit.borrow() { return Ok(()); } },
            accepted=listener.accept()=>{
                let (mut socket,address)=accepted?; if !address.ip().is_loopback() {continue;}
                let request=tokio::time::timeout(Duration::from_secs(1),bridge_host::control::read_json::<Request>(&mut socket)).await;
                let Ok(Ok(request))=request else {continue;};
                if request.command!="show" || !Token::from_hex(&request.token).is_ok_and(|other|token.matches(&other.0)) {continue;}
                show();
                let _=tokio::time::timeout(Duration::from_secs(1),bridge_host::control::write_json(&mut socket,&serde_json::json!({"ok":true}))).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread")]
    async fn second_launch_restores_primary_without_replacing_its_lock_or_record() {
        let dir = std::env::temp_dir().join(format!(
            "c64-keyboard-bridge-gui-instance-{}",
            crate::config::random_id().unwrap()
        ));
        let Instance::Primary(mut guard) = acquire(&dir).unwrap() else {
            panic!()
        };
        let before = std::fs::read(dir.join("gui-runtime.json")).unwrap();
        let token = guard.token();
        let listener = guard.take_listener();
        let (show_tx, mut show_rx) = tokio::sync::mpsc::channel(1);
        let (quit_tx, quit_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(serve(
            listener,
            token,
            Arc::new(move || {
                let _ = show_tx.try_send(());
            }),
            quit_rx,
        ));
        let copy = dir.clone();
        assert!(matches!(
            tokio::task::spawn_blocking(move || acquire(&copy))
                .await
                .unwrap()
                .unwrap(),
            Instance::Existing
        ));
        tokio::time::timeout(Duration::from_secs(2), show_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(dir.join("gui-runtime.json")).unwrap(), before);
        quit_tx.send(true).unwrap();
        task.await.unwrap().unwrap();
        drop(guard);
        assert!(!dir.join("gui-runtime.json").exists());
        assert!(dir.join("gui.lock").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
