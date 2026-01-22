//! Configuration and file path management.

use crate::error::Error;
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// Get the path to the GitHub token file
pub fn token_path() -> PathBuf {
    dirs::home_dir()
        .expect("No home directory")
        .join(".local/share/copilot-api-proxy/github_token")
}

/// Load GitHub token from environment or file
pub fn load_github_token() -> Result<String, Error> {
    std::env::var("GITHUB_TOKEN").or_else(|_| {
        let path = token_path();
        fs::read_to_string(&path)
            .map(|s| s.trim().to_string())
            .map_err(|_| Error::Config(format!("Token not found at {:?}", path)))
    })
}

/// Ensure token directory exists with secure permissions
pub fn ensure_token_dir() -> Result<(), Error> {
    if let Some(parent) = token_path().parent() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write token with secure permissions (0600)
pub fn write_token(path: &PathBuf, token: &str) -> Result<(), Error> {
    fs::write(path, token)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(())
}
