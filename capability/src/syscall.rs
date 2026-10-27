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
/// * `reference` — Slot-plus-generation reference from the syscall arguments
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
	reference: crate::cspace::CapabilityRef,
	request: crate::rights::CapabilityRequest,
	now: u64,
) -> Result<usize, u64> {
	use crate::validate::validate;

	match validate(store, cspace, caller_id, reference, &request, now) {
		Ok(ref validated) => {
			crate::audit::global_audit().reset_rate_limit(caller_id);
			crate::audit::global_audit().emit(crate::AuditRecord {
				sequence: 0,
				ticks: now,
				cpu: 0,
				caller: caller_id,
				operation: request.operation,
				object_kind: "inode",
				object_id: validated.record.cap_type.object_id(),
				requested_rights: request.required_rights.0,
				result: crate::AuditResult::Ok,
				parent_generation: 0,
			});
			Ok(validated.slot)
		}
		Err(ref err) => {
			crate::audit::global_audit().emit(crate::AuditRecord {
				sequence: 0,
				ticks: now,
				cpu: 0,
				caller: caller_id,
				operation: request.operation,
				object_kind: "inode",
				object_id: request.object.as_u64(),
				requested_rights: request.required_rights.0,
				result: crate::AuditResult::Denied(*err as u8),
				parent_generation: 0,
			});
			Err(err.to_errno())
		}
	}
}
