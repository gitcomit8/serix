use crate::Capability;
use alloc::collections::BTreeMap;
use spin::Mutex;

pub struct CapabilityStore {
	capabilities: Mutex<BTreeMap<[u8; 16], Capability>>,
}

impl CapabilityStore {
	pub fn new() -> Self {
		CapabilityStore {
			capabilities: Mutex::new(BTreeMap::new()),
		}
	}

	pub fn add_capability(&self, cap: Capability) -> bool {
		let mut caps = self.capabilities.lock();
		if caps.contains_key(&cap.handle.key) {
			false
		} else {
			caps.insert(cap.handle.key, cap);
			true
		}
	}

	pub fn get_capability(&self, key: &[u8; 16]) -> Option<Capability> {
		let caps = self.capabilities.lock();
		caps.get(key).cloned()
	}
	pub fn remove_capability(&self, key: &[u8; 16]) -> bool {
		let mut caps = self.capabilities.lock();
		caps.remove(key).is_some()
	}
}
