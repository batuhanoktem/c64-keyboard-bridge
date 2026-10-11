use crate::{
    config::Config,
    pairing::read_token,
    runtime::{FrameReader, write_frame},
};
use anyhow::{Context, Result, bail};
use bridge_protocol::{MatrixState, Message, PhysicalKey, Sequencer};
use std::time::Duration;

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
        use tokio::io::AsyncWriteExt;
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
            use tokio::io::AsyncWriteExt;
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
    }
    Ok(())
}
