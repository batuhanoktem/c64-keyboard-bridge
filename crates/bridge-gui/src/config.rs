use crate::model::GuiSettings;
use anyhow::{Context, Result, bail};
use bridge_host::config::{Config, Overrides};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SETTINGS_LIMIT: u64 = 1024 * 1024;

pub fn default_state_dir() -> PathBuf {
    directories::ProjectDirs::from("net", "c64bridge", "C64 Keyboard Bridge")
        .map(|d| d.data_local_dir().to_owned())
        .unwrap_or_else(|| PathBuf::from(".c64bridge-state"))
}

pub fn random_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Operating system randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn read_regular(path: &Path, private: bool) -> Result<String> {
    let metadata = fs::symlink_metadata(path).context("Cannot read configuration file")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > SETTINGS_LIMIT {
        bail!("Configuration must be a bounded regular file, not a symlink");
    }
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("GUI state file must have private permissions (chmod 600)");
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(SETTINGS_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > SETTINGS_LIMIT {
        bail!("Configuration file exceeds size limit");
    }
    String::from_utf8(bytes).context("Configuration must be UTF-8")
}

/// Replace only an explicitly selected regular file, using a fresh private temporary.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > SETTINGS_LIMIT {
        bail!("Configuration file exceeds size limit");
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("Refusing to replace a symlink or non-file");
        }
    } else if fs::symlink_metadata(path).is_ok() {
        bail!("Refusing to replace a dangling symlink");
    }
    let temporary = parent.join(format!(".c64bridge-{}.tmp", random_id()?));
    let result = (|| {
        let mut file = bridge_host::pairing::private_file(&temporary, true)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        replace_windows(&temporary, path)?;
        #[cfg(not(windows))]
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_windows(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH, never delete the old file first.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
        return Err(std::io::Error::last_os_error()).context("Cannot replace GUI configuration");
    }
    Ok(())
}

impl GuiSettings {
    pub fn from_config(c: &Config, env_file: &Path) -> Result<Self> {
        let mut settings = Self {
            env_file: env_file.to_string_lossy().into_owned(),
            device_host: c.device_host.clone().unwrap_or_default(),
            device_port: c.device_port,
            bind_address: c.bind_address.map(|v| v.to_string()).unwrap_or_default(),
            advertise_ip: c.advertise_ip.map(|v| v.to_string()).unwrap_or_default(),
            bridge_port: c.bridge_port,
            allowed_ips: c
                .allowed_ips
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
            layout: c.layout.clone(),
            profile: c.profile.clone(),
            backend: c.backend.clone(),
            mappings_file: c
                .mappings_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            repeat_delay_ms: c.repeat_delay.as_millis() as u64,
            repeat_interval_ms: c.repeat_interval.as_millis() as u64,
            log_level: c.log_level.clone(),
            ..Default::default()
        };
        settings.resolve_paths(&std::env::current_dir()?);
        Ok(settings)
    }
    /// Retain the import/save location so Finder or desktop relaunches do not change configuration.
    pub fn resolve_paths(&mut self, base: &Path) {
        for value in [&mut self.env_file, &mut self.mappings_file] {
            if !value.is_empty() && Path::new(value).is_relative() {
                *value = base.join(&*value).to_string_lossy().into_owned();
            }
        }
    }
    pub fn to_config(&self, state_dir: &Path, password: Option<&str>) -> Result<Config> {
        self.to_config_with_environment(state_dir, password, std::env::vars().collect())
    }
    pub fn to_config_with_environment(
        &self,
        state_dir: &Path,
        password: Option<&str>,
        mut environment: BTreeMap<String, String>,
    ) -> Result<Config> {
        if self.schema_version != 1 {
            bail!("Unsupported GUI settings version");
        }
        if !self.credential_id.is_empty()
            && (self.credential_id.len() != 32
                || !self.credential_id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            bail!("Invalid credential reference");
        }
        let file_values = if self.env_file.is_empty() || !Path::new(&self.env_file).exists() {
            BTreeMap::new()
        } else {
            if fs::metadata(&self.env_file)?.len() > SETTINGS_LIMIT {
                bail!("Environment file exceeds size limit");
            }
            dotenvy::from_path_iter(&self.env_file)
                .map_err(|_| anyhow::anyhow!("Cannot read .env configuration"))?
                .collect::<std::result::Result<BTreeMap<_, _>, _>>()
                .map_err(|_| anyhow::anyhow!("Invalid .env syntax; quote special characters"))?
        };
        environment.extend(
            [
                ("C64_HOST", self.device_host.clone()),
                ("C64_PORT", self.device_port.to_string()),
                ("BRIDGE_BIND_ADDRESS", self.bind_address.clone()),
                ("HOST_ADVERTISE_IP", self.advertise_ip.clone()),
                ("BRIDGE_PORT", self.bridge_port.to_string()),
                ("C64_ALLOWED_IPS", self.allowed_ips.clone()),
                ("KEYBOARD_LAYOUT", self.layout.clone()),
                ("KEYBOARD_PROFILE", self.profile.clone()),
                ("INPUT_BACKEND", self.backend.clone()),
                ("MAPPINGS_FILE", self.mappings_file.clone()),
                ("REPEAT_DELAY_MS", self.repeat_delay_ms.to_string()),
                ("REPEAT_INTERVAL_MS", self.repeat_interval_ms.to_string()),
                ("LOG_LEVEL", self.log_level.clone()),
                ("AUTO_DEPLOY", "false".into()),
                ("AUTO_CONNECT", "false".into()),
                ("BRIDGE_STATE_DIR", state_dir.to_string_lossy().into_owned()),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v)),
        );
        if let Some(password) = password {
            environment.insert("C64_NETWORK_PASSWORD".into(), password.into());
        }
        Config::from_values(file_values, environment, Overrides::default())
    }
}

pub fn load_settings(state_dir: &Path, env_override: Option<&Path>) -> Result<GuiSettings> {
    let path = state_dir.join("gui-settings.toml");
    let mut settings = if path.exists() {
        toml::from_str::<GuiSettings>(&read_regular(&path, true)?)
            .map_err(|_| anyhow::anyhow!("Invalid GUI settings; file preserved for recovery"))?
    } else {
        let env = env_override.unwrap_or(Path::new(".env"));
        let config = Config::load(env, Overrides::default())?;
        GuiSettings::from_config(&config, env)?
    };
    if let Some(path) = env_override {
        settings.env_file = path.to_string_lossy().into_owned();
    }
    settings.resolve_paths(&std::env::current_dir()?);
    if settings.credential_id.is_empty() {
        settings.credential_id = random_id()?;
    }
    settings.to_config(state_dir, None)?;
    Ok(settings)
}
pub fn save_settings(state_dir: &Path, settings: &GuiSettings) -> Result<()> {
    bridge_host::pairing::prepare_directory(state_dir)?;
    let mut settings = settings.clone();
    settings.resolve_paths(&std::env::current_dir()?);
    settings.to_config(state_dir, None)?;
    atomic_write(
        &state_dir.join("gui-settings.toml"),
        toml::to_string_pretty(&settings)?.as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("c64bridge-gui-settings-{}", random_id().unwrap()));
        bridge_host::pairing::prepare_directory(&p).unwrap();
        p
    }
    #[test]
    fn atomic_private_settings_roundtrip_excludes_secrets() {
        let p = directory();
        let s = GuiSettings {
            env_file: String::new(),
            credential_id: random_id().unwrap(),
            ..Default::default()
        };
        let sentinel = "PASSWORD_SENTINEL_7ab";
        let c = s.to_config(&p, Some(sentinel)).unwrap();
        assert_eq!(c.password.expose(), sentinel);
        save_settings(&p, &s).unwrap();
        assert!(
            !read_regular(&p.join("gui-settings.toml"), true)
                .unwrap()
                .contains(sentinel)
        );
        assert_eq!(load_settings(&p, None).unwrap(), s);
        let mut changed = s;
        changed.bridge_port = 6503;
        save_settings(&p, &changed).unwrap();
        assert_eq!(load_settings(&p, None).unwrap().bridge_port, 6503);
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn bad_settings_are_preserved_and_startup_never_enables_deployment() {
        let p = directory();
        let s = GuiSettings {
            env_file: String::new(),
            ..Default::default()
        };
        let c = s.to_config(&p, None).unwrap();
        assert!(!c.auto_deploy && !c.auto_connect);
        atomic_write(&p.join("gui-settings.toml"), b"schema_version=99").unwrap();
        assert!(load_settings(&p, None).is_err());
        assert_eq!(
            read_regular(&p.join("gui-settings.toml"), true).unwrap(),
            "schema_version=99"
        );
        fs::remove_dir_all(p).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn atomic_replace_refuses_symlinks_and_private_reads_refuse_public_modes() {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let p = directory();
        let target = p.join("target");
        fs::write(&target, "keep").unwrap();
        let link = p.join("link");
        symlink(&target, &link).unwrap();
        assert!(atomic_write(&link, b"replace").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_regular(&target, true).is_err());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn imported_paths_keep_original_location_across_desktop_relaunch() {
        let base = directory();
        let elsewhere = directory();
        atomic_write(
            &base.join("import.env"),
            b"C64_NETWORK_PASSWORD='SECRET_PATH_SENTINEL $ punctuation'\n",
        )
        .unwrap();
        atomic_write(
            &base.join("mapping.toml"),
            b"profile='modern'\nlayout='US'\n",
        )
        .unwrap();
        let mut settings = GuiSettings {
            env_file: "import.env".into(),
            mappings_file: "mapping.toml".into(),
            credential_id: random_id().unwrap(),
            ..Default::default()
        };
        settings.resolve_paths(&base);
        save_settings(&base, &settings).unwrap();
        let mut reloaded = load_settings(&base, None).unwrap();
        reloaded.resolve_paths(&elsewhere);
        assert_eq!(reloaded.env_file, base.join("import.env").to_string_lossy());
        assert_eq!(
            reloaded.mappings_file,
            base.join("mapping.toml").to_string_lossy()
        );
        assert_eq!(
            reloaded
                .to_config_with_environment(&base, None, Default::default())
                .unwrap()
                .password
                .expose(),
            "SECRET_PATH_SENTINEL $ punctuation"
        );
        assert!(
            !read_regular(&base.join("gui-settings.toml"), true)
                .unwrap()
                .contains("SECRET_PATH_SENTINEL")
        );
        fs::remove_dir_all(base).unwrap();
        fs::remove_dir_all(elsewhere).unwrap();
    }
}
