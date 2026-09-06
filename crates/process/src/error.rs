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
    /// Waiting on / killing the child failed.
    #[error("child process: {0}")]
    Child(#[source] std::io::Error),
}
