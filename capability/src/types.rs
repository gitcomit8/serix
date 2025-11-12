use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityHandle {
	pub key: [u8; 16], // 128-bt opaque handle
}

impl CapabilityHandle {
	pub fn new(key: [u8; 16]) -> Self {
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
