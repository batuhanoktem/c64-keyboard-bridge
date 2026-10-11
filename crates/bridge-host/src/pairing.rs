use anyhow::{Context, Result, bail};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub struct Token(pub [u8; 16]);
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([REDACTED])")
    }
}
impl Token {
    pub fn matches(self, other: &[u8; 16]) -> bool {
        self.0
            .iter()
            .zip(other)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    }
    pub fn hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
    pub fn from_hex(value: &str) -> Result<Self> {
        if value.len() != 32 || !value.is_ascii() {
            bail!("Invalid control authentication");
        }
        let mut token = [0; 16];
        for (i, b) in token.iter_mut().enumerate() {
            *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
                .map_err(|_| anyhow::anyhow!("Invalid control authentication"))?;
        }
        Ok(Self(token))
    }
}
pub fn prepare_directory(path: &Path) -> Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path).context("Cannot create private bridge state directory")?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("Bridge state path must be a private directory, not a symlink");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            bail!("Bridge state directory must be private (chmod 700 on that directory)");
        }
    }
    #[cfg(not(unix))]
    let _ = existed;
    Ok(())
}
pub fn private_file(path: &Path, create_new: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .context("Cannot create private bridge state file")
}
pub fn read_token(state_dir: &Path) -> Result<Token> {
    let path = state_dir.join("pairing.key");
    let metadata = fs::symlink_metadata(&path)
        .context("No pairing token; run start or device deploy first")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("Pairing token must be a regular private file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("Pairing token permissions must be private (chmod 600 pairing.key)");
        }
    }
    let mut bytes = Vec::new();
    File::open(&path)?.take(17).read_to_end(&mut bytes)?;
    if bytes.len() != 16 {
        bail!("Pairing token is corrupt; stop the bridge and run pair rotate");
    }
    Ok(Token(bytes.try_into().expect("checked token length")))
}
pub fn load_or_create_token(state_dir: &Path) -> Result<Token> {
    prepare_directory(state_dir)?;
    if state_dir.join("pairing.key").exists() {
        return read_token(state_dir);
    }
    let mut token = [0; 16];
    getrandom::fill(&mut token)
        .map_err(|_| anyhow::anyhow!("Operating system randomness unavailable"))?;
    let path = state_dir.join("pairing.key");
    match private_file(&path, true) {
        Ok(mut file) => {
            file.write_all(&token)?;
            file.sync_all()?;
            Ok(Token(token))
        }
        Err(_) if path.exists() => read_token(state_dir),
        Err(error) => Err(error),
    }
}
pub fn rotate_token(state_dir: &Path) -> Result<()> {
    prepare_directory(state_dir)?;
    if state_dir.join("runtime.json").exists() {
        bail!("Stop the bridge before rotating its pairing token");
    }
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Operating system randomness unavailable"))?;
    let temporary = state_dir.join("pairing.new");
    let mut file = private_file(&temporary, true)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    if state_dir.join("pairing.key").exists() {
        fs::remove_file(state_dir.join("pairing.key"))?;
    }
    fs::rename(&temporary, state_dir.join("pairing.key"))?;
    Ok(())
}

pub struct RuntimeGuard {
    path: PathBuf,
}
impl RuntimeGuard {
    pub fn create(state_dir: &Path, value: &impl serde::Serialize) -> Result<Self> {
        prepare_directory(state_dir)?;
        let path = state_dir.join("runtime.json");
        let mut file=private_file(&path,true).context("A runtime record already exists; use status, or remove the stale runtime.json after confirming the bridge stopped")?;
        serde_json::to_writer(&mut file, value)?;
        file.sync_all()?;
        Ok(Self { path })
    }
}
impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_persist_rotate_and_redact() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("private");
        let first = load_or_create_token(&dir).unwrap();
        assert!(first.matches(&read_token(&dir).unwrap().0));
        assert!(!format!("{first:?}").contains(&first.hex()));
        rotate_token(&dir).unwrap();
        assert!(!first.matches(&read_token(&dir).unwrap().0));
    }
    #[test]
    fn rejects_invalid_token_encoding() {
        assert!(Token::from_hex("🐈🐈🐈🐈🐈🐈🐈🐈").is_err());
    }
}
