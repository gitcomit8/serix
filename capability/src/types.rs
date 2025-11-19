use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityHandle {
	pub key: [u8; 16], // 128-bt opaque handle
}

impl CapabilityHandle {
	pub fn new(key: [u8; 16]) -> Self {
		CapabilityHandle { key }
	}

	fn generate() -> Self {
		// Seed using CPU timestamp counter
		let mut seed = unsafe { core::arch::x86_64::_rdtsc() };

		// Simple Xorshift64
		let rng = |s: &mut u64| {
			*s ^= *s << 13;
			*s ^= *s >> 17;
			*s ^= *s << 5;
			*s
		};

		let mut key = [0u8; 16];
		// Generate 128 bits (2 u64s)
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapabilityType {
	Task,
	MemoryRegion,
	IODevice,
	FileDescriptor,
}

#[derive(Clone, Debug)]
pub struct Capability {
	pub cap_type: CapabilityType,
	pub handle: CapabilityHandle,
}
