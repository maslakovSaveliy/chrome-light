//! Well-known directories. Profile data lives under the OS-appropriate local data dir.

use std::path::PathBuf;

/// Error locating a platform directory.
#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    /// The OS did not report a home/data directory (e.g. broken `$HOME`).
    #[error("no local data directory available on this system")]
    NoDataDir,
}

/// `<os local data dir>/ChromeLight/Default`, e.g.
/// `~/Library/Application Support/ChromeLight/Default` on macOS,
/// `%LOCALAPPDATA%\ChromeLight\Default` on Windows, `~/.local/share/ChromeLight/Default` on Linux.
pub fn default_profile_dir() -> Result<PathBuf, PathsError> {
    let base = directories::BaseDirs::new().ok_or(PathsError::NoDataDir)?;
    Ok(base.data_local_dir().join("ChromeLight").join("Default"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::expect_used)]
    fn default_profile_dir_should_end_with_product_and_profile_segments() {
        let dir = default_profile_dir().expect("data dir available in test environment");
        let mut comps = dir.components().rev();
        assert_eq!(
            comps
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
            Some("Default".to_owned())
        );
        assert_eq!(
            comps
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
            Some("ChromeLight".to_owned())
        );
    }
}
