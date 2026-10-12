use crate::{
    config::Config,
    pairing::read_token,
    runtime::{FrameReader, write_frame},
};
use anyhow::{Context, Result, bail};
use bridge_protocol::{MatrixState, Message, PhysicalKey, Sequencer};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DISCONNECT_BYTES: usize = 4096;

async fn finish_typing(socket: &mut tokio::net::TcpStream) -> Result<()> {
    tokio::time::timeout(DISCONNECT_TIMEOUT, async {
        // Keep receiving pending host heartbeats after the complete DISCONNECT.
        // Dropping a socket with unread inbound data can reset the connection.
        socket
            .shutdown()
            .await
            .context("Simulator could not half-close after DISCONNECT")?;
        let mut buffer = [0u8; 256];
        let mut received = 0;
        loop {
            // Read one byte beyond the remaining budget only to detect overflow;
            // exactly MAX_DISCONNECT_BYTES followed by EOF is still graceful.
            let capacity = buffer.len().min(MAX_DISCONNECT_BYTES - received + 1);
            let count = socket
                .read(&mut buffer[..capacity])
                .await
                .context("Simulator peer failed during graceful disconnect")?;
            if count == 0 {
                return Ok(());
            }
            received += count;
            if received > MAX_DISCONNECT_BYTES {
                bail!("Simulator peer exceeded the graceful-disconnect byte limit");
            }
        }
    })
    .await
    .context("Simulator graceful disconnect timed out waiting for peer EOF")?
}

pub async fn run(config: &Config, scenario: &str, fragment: bool) -> Result<()> {
    let network = config.network().await?;
    if !network.advertise_ip.is_loopback() || config.backend != "recording" {
        bail!(
            "Simulator requires a loopback host and INPUT_BACKEND=recording; start --fake-device --backend recording"
        );
    }
    let token = read_token(&config.state_dir)?;
    let mut socket = tokio::net::TcpStream::connect((network.advertise_ip, config.bridge_port))
        .await
        .context("Start the recording bridge before the simulator")?;
    let mut sequence = Sequencer::new();
    let mut reader = FrameReader::default();
    write_frame(
        &mut socket,
        &mut sequence,
        Message::Hello {
            token: token.0,
            capabilities: 1,
            device_id: "FAKE-C64".into(),
        },
    )
    .await?;
    let ack = reader.next(&mut socket).await?;
    if !matches!(ack.message, Message::HelloAck { .. }) {
        bail!("Host did not acknowledge simulator");
    }
    write_frame(
        &mut socket,
        &mut sequence,
        Message::StateSync(MatrixState::default()),
    )
    .await?;
    if scenario == "malformed" {
        socket.write_all(b"XX\x01\x03\x02\0\x02\0\x0a\0").await?;
        return Ok(());
    }
    let mut matrix = MatrixState::default();
    let events = match scenario {
        "typing" => vec![
            (PhysicalKey::C, true),
            (PhysicalKey::C, false),
            (PhysicalKey::Num6, true),
            (PhysicalKey::Num6, false),
            (PhysicalKey::Num4, true),
            (PhysicalKey::Num4, false),
        ],
        "disconnect-held" | "heartbeat-timeout" => {
            vec![(PhysicalKey::Control, true), (PhysicalKey::A, true)]
        }
        _ => bail!("Unknown simulator scenario"),
    };
    for (key, pressed) in events {
        matrix.set(key, pressed);
        let modifiers = matrix.modifiers();
        let message = if pressed {
            Message::KeyDown { key, modifiers }
        } else {
            Message::KeyUp { key, modifiers }
        };
        if fragment {
            let frame = bridge_protocol::Frame {
                sequence: sequence.next_sequence(),
                message,
            }
            .encode()?;
            for byte in frame {
                socket.write_all(&[byte]).await?;
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        } else {
            write_frame(&mut socket, &mut sequence, message).await?;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if scenario == "heartbeat-timeout" {
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
    if scenario == "typing" {
        write_frame(&mut socket, &mut sequence, Message::Heartbeat(matrix)).await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        write_frame(
            &mut socket,
            &mut sequence,
            Message::Disconnect { reason: 0 },
        )
        .await?;
        finish_typing(&mut socket).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Overrides, pairing};
    use std::{collections::BTreeMap, net::Ipv4Addr};
    use tokio::{net::TcpListener, sync::oneshot};

    async fn fixture() -> (tempfile::TempDir, Config, TcpListener, pairing::Token) {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let values = BTreeMap::from([
            ("C64_HOST".into(), "127.0.0.1".into()),
            ("HOST_ADVERTISE_IP".into(), "127.0.0.1".into()),
            ("INPUT_BACKEND".into(), "recording".into()),
            ("AUTO_CONNECT".into(), "false".into()),
            (
                "BRIDGE_STATE_DIR".into(),
                directory.path().join("state").to_str().unwrap().into(),
            ),
            (
                "BRIDGE_PORT".into(),
                listener.local_addr().unwrap().port().to_string(),
            ),
        ]);
        let config = Config::from_values(values, BTreeMap::new(), Overrides::default()).unwrap();
        let token = pairing::load_or_create_token(&config.state_dir).unwrap();
        (directory, config, listener, token)
    }

    async fn receive_typing(socket: &mut tokio::net::TcpStream, token: pairing::Token) {
        tokio::time::timeout(Duration::from_secs(4), async {
            let mut reader = FrameReader::default();
            let mut outgoing = Sequencer::new();
            let hello = reader.next(socket).await.unwrap();
            assert_eq!(hello.sequence, 0);
            let Message::Hello {
                token: received,
                capabilities,
                device_id,
            } = hello.message
            else {
                panic!("Simulator did not authenticate before typing")
            };
            assert!(token.matches(&received));
            assert_eq!(capabilities, 1);
            assert_eq!(device_id, "FAKE-C64");
            write_frame(socket, &mut outgoing, Message::HelloAck { enabled: true })
                .await
                .unwrap();
            let expected = [
                Message::StateSync(MatrixState::default()),
                Message::KeyDown {
                    key: PhysicalKey::C,
                    modifiers: 0,
                },
                Message::KeyUp {
                    key: PhysicalKey::C,
                    modifiers: 0,
                },
                Message::KeyDown {
                    key: PhysicalKey::Num6,
                    modifiers: 0,
                },
                Message::KeyUp {
                    key: PhysicalKey::Num6,
                    modifiers: 0,
                },
                Message::KeyDown {
                    key: PhysicalKey::Num4,
                    modifiers: 0,
                },
                Message::KeyUp {
                    key: PhysicalKey::Num4,
                    modifiers: 0,
                },
                Message::Heartbeat(MatrixState::default()),
                Message::Disconnect { reason: 0 },
            ];
            for (index, message) in expected.into_iter().enumerate() {
                let frame = reader.next(socket).await.unwrap();
                assert_eq!(frame.sequence, index as u16 + 1);
                assert_eq!(frame.message, message);
                if index == 0 {
                    // Deliberately leave host data pending while client frames
                    // continue, as a periodic host heartbeat can do in practice.
                    write_frame(
                        socket,
                        &mut outgoing,
                        Message::HostHeartbeat { enabled: true },
                    )
                    .await
                    .unwrap();
                }
            }
        })
        .await
        .expect("Simulator did not deliver its complete DISCONNECT");
    }

    async fn receive_sending_eof(socket: &mut tokio::net::TcpStream) {
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
                .await
                .expect("Simulator did not half-close its sending side")
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn typing_waits_for_peer_eof_with_a_pending_host_heartbeat() {
        let (_directory, config, listener, token) = fixture().await;
        let (disconnect_tx, disconnect_rx) = oneshot::channel();
        let (close_tx, close_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            receive_typing(&mut socket, token).await;
            disconnect_tx.send(()).unwrap();
            receive_sending_eof(&mut socket).await;
            close_rx.await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let simulator = tokio::spawn(async move { run(&config, "typing", true).await });
        tokio::time::timeout(Duration::from_secs(5), disconnect_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !simulator.is_finished(),
            "Simulator returned before its peer closed gracefully"
        );
        close_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), simulator)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn typing_fails_within_its_deadline_when_peer_does_not_close() {
        let (_directory, config, listener, token) = fixture().await;
        let (eof_tx, eof_rx) = oneshot::channel();
        let (close_tx, close_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            receive_typing(&mut socket, token).await;
            receive_sending_eof(&mut socket).await;
            eof_tx.send(()).unwrap();
            close_rx.await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let simulator = tokio::spawn(async move { run(&config, "typing", false).await });
        tokio::time::timeout(Duration::from_secs(5), eof_rx)
            .await
            .unwrap()
            .unwrap();
        let error = tokio::time::timeout(DISCONNECT_TIMEOUT + Duration::from_secs(1), simulator)
            .await
            .expect("Simulator failed to bound its wait for peer closure")
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("timed out waiting for peer EOF"));
        close_tx.send(()).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn typing_bounds_peer_data_while_waiting_for_eof() {
        let (_directory, config, listener, token) = fixture().await;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            receive_typing(&mut socket, token).await;
            receive_sending_eof(&mut socket).await;
            socket
                .write_all(&vec![0; MAX_DISCONNECT_BYTES + 1])
                .await
                .unwrap();
            // Rejecting oversized data may reset the deliberately bad peer.
            let _ = socket.shutdown().await;
        });
        let error = tokio::time::timeout(Duration::from_secs(5), run(&config, "typing", false))
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("byte limit"));
        server.await.unwrap();
    }
}
