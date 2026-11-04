//! Copy-then-validate for shared memory received from a less trusted process (ADR-0016 §4).
//! `ipc-channel` leaves the sender a writable mapping, so validating the mapping in place is a
//! time-of-check/time-of-use bug; every receiver copies first, through [`copy_bounded`].

use ipc_channel::ipc::IpcSharedMemory;

/// Shared memory that cannot be accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShmError {
    /// The region is larger than the receiver's limit.
    #[error("shared memory too large: {size} bytes > {max}")]
    TooLarge {
        /// The region's length.
        size: usize,
        /// The receiver's limit.
        max: usize,
    },
}

/// Copies `shm` into owned memory, refusing a region longer than `max` before touching a byte
/// of it. Validate and decode the returned copy, never the mapping.
pub fn copy_bounded(shm: &IpcSharedMemory, max: usize) -> Result<Vec<u8>, ShmError> {
    let size = shm.len();
    if size > max {
        return Err(ShmError::TooLarge { size, max });
    }
    Ok(shm.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_bounded_should_reject_oversize_before_copy() {
        let shm = IpcSharedMemory::from_bytes(&[7; 100]);
        assert_eq!(
            copy_bounded(&shm, 99),
            Err(ShmError::TooLarge { size: 100, max: 99 })
        );
    }

    #[test]
    fn copy_bounded_should_copy_a_region_at_the_limit() {
        let shm = IpcSharedMemory::from_bytes(&[7; 100]);
        assert_eq!(copy_bounded(&shm, 100), Ok(vec![7; 100]));
    }
}
