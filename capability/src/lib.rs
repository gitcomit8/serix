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

/// Whether secure entropy (RDSEED/RDRAND) is available on this CPU.
/// Set during init; read-only after.
static SECURE_ENTROPY_AVAILABLE: Once<bool> = Once::new();

/*
 * has_secure_entropy — Check if secure RNG is available
 *
 /// Returns true if RDSEED or RDRAND is available.
 /// If false, capability handle generation will panic.
 */
pub fn has_secure_entropy() -> bool {
	*SECURE_ENTROPY_AVAILABLE.call_once(|| {
		// Try RDSEED first
		let mut val = 0u64;
		let rdseed_ok =
			unsafe { core::arch::x86_64::_rdseed64_step(&mut val) } == 1;
		if rdseed_ok {
			return true;
		}

		// Try RDRAND
		let rdrand = RdRand::new();
		rdrand.is_some()
	})
}

/*
 * init_entropy — Initialize entropy availability flag
 *
 * Called early in boot before any capability handles are minted.
 * If no secure entropy is available, the kernel should enter a
 * restricted security state and disable untrusted capability handoff.
 */
pub fn init_entropy() {
	let _ = has_secure_entropy();
}

/* ------------------------------------------------------------------ */
/*  RdRand wrapper (from x86_64 crate)                                 */
/* ------------------------------------------------------------------ */

use x86_64::instructions::random::RdRand;

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
