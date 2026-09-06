//! Sandbox type-state (ADR-0005, Gate S0). Renderer/network/gpu/utility mains take a
//! `Sandbox<Applied>` by value, so they cannot be constructed without applying a policy.
//!
//! M0 ships only `NotImplementedPolicy` (always fails) and, in debug builds, `DevNoSandbox`.
//! Real seatbelt/seccomp/AppContainer policies land in M1/M2 as further `SandboxPolicy` impls.

use std::marker::PhantomData;

/// Marker: policy not yet applied.
#[derive(Debug)]
pub struct Unapplied;

/// Marker: policy applied to the current process.
#[derive(Debug)]
pub struct Applied;

/// Why a policy could not be applied. Any error here must abort the child.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// No real policy exists yet for this platform/process type.
    #[error("sandbox not implemented for this platform yet; refusing to run untrusted content")]
    NotImplemented,
    /// OS call failed.
    #[error("sandbox os error: {0}")]
    Os(#[source] std::io::Error),
}

/// A sandbox policy for the calling process.
pub trait SandboxPolicy {
    /// Human-readable name for logs.
    fn name(&self) -> &'static str;
    /// Apply to the current process. Irreversible.
    fn apply(&self) -> Result<(), SandboxError>;
}

/// Proof-of-application token. `S` is [`Unapplied`] or [`Applied`].
#[derive(Debug)]
pub struct Sandbox<S> {
    policy_name: &'static str,
    _state: PhantomData<S>,
}

/// Release default: always fails. Exists so release binaries cannot accidentally run children
/// without a sandbox (docs/SECURITY.md §5 Gate S0).
#[derive(Debug, Default, Clone, Copy)]
pub struct NotImplementedPolicy;

/// Debug-only escape hatch behind `--no-sandbox`. Does not exist in release builds.
#[cfg(debug_assertions)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DevNoSandbox;

impl Sandbox<Unapplied> {
    /// Start the type-state. Nothing has happened to the process yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            policy_name: "unapplied",
            _state: PhantomData,
        }
    }

    /// Apply `policy` to the current process and return proof.
    #[allow(clippy::unused_self)]
    pub fn apply(self, policy: &dyn SandboxPolicy) -> Result<Sandbox<Applied>, SandboxError> {
        policy.apply()?;
        tracing::info!(policy = policy.name(), "sandbox applied");
        Ok(Sandbox {
            policy_name: policy.name(),
            _state: PhantomData,
        })
    }
}

impl Default for Sandbox<Unapplied> {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox<Applied> {
    /// Name of the policy that was applied.
    #[must_use]
    pub fn policy_name(&self) -> &'static str {
        self.policy_name
    }
}

impl SandboxPolicy for NotImplementedPolicy {
    fn name(&self) -> &'static str {
        "not-implemented"
    }

    fn apply(&self) -> Result<(), SandboxError> {
        Err(SandboxError::NotImplemented)
    }
}

#[cfg(debug_assertions)]
impl SandboxPolicy for DevNoSandbox {
    fn name(&self) -> &'static str {
        "dev-no-sandbox"
    }

    fn apply(&self) -> Result<(), SandboxError> {
        tracing::warn!(
            "running WITHOUT a sandbox (debug build, --no-sandbox). Never load untrusted content."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::expect_used)]
    fn not_implemented_policy_should_fail_to_apply() {
        let err = Sandbox::new()
            .apply(&NotImplementedPolicy)
            .expect_err("must fail");
        assert!(matches!(err, SandboxError::NotImplemented));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[allow(clippy::expect_used)]
    fn dev_no_sandbox_should_apply_and_report_its_name() {
        let sb = Sandbox::new()
            .apply(&DevNoSandbox)
            .expect("dev policy applies");
        assert_eq!(sb.policy_name(), "dev-no-sandbox");
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn applied_sandbox_should_not_be_constructible_without_apply() {
        // Compile-time property: `Sandbox<Applied>` has no public constructor. This test documents
        // it; the real guard is the private field + PhantomData. If someone adds a constructor,
        // review must reject it.
        fn takes_applied(_: Sandbox<Applied>) {}
        #[cfg(debug_assertions)]
        takes_applied(
            Sandbox::new()
                .apply(&DevNoSandbox)
                .expect("dev policy applies"),
        );
        #[cfg(not(debug_assertions))]
        let _ = takes_applied;
    }
}
