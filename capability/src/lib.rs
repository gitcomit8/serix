/*
 * Capability-based Security System
 *
 * Implements object capabilities for fine-grained access control.
 * Capabilities are unforgeable tokens that grant specific rights to resources.
 *
 * Architecture:
 * - types: CapabilityId, CapabilityHandle, CapabilityType, Rights, CapabilityRecord
 * - object: Typed ObjectId newtypes (PortId, InodeId, DeviceId, etc.)
 * - rights: Rights bitmask, CapabilityRequest, ValidatedCapability, CapabilityError
 * - cspace: Per-task capability space (slot → CapabilityId + generation)
 * - store: Authoritative CapabilityId → CapabilityRecord map with lifecycle ops
 * - validate: Core validation API (non-allocating, ordered checks)
 * - audit: Bounded overwrite-on-full audit ring buffer
 * - syscall: Errno conversion at syscall boundary
 */

#![no_std]

extern crate alloc;

pub mod audit;
pub mod cspace;
pub mod object;
pub mod rights;
pub mod store;
pub mod syscall;
pub mod types;
pub mod validate;

pub use store::CapabilityStore;
pub use types::{
	Capability, CapabilityHandle, CapabilityId, CapabilityRecord, CapabilityType, Rights,
};
pub use object::ObjectId;

use spin::{Mutex, Once};

pub use self::rights::{CapabilityError, CapabilityRequest, ValidatedCapability};

/* ------------------------------------------------------------------ */
/*  Global capability store                                            */
/* ------------------------------------------------------------------ */

static GLOBAL_CAP_STORE: Once<Mutex<CapabilityStore>> = Once::new();

/*
 * global_cap_store — Get the global capability store
 *
 * Returns a reference to the global capability store, initializing it
 * if needed. Called during kernel bootstrap.
 */
pub fn global_cap_store() -> &'static Mutex<CapabilityStore> {
	GLOBAL_CAP_STORE.call_once(|| Mutex::new(CapabilityStore::new()))
}

/*
 * init_global_store — Initialize the global capability store
 *
 * Idempotent: safe to call multiple times. Returns Err if already
 * initialized (should not happen in normal operation).
 */
pub fn init_global_store() -> Result<(), &'static str> {
	if GLOBAL_CAP_STORE
		.try_call_once(|| Ok::<_, &'static str>(Mutex::new(CapabilityStore::new())))
		.is_err()
	{
		Err("capability store already initialized")
	} else {
		Ok(())
	}
}

/* ------------------------------------------------------------------ */
/*  Entropy state                                                      */
/* ------------------------------------------------------------------ */

/// Whether RDSEED/RDRAND was health-checked during boot.  A false value
/// means the kernel is in restricted mode and must not accept capabilities
/// originating outside its bootstrap trust boundary.
static SECURE_ENTROPY_AVAILABLE: Once<bool> = Once::new();

pub fn has_secure_entropy() -> bool {
	*SECURE_ENTROPY_AVAILABLE.call_once(|| {
		let mut value = 0u64;
		let leaf1 = core::arch::x86_64::__cpuid(1);
		let leaf7 = core::arch::x86_64::__cpuid_count(7, 0);
		if leaf7.ebx & (1 << 18) != 0 {
			for _ in 0..16 {
				if unsafe { core::arch::x86_64::_rdseed64_step(&mut value) } == 1 {
					return true;
				}
			}
		}
		if leaf1.ecx & (1 << 30) != 0 {
			for _ in 0..16 {
				if unsafe { core::arch::x86_64::_rdrand64_step(&mut value) } == 1 {
					return true;
				}
			}
		}
		false
	})
}

pub fn init_entropy() { let _ = has_secure_entropy(); }

pub fn restricted_mode() -> bool { !has_secure_entropy() }

/* ------------------------------------------------------------------ */
/*  Boot-time seeded handle generation (fallback)                      */
/* ------------------------------------------------------------------ */

/// Seed a capability handle from a boot-time entropy source.
/// Used when RDSEED/RDRAND are unavailable but firmware provides
/// an entropy blob.
///
 /// # Safety
 /// The caller must ensure `seed` contains at least 16 bytes of
 /// cryptographic-quality entropy.
pub unsafe fn handle_from_seed(seed: [u8; 16]) -> CapabilityHandle {
	CapabilityHandle::generate_from_seed(seed)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cspace::CapabilitySpace;
	use crate::object::ObjectId;
	use crate::rights::CapabilityRequest;
	use crate::validate::validate;

	fn handle(value: u8) -> CapabilityHandle { CapabilityHandle::new([value; 16]) }
	fn request(port: u64, rights: Rights) -> CapabilityRequest {
		CapabilityRequest {
			object: ObjectId(port), required_rights: rights, operation: "test",
			caller: 1,
			expected_type: None, expected_rights: None,
		}
	}

	#[test]
	fn validation_enforces_membership_scope_rights_and_lifecycle() {
		let store = CapabilityStore::new();
		let record = store.mint_root(handle(1), CapabilityType::IpcPort {
			port_id: 7, can_send: true, can_recv: false,
		}, Rights::SEND, 10);
		let mut owner = CapabilitySpace::new();
		let slot = owner.insert(record.id, false).unwrap();

		assert!(validate(&store, &owner, 1, slot, &request(7, Rights::SEND), 1).is_ok());
		assert_eq!(validate(&store, &owner, 1, slot, &request(8, Rights::SEND), 1).unwrap_err(), CapabilityError::ObjectMismatch);
		assert_eq!(validate(&store, &owner, 1, slot, &request(7, Rights::RECV), 1).unwrap_err(), CapabilityError::InsufficientRights);
		assert_eq!(validate(&store, &owner, 1, slot, &request(7, Rights::SEND), 10).unwrap_err(), CapabilityError::Expired);
		let foreign = CapabilitySpace::new();
		assert_eq!(validate(&store, &foreign, 2, slot, &request(7, Rights::SEND), 1).unwrap_err(), CapabilityError::BadHandle);
		store.revoke(record.id);
		assert_eq!(validate(&store, &owner, 1, slot, &request(7, Rights::SEND), 1).unwrap_err(), CapabilityError::Revoked);
	}

	#[test]
	fn delegation_is_narrowing_and_revocation_is_transitive() {
		let store = CapabilityStore::new();
		let root = store.mint_root(handle(2), CapabilityType::IpcPort {
			port_id: 9, can_send: true, can_recv: true,
		}, Rights::SEND | Rights::RECV, 100);
		assert_eq!(store.delegate(&root, Rights::SEND | Rights::NOTIFY, 0).unwrap_err(), store::DelegateError::RightsSuperset);
		assert_eq!(store.delegate(&root, Rights::SEND, 101).unwrap_err(), store::DelegateError::ExpiryExceedsParent);
		let child = store.delegate(&root, Rights::SEND, 0).unwrap();
		let grandchild = store.delegate(&child, Rights::SEND, 0).unwrap();
		let great_grandchild = store.delegate(&grandchild, Rights::SEND, 0).unwrap();
		store.revoke(root.id);
		for id in [root.id, child.id, grandchild.id, great_grandchild.id] {
			assert!(store.lookup(id).unwrap().revoked);
		}
	}

	#[test]
	fn reused_slot_has_a_new_generation() {
		let mut space = CapabilitySpace::new();
		let first = space.insert(CapabilityId::new(1), false).unwrap();
		let old_generation = space.get(first).unwrap().1;
		space.remove(first);
		let reused = space.insert(CapabilityId::new(2), false).unwrap();
		assert_eq!(first, reused);
		assert_ne!(old_generation, space.get(reused).unwrap().1);
	}
}
