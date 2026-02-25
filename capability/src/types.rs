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
#[derive(Clone, Copy, Eq, Hash)]
pub struct CapabilityHandle {
	key: [u8; 16],
}

impl PartialEq for CapabilityHandle {
	/*
	 * Constant-time comparison to prevent timing side channels.
	 * Uses fold with | instead of early-return == to ensure all bytes
	 * are always compared regardless of where a difference is found.
	 */
	fn eq(&self, other: &Self) -> bool {
		let mut diff: u8 = 0;
		for i in 0..16 {
			diff |= self.key[i] ^ other.key[i];
		}
		diff == 0
	}
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
	 * as_bytes - Return the raw key bytes
	 *
	 * Return: Reference to the 128-bit key
	 */
	pub fn as_bytes(&self) -> &[u8; 16] {
		&self.key
	}

	/*
	 * generate - Generate a new random capability handle
	 *
	 * Uses the RDRAND hardware instruction to fill all 16 bytes of the key
	 * with cryptographically random data. Retries on transient RDRAND
	 * failures (carry flag = 0) until success.
	 * Returns a new CapabilityHandle with a unique key.
	 */
	pub fn generate() -> Self {
		let mut key = [0u8; 16];
		let mut i = 0;
		while i < 16 {
			let mut val: u64;
			let mut success: u8;
			/* Retry loop: RDRAND may fail transiently (carry flag = 0) */
			loop {
				unsafe {
					core::arch::asm!(
						"rdrand {val}",
						"setc {success}",
						val = out(reg) val,
						success = out(reg_byte) success,
					);
				}
				if success != 0 {
					break;
				}
			}
			let bytes = val.to_ne_bytes();
			let to_copy = (16 - i).min(8);
			for j in 0..to_copy {
				key[i + j] = bytes[j];
			}
			i += to_copy;
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
