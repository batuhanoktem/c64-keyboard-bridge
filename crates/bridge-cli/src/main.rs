use anyhow::{Context, Result, bail};
use bridge_host::{
    backend,
    config::{Config, Overrides},
    control, diagnostics, operations, pairing,
    rest::RestClient,
    runtime, simulator,
};
use clap::{Args, Parser, Subcommand};
use std::{net::Ipv4Addr, path::PathBuf, time::Duration};

const KEYBOARD_PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/KEYBRIDGE.PRG"));
const PROBE_PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/NETWORKTEST.PRG"));

#[derive(Parser)]
#[command(
    name = "c64bridge",
    version,
    about = "Use the physical C64 Ultimate keyboard on a modern computer"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".env")]
    env_file: PathBuf,
    #[arg(long, global = true)]
    device_host: Option<String>,
    #[arg(long, global = true)]
    bind_address: Option<Ipv4Addr>,
    #[arg(long, global = true)]
    advertise_ip: Option<Ipv4Addr>,
    #[arg(long, global = true)]
    bridge_port: Option<u16>,
    #[arg(long, global = true)]
    layout: Option<String>,
    #[arg(long, global = true)]
    profile: Option<String>,
    #[arg(long, global = true)]
    backend: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Start(Start),
    Stop,
    Status,
    Pause,
    Resume,
    Doctor {
        #[arg(long)]
        json: bool,
    },
    #[command(subcommand)]
    Device(Device),
    #[command(subcommand)]
    Mappings(Mappings),
    #[command(subcommand)]
    Pair(Pair),
    /// Deliberately synthesize a short phrase after focusing a scratch application.
    TestInput {
        #[arg(long,default_value_t=5,value_parser=clap::value_parser!(u64).range(3..=30))]
        countdown: u64,
    },
    /// Request native input authorization; doctor never opens a permission prompt.
    Permissions {
        #[arg(long)]
        request: bool,
    },
    /// Simulate a C64 against a loopback recording bridge; never sends native input.
    Simulate {
        #[arg(long,default_value="typing",value_parser=["typing","disconnect-held","heartbeat-timeout","malformed"])]
        scenario: String,
        #[arg(long)]
        fragment: bool,
    },
}
#[derive(Args)]
struct Start {
    #[arg(long)]
    enable_input: bool,
    #[arg(long)]
    deploy: bool,
    #[arg(long)]
    allow_reset: bool,
    /// Bind loopback and accept a simulator instead of contacting hardware.
    #[arg(long)]
    fake_device: bool,
}
#[derive(Subcommand)]
enum Device {
    Info,
    Deploy {
        #[arg(long)]
        allow_reset: bool,
    },
    Run {
        #[arg(long)]
        allow_reset: bool,
    },
    Test {
        #[arg(long)]
        allow_reset: bool,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
}
#[derive(Subcommand)]
enum Mappings {
    List,
    Validate { path: Option<PathBuf> },
}
#[derive(Subcommand)]
enum Pair {
    Rotate,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let cli = Cli::parse();
    let fake = matches!(
        &cli.command,
        Command::Start(Start {
            fake_device: true,
            ..
        })
    );
    let overrides = Overrides {
        device_host: if fake {
            Some("127.0.0.1".into())
        } else {
            cli.device_host
        },
        bind_address: if fake {
            Some(Ipv4Addr::LOCALHOST)
        } else {
            cli.bind_address
        },
        advertise_ip: if fake {
            Some(Ipv4Addr::LOCALHOST)
        } else {
            cli.advertise_ip
        },
        bridge_port: cli.bridge_port,
        layout: cli.layout,
        profile: cli.profile,
        backend: if fake {
            Some("recording".into())
        } else {
            cli.backend
        },
    };
    let mut config = Config::load(&cli.env_file, overrides)?;
    if fake {
        config.allowed_ips = vec![Ipv4Addr::LOCALHOST];
        config.auto_deploy = false;
        config.auto_connect = false;
    }
    let filter =
        tracing_subscriber::EnvFilter::try_new(&config.log_level).context("Invalid LOG_LEVEL")?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    match cli.command {
        Command::Start(args) => {
            runtime::start(
                config,
                runtime::StartOptions {
                    enable_input: args.enable_input,
                    deploy: args.deploy,
                    allow_reset: args.allow_reset,
                    template: KEYBOARD_PRG.to_vec(),
                    fake_device: args.fake_device,
                },
            )
            .await?
        }
        Command::Stop | Command::Status | Command::Pause | Command::Resume => {
            let name = match cli.command {
                Command::Stop => "stop",
                Command::Status => "status",
                Command::Pause => "pause",
                _ => "resume",
            };
            let response = control::send(&config.state_dir, name).await?;
            println!("{}", serde_json::to_string_pretty(&response.status)?);
            if !response.ok {
                bail!(
                    "{}",
                    response
                        .error
                        .unwrap_or_else(|| "Control operation failed".into())
                );
            }
        }
        Command::Doctor { json } | Command::Device(Device::Doctor { json }) => {
            let report = diagnostics::doctor(&config).await;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                report.print();
            }
        }
        Command::Device(Device::Info) => {
            let info = RestClient::new(&config)?.info().await?;
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        Command::Device(Device::Deploy { allow_reset }) => {
            deploy(&config, KEYBOARD_PRG, false, allow_reset).await?
        }
        Command::Device(Device::Run { allow_reset }) => {
            deploy(&config, KEYBOARD_PRG, true, allow_reset).await?
        }
        Command::Device(Device::Test { allow_reset }) => device_test(&config, allow_reset).await?,
        Command::Mappings(Mappings::List) => {
            println!("modern-us\nmodern-uk\nauthentic-c64 (US or UK)");
        }
        Command::Mappings(Mappings::Validate { path }) => {
            let profile = if let Some(path) = path {
                bridge_core::MappingProfile::from_toml(&std::fs::read_to_string(path)?)?
            } else {
                runtime::mapping_profile(&config)?
            };
            println!(
                "Valid mapping profile: {} ({:?})",
                profile.name, profile.layout
            );
        }
        Command::Pair(Pair::Rotate) => {
            pairing::rotate_token(&config.state_dir)?;
            println!("Pairing token rotated. Redeploy the C64 client before connecting.");
        }
        Command::Permissions { request } => {
            if request {
                backend::request_permission()?;
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&backend::probe_backend(
                    &config.backend,
                    runtime::layout(&config)
                ))?
            );
        }
        Command::TestInput { countdown } => test_input(&config, countdown).await?,
        Command::Simulate { scenario, fragment } => {
            simulator::run(&config, &scenario, fragment).await?;
            println!("Simulator scenario completed; inspect bridge status for results.");
        }
    }
    Ok(())
}
async fn deploy(config: &Config, template: &[u8], run: bool, allow_reset: bool) -> Result<()> {
    if !allow_reset {
        bail!(
            "PRG deployment resets the C64 and interrupts its software; add --allow-reset explicitly"
        );
    }
    eprintln!(
        "Warning: {} this PRG resets the C64 and interrupts its running software.",
        if run { "Running" } else { "Loading" }
    );
    operations::deploy(config, template, run, allow_reset).await?;
    println!(
        "PRG {} successfully; networking and physical input still require observed tests.",
        if run {
            "run request completed"
        } else {
            "loaded without running"
        }
    );
    Ok(())
}
async fn device_test(config: &Config, allow_reset: bool) -> Result<()> {
    if !allow_reset {
        bail!(
            "The hardware probe resets and replaces the running C64 program; add --allow-reset explicitly"
        );
    }
    eprintln!(
        "Warning: deploying the hardware probe resets the C64. Inspect its display for UCI statuses and peer-close results."
    );
    let evidence = operations::probe(config, PROBE_PRG, allow_reset).await?;
    println!("{}", serde_json::to_string_pretty(&evidence)?);
    println!(
        "Read the C64 display to verify peer closure and clean exit. This test does not verify physical keyboard input."
    );
    Ok(())
}
async fn test_input(config: &Config, countdown: u64) -> Result<()> {
    // Preserve fail-fast backend/layout checks before beginning the deliberate countdown.
    let selection = config.backend.clone();
    let layout = runtime::layout(config);
    let probe =
        tokio::task::spawn_blocking(move || backend::probe_backend(&selection, layout)).await?;
    if !probe.available || !probe.authorized || probe.layout_matches != Some(true) {
        anyhow::bail!("Input backend preflight failed: {}", probe.detail);
    }
    eprintln!(
        "Deliberate input test: focus an empty TextEdit/VS Code/Notepad/terminal buffer. No delivery result is inferred automatically."
    );
    for remaining in (1..=countdown).rev() {
        eprintln!("Typing test in {remaining} seconds…");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    operations::test_input(config).await?;
    println!("Input events posted. Verify the phrase c64 bridge in the focused application.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_host::prg;
    #[test]
    fn actual_prg_templates_can_be_patched() {
        for template in [KEYBOARD_PRG, PROBE_PRG] {
            let prg = prg::patch(
                template,
                Ipv4Addr::LOCALHOST,
                6502,
                pairing::Token([42; 16]),
                "C64-Ultimate",
            )
            .unwrap();
            assert_eq!(prg.len(), template.len());
            assert!(prg.len() > 4096);
        }
    }
    #[test]
    fn cli_has_explicit_start_safety_flags() {
        let args = Cli::try_parse_from([
            "c64bridge",
            "start",
            "--deploy",
            "--allow-reset",
            "--enable-input",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            Command::Start(Start {
                deploy: true,
                allow_reset: true,
                enable_input: true,
                ..
            })
        ));
    }
}
