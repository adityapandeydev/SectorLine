//! User-owned recording storage, independent of the source checkout.
use super::openf1::Result;
use std::path::PathBuf;

pub fn session_cache_root() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|s| PathBuf::from(s).join(".cache")))
    };
    Ok(base
        .filter(|p| p.is_absolute())
        .ok_or("No absolute user cache directory is available; provide a cache root explicitly")?
        .join("sectorline")
        .join("sessions"))
}
