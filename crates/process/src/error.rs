//! Process-layer errors.

/// Failure to spawn or talk to a child.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// OS refused to spawn.
    #[error("spawn: {0}")]
    Spawn(#[source] std::io::Error),
    /// Bootstrap/handshake failed; child has been killed.
    #[error("ipc during bootstrap: {0}")]
    Ipc(#[from] cl_ipc::IpcError),
    /// The child exited while the browser waited for it to connect to the bootstrap server (a
    /// child that connects and exits within one poll slice is reported here too); `code` is its
    /// exit code (`None` if it was killed by a signal). A child that refuses to run without a
    /// sandbox exits 78 here.
    #[error("child exited during bootstrap with code {}", .code.map_or_else(|| "none (signal)".to_owned(), |c| c.to_string()))]
    ExitedDuringBootstrap {
        /// The child's exit code, if it exited normally.
        code: Option<i32>,
    },
    /// Waiting on / killing the child failed.
    #[error("child process: {0}")]
    Child(#[source] std::io::Error),
}
