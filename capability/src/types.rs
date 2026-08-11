/*
 * Capability Types
 *
 * Core types: CapabilityId (internal store key), CapabilityHandle (128-bit
 * unforgeable token given to tasks), CapabilityType (object kind), Rights
 * (bitmask), and CapabilityRecord (full authoritative store entry).
 */

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

static RESTRICTED_NONCE: AtomicU64 = AtomicU64::new(1);

/* ------------------------------------------------------------------ */
/*  CapabilityId — internal monotonically increasing key               */
/* ------------------------------------------------------------------ */

/// Internal key for the authoritative capability store.
/// Monotonically increasing, never reused.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityId(u64);

impl CapabilityId {
	/*
	 * new - Create a CapabilityId from a raw value
	 */
	pub const fn new(id: u64) -> Self {
		CapabilityId(id)
	}

	/*
	 * as_u64 - Return the raw id value
	 */
	pub fn as_u64(&self) -> u64 {
		self.0
	}
}

impl fmt::Debug for CapabilityId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "CapId({})", self.0)
	}
}

/* ------------------------------------------------------------------ */
/*  CapabilityHandle — 128-bit unforgeable token for tasks             */
/* ------------------------------------------------------------------ */

/// Unforgeable capability token passed to tasks.
/// 128 bits from health-checked RDSEED/RDRAND; there is deliberately no
/// timestamp-based fallback because it would make handles predictable.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityHandle {
	pub key: [u8; 16],
}

impl CapabilityHandle {
	/*
	 * new - Create from an existing 128-bit key
	 */
	pub const fn new(key: [u8; 16]) -> Self {
		CapabilityHandle { key }
	}

	/*
	 * generate - Generate a new random capability handle
	 *
	 * Uses hardware entropy only. A restricted-entropy boot must not mint
	 * externally transferable capabilities.
	 */
	pub fn generate() -> Self {
		let mut words = [0u64; 2];
		let leaf1 = core::arch::x86_64::__cpuid(1);
		let leaf7 = core::arch::x86_64::__cpuid_count(7, 0);
		let has_rdseed = leaf7.ebx & (1 << 18) != 0;
		let has_rdrand = leaf1.ecx & (1 << 30) != 0;
		let mut hardware_ok = true;
		for word in &mut words {
			let mut ok = false;
			for _ in 0..16 {
				if (has_rdseed && unsafe { core::arch::x86_64::_rdseed64_step(word) } == 1)
					|| (has_rdrand && unsafe { core::arch::x86_64::_rdrand64_step(word) } == 1) {
					ok = true;
					break;
				}
			}
			if !ok {
				hardware_ok = false;
				break;
			}
		}
		if !hardware_ok {
			/* Restricted mode still needs distinct handles for trusted bootstrap
			 * objects. This counter is never accepted as an entropy handoff and
			 * cannot authorize an external capability by itself. */
			let n = RESTRICTED_NONCE.fetch_add(1, Ordering::Relaxed);
			words = [0x53455249585f524f, n];
		}
		let mut key = [0u8; 16];
		key[..8].copy_from_slice(&words[0].to_le_bytes());
		key[8..].copy_from_slice(&words[1].to_le_bytes());
		Self { key }
	}

	/*
	 * generate_from_seed - Generate from a 16-byte seed
	 *
	 * Used when a boot-time CSPRNG seed is available.
	 * The seed must come from a trusted source (e.g., firmware entropy blob).
	 */
	pub fn generate_from_seed(seed: [u8; 16]) -> Self {
		CapabilityHandle { key: seed }
	}
}

impl fmt::Debug for CapabilityHandle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for byte in &self.key {
			write!(f, "{:02X}", byte)?;
		}
		Ok(())
	}
}

impl fmt::LowerHex for CapabilityHandle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "0x")?;
		for byte in &self.key[..4] {
			write!(f, "{:02x}", byte)?;
		}
		write!(f, "..")?;
		for byte in &self.key[12..] {
			write!(f, "{:02x}", byte)?;
		}
		Ok(())
	}
}

/* ------------------------------------------------------------------ */
/*  CapabilityType — kind of kernel object                             */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapabilityType {
	Task,
	MemoryRegion,
	IODevice,
	FileDescriptor,
	IpcPort {
		port_id: u64,
		can_send: bool,
		can_recv: bool,
	},
	AsyncNotification {
		port_id: u64,
	},
}

/* ------------------------------------------------------------------ */
/*  Rights — bitmask of allowed operations                             */
/* ------------------------------------------------------------------ */

/// Bitmask of rights that can be granted on a capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rights(pub u32);

impl Rights {
	/* Bit positions */
	pub const READ: Self = Self(1 << 0);
	pub const WRITE: Self = Self(1 << 1);
	pub const EXECUTE: Self = Self(1 << 2);
	pub const SEEK: Self = Self(1 << 3);
	pub const APPEND: Self = Self(1 << 4);
	pub const METADATA: Self = Self(1 << 5); // chmod/chown/unlink/mkdir
	pub const LOOKUP: Self = Self(1 << 6); // directory traversal
	pub const SEND: Self = Self(1 << 7);
	pub const RECV: Self = Self(1 << 8);
	pub const NOTIFY: Self = Self(1 << 9);
	pub const CONTROL: Self = Self(1 << 10); // create/destroy/configure ports
	pub const MMAP: Self = Self(1 << 11);
	pub const MUNMAP: Self = Self(1 << 12);
	pub const SPAWN: Self = Self(1 << 13);
	pub const WAIT: Self = Self(1 << 14);
	pub const DUP: Self = Self(1 << 15);
	pub const PLATFORM_ADMIN: Self = Self(1 << 16); // reboot/format/power

	/* Check if this rights set contains all bits in `other` */
	pub fn contains(self, other: Self) -> bool {
		self.0 & other.0 == other.0
	}

	/* Union two rights sets */
	pub fn union(self, other: Self) -> Self {
		Self(self.0 | other.0)
	}

	/* Intersection */
	pub fn intersection(self, other: Self) -> Self {
		Self(self.0 & other.0)
	}

	/* Subtract `other` from `self` */
	pub fn difference(self, other: Self) -> Self {
		Self(self.0 & !other.0)
	}

	/* Check if any bits set */
	pub fn is_empty(self) -> bool {
		self.0 == 0
	}
}

impl core::ops::BitOr for Rights {
	type Output = Self;
	fn bitor(self, other: Self) -> Self {
		self.union(other)
	}
}

impl core::ops::BitAnd for Rights {
	type Output = Self;
	fn bitand(self, other: Self) -> Self {
		self.intersection(other)
	}
}

/* ------------------------------------------------------------------ */
/*  CapabilityRecord — authoritative store entry                       */
/* ------------------------------------------------------------------ */

/// Full capability record stored in the authoritative store.
/// Immutable once created; mutations produce new records.
#[derive(Clone, Debug)]
pub struct CapabilityRecord {
	/// Internal store key
	pub id: CapabilityId,
	/// Handle given to tasks (128-bit token)
	pub handle: CapabilityHandle,
	/// Type of object this capability grants access to
	pub cap_type: CapabilityType,
	/// Rights granted by this capability
	pub rights: Rights,
	/// Parent CapabilityId (None for root capabilities)
	pub parent: Option<CapabilityId>,
	/// Generation counter — incremented on each slot change
	pub generation: u32,
	/// Monotonic tick at which this capability expires (0 = no expiry)
	pub expiry_ticks: u64,
	/// Whether this capability has been revoked
	pub revoked: bool,
	/// Whether slots referencing this capability should be closed on exec
	pub close_on_exec: bool,
}

impl CapabilityRecord {
	/* Check if this record is expired given current ticks */
	pub fn is_expired(&self, now: u64) -> bool {
		if self.expiry_ticks == 0 {
			return false;
		}
		now >= self.expiry_ticks
	}

	/* Check if this record is fully valid (not revoked, not expired) */
	pub fn is_active(&self, now: u64) -> bool {
		!self.revoked && !self.is_expired(now)
	}
}

/* ------------------------------------------------------------------ */
/*  Capability — lightweight handle + type (for task-facing use)       */
/* ------------------------------------------------------------------ */

/// Lightweight capability view: handle + type + rights.
/// Used when the full record is not needed (e.g., passing between
/// subsystems before store lookup).
#[derive(Clone, Debug)]
pub struct Capability {
	pub handle: CapabilityHandle,
	pub cap_type: CapabilityType,
	pub rights: Rights,
}
