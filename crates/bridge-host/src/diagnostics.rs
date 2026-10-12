use crate::{backend, config::Config, control, rest::RestClient, runtime};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub state: String,
    pub detail: String,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}
impl Report {
    fn add(&mut self, name: &str, state: &str, detail: impl Into<String>) {
        self.checks.push(Check {
            name: name.into(),
            state: state.into(),
            detail: detail.into(),
        });
    }
    pub fn print(&self) {
        for check in &self.checks {
            println!("{}: {} — {}", check.name, check.state, check.detail);
        }
    }
}
pub async fn doctor(config: &Config) -> Report {
    let mut report = Report { checks: Vec::new() };
    report.add(
        "Configuration",
        "valid",
        "Values parsed and validated; secrets redacted",
    );
    let probe = backend::probe_backend(&config.backend, runtime::layout(config));
    report.add(
        "Input backend",
        if probe.available && probe.authorized {
            "available"
        } else {
            "unavailable"
        },
        format!("{}: {}", probe.name, probe.detail),
    );
    report.add(
        "Layout",
        match probe.layout_matches {
            Some(true) => "matches",
            Some(false) => "mismatch",
            None => "unknown; injection disabled",
        },
        format!(
            "Configured {}. Unicode and focused-application delivery require manual tests",
            config.layout
        ),
    );
    let device = match RestClient::new(config) {
        Ok(client) => match client.info().await {
            Ok(info) => {
                report.add(
                    "REST API",
                    "verified",
                    "Authenticated GET /v1/info succeeded",
                );
                report.add(
                    "Device",
                    "verified",
                    format!(
                        "{}; firmware {}; FPGA {}; core {}",
                        info.product,
                        info.firmware_version,
                        info.fpga_version,
                        info.core_version.as_deref().unwrap_or("not supplied")
                    ),
                );
                Some(info)
            }
            Err(error) => {
                report.add("REST API", "failed", format!("{error:#}"));
                None
            }
        },
        Err(error) => {
            report.add("REST API", "not configured", format!("{error:#}"));
            None
        }
    };
    match config.network().await {
        Ok(network) => {
            report.add(
                "Network route",
                "available",
                format!(
                    "Device {}; advertise {}; bind {}",
                    network.device_ip, network.advertise_ip, network.bind
                ),
            );
            match tokio::net::TcpListener::bind(network.bind).await {
                Ok(_) => report.add("Listener port", "available", "Port can be bound"),
                Err(_) => report.add(
                    "Listener port",
                    "in use",
                    "Check bridge status or choose BRIDGE_PORT",
                ),
            }
        }
        Err(error) => report.add("Network route", "unavailable", format!("{error:#}")),
    }
    if let Ok(status) = control::send(&config.state_dir, "status").await {
        report.add(
            "Runtime",
            if status.status.enabled {
                "input enabled"
            } else {
                "input disabled"
            },
            status.status.detail,
        );
        let diagnostics = status.status.diagnostics;
        report.add(
            "Session diagnostics",
            "retained in memory",
            format!(
                "{} attempts; {} authenticated sessions; {} reconnects; phase {}; valid-frame age {}",
                diagnostics.attempts,
                diagnostics.authenticated_sessions,
                diagnostics.reconnects,
                diagnostics.current.phase.as_str(),
                frame_age(diagnostics.current.valid_frame_age_ms),
            ),
        );
        report.add(
            "Session failure counters",
            "retained in memory",
            serde_json::to_string(&diagnostics.failures).expect("Fixed counters serialize"),
        );
        if let Some(failure) = diagnostics.last_failure {
            report.add(
                "Last session/input failure",
                match failure.cleanup {
                    crate::session_diagnostics::CleanupOutcome::Pending => "cleanup pending",
                    crate::session_diagnostics::CleanupOutcome::Succeeded => "keys released",
                    crate::session_diagnostics::CleanupOutcome::Failed => "cleanup failed",
                },
                format!(
                    "{} during {}; {} ms elapsed; valid-frame age {}; {} validated frames",
                    failure.reason.as_str(),
                    failure.phase.as_str(),
                    failure.elapsed_ms,
                    frame_age(failure.valid_frame_age_ms),
                    failure.validated_frames,
                ),
            );
        }
    }
    let evidence = std::fs::read(config.state_dir.join("hardware-evidence.json"))
        .ok()
        .filter(|v| v.len() <= 65536)
        .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
    let same_device = evidence.as_ref().is_some_and(|e| {
        device.as_ref().is_some_and(|d| {
            e["device"]["unique_id"] == d.unique_id
                && e["device"]["firmware_version"] == d.firmware_version
                && e["device"]["fpga_version"] == d.fpga_version
                && e["device"]["core_version"]
                    == serde_json::to_value(&d.core_version).unwrap_or_default()
        })
    });
    if same_device {
        let e = evidence.unwrap();
        report.add("UCI / Network Target","previously verified",format!("Bidirectional probe at Unix time {}; peer-close and physical typing remain manual tests",e["verified_unix_seconds"]));
    } else {
        report.add("UCI / Network Target","not verified","REST connectivity and the reported $C9 ID do not prove socket operations. Run device test --allow-reset on the physical C64.");
    }
    report
}
fn frame_age(age: Option<u64>) -> String {
    age.map(|age| format!("{age} ms"))
        .unwrap_or_else(|| "none".into())
}
