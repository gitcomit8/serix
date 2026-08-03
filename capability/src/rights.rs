/*
 * Rights, CapabilityRequest, ValidatedCapability, CapabilityError
 *
 * Rights are a bitmask (defined in types::Rights). This module defines
 * the validation request/response types and the error enum.
 */

use crate::types::{CapabilityType, Rights};
use crate::object::ObjectId;

/* ------------------------------------------------------------------ */
/*  CapabilityRequest — what the caller wants to do                    */
/* ------------------------------------------------------------------ */

/// Describes the operation a caller wants to perform on an object.
/// Passed to `validate()` along with the caller's handle.
pub struct CapabilityRequest {
	/// The object being accessed
	pub object: ObjectId,
	/// Rights the caller needs for this operation
	pub required_rights: Rights,
	/// The operation class (for audit logging)
	pub operation: &'static str,
	/// The caller's task ID
	pub caller: u64,
	/// Optional: expected capability type (type check)
	pub expected_type: Option<CapabilityType>,
	/// Optional: expected rights superset (scope check)
	pub expected_rights: Option<Rights>,
}

/* ------------------------------------------------------------------ */
/*  ValidatedCapability — result of a successful validation            */
/* ------------------------------------------------------------------ */

/// Returned by `validate()` on success. Contains the full record plus
/// the slot index and generation from the caller's cspace.
#[derive(Clone, Debug)]
pub struct ValidatedCapability {
	/// The full capability record from the store
	pub record: crate::types::CapabilityRecord,
	/// Slot index in the caller's cspace
	pub slot: usize,
	/// Generation at time of validation
	pub generation: u32,
	/// Rights actually granted (intersection of record rights and requested)
	pub granted_rights: Rights,
}

impl ValidatedCapability {
	/* Check if the granted rights satisfy the requested rights */
	pub fn satisfies(&self, required: Rights) -> bool {
		self.granted_rights.contains(required)
	}
}

/* ------------------------------------------------------------------ */
/*  CapabilityError — structured validation failure                    */
/* ------------------------------------------------------------------ */

/// Detailed reason for validation failure. Converted to errno at the
/// syscall boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapabilityError {
	/// Handle slot does not exist in caller's cspace
	BadHandle,
	/// Handle slot has stale generation (was closed and reused)
	StaleGeneration,
	/// Capability record not found in store
	NotFound,
	/// Record generation mismatch (record was replaced)
	RecordGenerationMismatch,
	/// Handle does not belong to the caller's task
	ForeignHandle,
	/// Capability has been revoked
	Revoked,
	/// Capability has expired
	Expired,
	/// Capability type does not match expected type
	TypeMismatch,
	/// Capability does not cover the requested object
	ObjectMismatch,
	/// Capability lacks the required rights
	InsufficientRights,
	/// Capability cannot be delegated further (depth exhausted)
	DelegationDepthExhausted,
}

impl CapabilityError {
	/*
	 * to_errno - Convert to POSIX errno (u64 encoded as u64::MAX - n)
	 *
	 * Only called at the syscall boundary. Internal validation uses
	 * the structured CapabilityError.
	 */
	pub fn to_errno(self) -> u64 {
		const EPERM: u64 = u64::MAX - 1;
		const EBADF: u64 = u64::MAX - 8;
		const ENOENT: u64 = u64::MAX - 2;

		match self {
			Self::BadHandle
			| Self::StaleGeneration
			| Self::ForeignHandle
			| Self::DelegationDepthExhausted => EBADF,
			Self::NotFound | Self::RecordGenerationMismatch | Self::Expired => ENOENT,
			Self::Revoked | Self::TypeMismatch | Self::ObjectMismatch | Self::InsufficientRights => {
				EPERM
			}
		}
	}
}
