//! Process types of the multi-process architecture (ADR-0005).

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Which role a process plays. One binary, selected by `--type=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProcessType {
    /// The single privileged process: UI, navigation, broker.
    Browser,
    /// Sandboxed web-content process, one per site.
    Renderer,
    /// Network service: TLS, HTTP, cache, cookies.
    Network,
    /// Compositor and raster.
    Gpu,
    /// Short-lived sandboxed helper (decoders).
    Utility,
}

/// Error for an unrecognised `--type=` value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown process type {0:?} (expected browser|renderer|network|gpu|utility)")]
pub struct UnknownProcessType(pub String);

impl ProcessType {
    /// Stable command-line name used in `--type=<name>`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Renderer => "renderer",
            Self::Network => "network",
            Self::Gpu => "gpu",
            Self::Utility => "utility",
        }
    }

    /// Every process except the browser runs under a sandbox policy (ADR-0005).
    #[must_use]
    pub const fn is_sandboxed(self) -> bool {
        !matches!(self, Self::Browser)
    }
}

impl FromStr for ProcessType {
    type Err = UnknownProcessType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "browser" => Ok(Self::Browser),
            "renderer" => Ok(Self::Renderer),
            "network" => Ok(Self::Network),
            "gpu" => Ok(Self::Gpu),
            "utility" => Ok(Self::Utility),
            other => Err(UnknownProcessType(other.to_owned())),
        }
    }
}

impl fmt::Display for ProcessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_should_parse_all_known_names() {
        assert_eq!("browser".parse::<ProcessType>(), Ok(ProcessType::Browser));
        assert_eq!("renderer".parse::<ProcessType>(), Ok(ProcessType::Renderer));
        assert_eq!("network".parse::<ProcessType>(), Ok(ProcessType::Network));
        assert_eq!("gpu".parse::<ProcessType>(), Ok(ProcessType::Gpu));
        assert_eq!("utility".parse::<ProcessType>(), Ok(ProcessType::Utility));
    }

    #[test]
    fn from_str_should_reject_unknown_name() {
        assert_eq!(
            "kernel".parse::<ProcessType>(),
            Err(UnknownProcessType("kernel".to_owned()))
        );
    }

    #[test]
    fn as_str_should_round_trip_through_from_str() {
        for pt in [
            ProcessType::Browser,
            ProcessType::Renderer,
            ProcessType::Network,
            ProcessType::Gpu,
            ProcessType::Utility,
        ] {
            assert_eq!(pt.as_str().parse::<ProcessType>(), Ok(pt));
        }
    }

    #[test]
    fn only_browser_should_be_unsandboxed() {
        assert!(!ProcessType::Browser.is_sandboxed());
        assert!(ProcessType::Renderer.is_sandboxed());
        assert!(ProcessType::Network.is_sandboxed());
        assert!(ProcessType::Gpu.is_sandboxed());
        assert!(ProcessType::Utility.is_sandboxed());
    }
}
