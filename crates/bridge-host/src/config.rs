use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Secret(String);
impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Overrides {
    pub device_host: Option<String>,
    pub bind_address: Option<Ipv4Addr>,
    pub advertise_ip: Option<Ipv4Addr>,
    pub bridge_port: Option<u16>,
    pub layout: Option<String>,
    pub profile: Option<String>,
    pub backend: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub device_host: Option<String>,
    pub device_port: u16,
    pub password: Secret,
    pub bind_address: Option<Ipv4Addr>,
    pub advertise_ip: Option<Ipv4Addr>,
    pub bridge_port: u16,
    pub allowed_ips: Vec<Ipv4Addr>,
    pub auto_connect: bool,
    pub auto_deploy: bool,
    pub layout: String,
    pub profile: String,
    pub mappings_file: Option<PathBuf>,
    pub backend: String,
    pub log_level: String,
    pub state_dir: PathBuf,
    pub repeat_delay: Duration,
    pub repeat_interval: Duration,
}

impl Config {
    pub fn load(env_file: &Path, overrides: Overrides) -> Result<Self> {
        let file_values = if env_file.exists() {
            dotenvy::from_path_iter(env_file)
                .map_err(|_| anyhow::anyhow!("Cannot read .env configuration"))?
                .collect::<std::result::Result<BTreeMap<_, _>, _>>()
                .map_err(|_| {
                    anyhow::anyhow!("Invalid .env syntax; quote passwords with special characters")
                })?
        } else {
            BTreeMap::new()
        };
        let environment = std::env::vars().collect();
        Self::from_values(file_values, environment, overrides)
    }

    pub fn from_values(
        mut values: BTreeMap<String, String>,
        environment: BTreeMap<String, String>,
        o: Overrides,
    ) -> Result<Self> {
        values.extend(environment);
        let value =
            |name: &str, default: &str| values.get(name).cloned().unwrap_or_else(|| default.into());
        let optional = |name: &str| values.get(name).filter(|s| !s.trim().is_empty()).cloned();
        let port = |name: &str, default: &str| -> Result<u16> {
            let port: u16 = value(name, default)
                .parse()
                .map_err(|_| anyhow::anyhow!("{name} must be a port from 1 to 65535"))?;
            if port == 0 {
                bail!("{name} must be a port from 1 to 65535");
            }
            Ok(port)
        };
        let boolean = |name: &str, default: &str| -> Result<bool> {
            value(name, default)
                .parse()
                .map_err(|_| anyhow::anyhow!("{name} must be true or false"))
        };
        let address = |name: &str| -> Result<Option<Ipv4Addr>> {
            optional(name)
                .map(|s| {
                    s.parse()
                        .map_err(|_| anyhow::anyhow!("{name} must be an IPv4 address"))
                })
                .transpose()
        };
        let state_dir = optional("BRIDGE_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                directories::ProjectDirs::from("net", "c64bridge", "C64 Keyboard Bridge")
                    .map(|d| d.data_local_dir().to_owned())
                    .unwrap_or_else(|| PathBuf::from(".c64bridge-state"))
            });
        let allowed_ips = optional("C64_ALLOWED_IPS")
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().parse::<Ipv4Addr>())
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .transpose()
            .map_err(|_| {
                anyhow::anyhow!("C64_ALLOWED_IPS must contain comma-separated IPv4 addresses")
            })?
            .unwrap_or_default();
        let password = value("C64_NETWORK_PASSWORD", "");
        if password.bytes().any(|c| c < 32 || c == 127) {
            bail!("C64_NETWORK_PASSWORD contains characters invalid in an HTTP header");
        }
        let millis = |name: &str, default: &str, min: u64, max: u64| -> Result<Duration> {
            let n: u64 = value(name, default)
                .parse()
                .map_err(|_| anyhow::anyhow!("{name} must be an integer"))?;
            if !(min..=max).contains(&n) {
                bail!("{name} must be between {min} and {max} milliseconds");
            }
            Ok(Duration::from_millis(n))
        };
        let c = Self {
            device_host: o.device_host.or_else(|| optional("C64_HOST")),
            device_port: port("C64_PORT", "80")?,
            password: Secret(password),
            bind_address: o.bind_address.or(address("BRIDGE_BIND_ADDRESS")?),
            advertise_ip: o.advertise_ip.or(address("HOST_ADVERTISE_IP")?),
            bridge_port: o.bridge_port.unwrap_or(port("BRIDGE_PORT", "6502")?),
            allowed_ips,
            auto_connect: boolean("AUTO_CONNECT", "true")?,
            auto_deploy: boolean("AUTO_DEPLOY", "false")?,
            layout: o.layout.unwrap_or(value("KEYBOARD_LAYOUT", "US")),
            profile: o.profile.unwrap_or(value("KEYBOARD_PROFILE", "modern")),
            mappings_file: optional("MAPPINGS_FILE").map(PathBuf::from),
            backend: o.backend.unwrap_or(value("INPUT_BACKEND", "auto")),
            log_level: value("LOG_LEVEL", "info"),
            state_dir,
            repeat_delay: millis("REPEAT_DELAY_MS", "500", 100, 5000)?,
            repeat_interval: millis("REPEAT_INTERVAL_MS", "40", 10, 1000)?,
        };
        if c.bridge_port == 0 {
            bail!("BRIDGE_PORT must be nonzero");
        }
        if !["US", "UK"].contains(&c.layout.as_str()) {
            bail!("KEYBOARD_LAYOUT must be US or UK");
        }
        if !["modern", "authentic"].contains(&c.profile.as_str()) {
            bail!("KEYBOARD_PROFILE must be modern or authentic");
        }
        if ![
            "auto",
            "recording",
            "macos",
            "windows",
            "x11",
            "uinput",
            "portal",
        ]
        .contains(&c.backend.as_str())
        {
            bail!("INPUT_BACKEND must be auto, recording, macos, windows, x11, uinput, or portal");
        }
        if let Some(h) = &c.device_host
            && (h.is_empty()
                || h.len() > 253
                || h.chars()
                    .any(|c| c.is_whitespace() || "/\\:@?#".contains(c)))
        {
            bail!("C64_HOST must be an IPv4 address or hostname without a URL scheme");
        }
        for (name, ip) in [
            ("HOST_ADVERTISE_IP", c.advertise_ip),
            ("BRIDGE_BIND_ADDRESS", c.bind_address),
        ] {
            if let Some(ip) = ip
                && (ip.is_multicast()
                    || ip.is_broadcast()
                    || (name == "HOST_ADVERTISE_IP" && ip.is_unspecified()))
            {
                bail!("{name} must be a unicast IPv4 address");
            }
        }
        Ok(c)
    }

    pub fn require_device(&self) -> Result<&str> {
        self.device_host
            .as_deref()
            .context("C64_HOST is missing; set the device address in your ignored .env file")
    }

    pub async fn network(&self) -> Result<NetworkConfig> {
        let host = self.require_device()?;
        let ips: Vec<_> = tokio::net::lookup_host((host, self.device_port))
            .await
            .context("Cannot resolve C64_HOST")?
            .filter_map(|a| match a.ip() {
                std::net::IpAddr::V4(ip) => Some(ip),
                _ => None,
            })
            .collect();
        let device_ip = *ips.first().context("C64_HOST resolved no IPv4 address")?;
        let advertise_ip = if let Some(ip) = self.advertise_ip {
            ip
        } else {
            let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                .context("Cannot inspect local network route")?;
            socket
                .connect((device_ip, self.device_port))
                .context("No route to the C64; set HOST_ADVERTISE_IP explicitly")?;
            match socket.local_addr()?.ip() {
                std::net::IpAddr::V4(ip) if !ip.is_unspecified() => ip,
                _ => bail!("Set HOST_ADVERTISE_IP explicitly"),
            }
        };
        Ok(NetworkConfig {
            device_ip,
            advertise_ip,
            bind: SocketAddrV4::new(self.bind_address.unwrap_or(advertise_ip), self.bridge_port),
            allowed_ips: if self.allowed_ips.is_empty() {
                ips
            } else {
                self.allowed_ips.clone()
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct NetworkConfig {
    pub device_ip: Ipv4Addr,
    pub advertise_ip: Ipv4Addr,
    pub bind: SocketAddrV4,
    pub allowed_ips: Vec<Ipv4Addr>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    #[test]
    fn precedence_and_secret_redaction() {
        let c = Config::from_values(
            values(&[
                ("BRIDGE_PORT", "6000"),
                ("C64_NETWORK_PASSWORD", "a$ b#=\"'"),
            ]),
            values(&[("BRIDGE_PORT", "6001")]),
            Overrides {
                bridge_port: Some(6002),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(c.bridge_port, 6002);
        assert_eq!(c.password.expose(), "a$ b#=\"'");
        assert!(!format!("{c:?}").contains(c.password.expose()));
    }
    #[test]
    fn invalid_settings_and_header_never_echo_values() {
        for (name, value) in [
            ("C64_NETWORK_PASSWORD", "topsecret\n"),
            ("BRIDGE_PORT", "0"),
            ("AUTO_DEPLOY", "maybe"),
            ("KEYBOARD_LAYOUT", "DE"),
        ] {
            let error = Config::from_values(
                values(&[(name, value)]),
                BTreeMap::new(),
                Overrides::default(),
            )
            .unwrap_err()
            .to_string();
            assert!(!error.contains(value));
        }
    }
    #[test]
    fn dotenv_literal_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.env");
        std::fs::write(&path, "C64_NETWORK_PASSWORD='a$ b#=\"'\n").unwrap();
        let parsed = dotenvy::from_path_iter(path)
            .unwrap()
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()
            .unwrap();
        assert_eq!(parsed["C64_NETWORK_PASSWORD"], "a$ b#=\"");
    }
}
