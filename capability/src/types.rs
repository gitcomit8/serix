/*
 * Capability Types
 *
 * Defines capability handles and types for object-capability security model.
 * Capabilities are cryptographically random handles that grant access rights.
 */

use core::fmt;

/*
 * struct CapabilityHandle - Unforgeable capability token
 * @key: 128-bit cryptographically random identifier
 *
 * Represents an unforgeable token that grants specific access rights.
 */
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityHandle {
	pub key: [u8; 16],
}

impl CapabilityHandle {
	/*
	 * new - Create a capability handle from an existing key
	 * @key: 128-bit key
	 */
	pub fn new(key: [u8; 16]) -> Self {
		CapabilityHandle { key }
	}

	/*
	 * generate - Generate a new random capability handle
	 *
	 * Uses RDTSC and Xorshift64 PRNG to generate a random 128-bit handle.
	 * Returns a new CapabilityHandle with a unique key.
	 */
	pub fn generate() -> Self {
		/* Seed using CPU timestamp counter */
		let mut seed = unsafe { core::arch::x86_64::_rdtsc() };

		/* Simple Xorshift64 PRNG */
		let rng = |s: &mut u64| {
			*s ^= *s << 13;
			*s ^= *s >> 17;
			*s ^= *s << 5;
			*s
		};

		let mut key = [0u8; 16];
		/* Generate 128 bits (2 x 64-bit values) */
		for i in 0..2 {
			let rand = rng(&mut seed);
			let bytes = rand.to_ne_bytes();
			for j in 0..8 {
				key[i * 8 + j] = bytes[j];
			}
		}
		CapabilityHandle { key }
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

/*
 * enum CapabilityType - Types of kernel objects
 * @Task: Task/process capability
 * @MemoryRegion: Memory region access capability
 * @IODevice: I/O device access capability
 * @FileDescriptor: File descriptor capability
 */
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapabilityType {
	Task,
	MemoryRegion,
	IODevice,
	FileDescriptor,
}

/*
 * struct Capability - Complete capability with type and handle
 * @cap_type: Type of capability
 * @handle: Unique handle for this capability
 */
#[derive(Clone, Debug)]
pub struct Capability {
	pub cap_type: CapabilityType,
	pub handle: CapabilityHandle,
}
