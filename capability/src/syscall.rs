/*
 * Syscall Boundary — convert CapabilityError to errno
 *
 * Called at the syscall/IPC boundary. Internal validation uses
 * structured CapabilityError; this module converts to the kernel's
 * errno encoding (u64::MAX - n).
 */

use crate::CapabilityError;

/// Kernel errno constants (matching existing encoding).
pub const ERRNO_EPERM: u64 = u64::MAX - 1;
pub const ERRNO_EBADF: u64 = u64::MAX - 8;
pub const ERRNO_ENOENT: u64 = u64::MAX - 2;

/*
 * capability_error_to_errno — Convert a CapabilityError to kernel errno
 */
pub fn capability_error_to_errno(err: CapabilityError) -> u64 {
	err.to_errno()
}

/*
 * authorize_syscall — Common authorization gate for syscalls
 *
 * Validates a capability handle against a request and returns
 * an errno on failure. On success, returns the slot index for
 * the caller to use in the protected operation.
 *
 /// # Arguments
 /// * `store` — Global capability store
 /// * `cspace` — Caller's capability space
 /// * `caller_id` — Caller's task ID
 /// * `handle` — Capability handle from the syscall argument
 /// * `request` — What the syscall wants to do
 /// * `now` — Current tick count
 ///
 /// # Returns
 /// `Ok(slot)` if validated, `Err(errno)` otherwise.
 */
pub fn authorize_syscall(
	store: &crate::store::CapabilityStore,
	cspace: &crate::cspace::CapabilitySpace,
	caller_id: u64,
	handle: &crate::types::CapabilityHandle,
	request: crate::rights::CapabilityRequest,
	now: u64,
) -> Result<usize, u64> {
	use crate::validate::validate_handle;

	match validate_handle(store, cspace, caller_id, handle, &request, now) {
		Ok(_validated) => {
			// ponytail: audit reset on success
			// crate::audit::GLOBAL_AUDIT.reset_rate_limit(caller_id);
			Ok(_validated.slot)
		}
		Err(err) => Err(err.to_errno()),
	}
}
