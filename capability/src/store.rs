/*
 * Capability Store
 *
 * Manages storage and lookup of capability handles using a BTreeMap.
 */

use crate::Capability;
use alloc::collections::BTreeMap;
use spin::Mutex;

/*
 * struct CapabilityStore - Thread-safe capability storage
 * @capabilities: Map from capability key to full capability
 */
pub struct CapabilityStore {
	capabilities: Mutex<BTreeMap<[u8; 16], Capability>>,
}

impl CapabilityStore {
	/*
	 * new - Create a new empty capability store
	 */
	pub fn new() -> Self {
		CapabilityStore {
			capabilities: Mutex::new(BTreeMap::new()),
		}
	}

	/*
	 * add_capability - Add a capability to the store
	 * @cap: Capability to add
	 *
	 * Returns true if added successfully, false if key already exists.
	 */
	pub fn add_capability(&self, cap: Capability) -> bool {
		let mut caps = self.capabilities.lock();
		if caps.contains_key(&cap.handle.key) {
			false
		} else {
			caps.insert(cap.handle.key, cap);
			true
		}
	}

	/*
	 * get_capability - Look up a capability by key
	 * @key: 128-bit capability key
	 *
	 * Returns the capability if found, None otherwise.
	 */
	pub fn get_capability(&self, key: &[u8; 16]) -> Option<Capability> {
		let caps = self.capabilities.lock();
		caps.get(key).cloned()
	}

	/*
	 * remove_capability - Remove a capability from the store
	 * @key: 128-bit capability key
	 *
	 * Returns true if removed, false if not found.
	 */
	pub fn remove_capability(&self, key: &[u8; 16]) -> bool {
		let mut caps = self.capabilities.lock();
		caps.remove(key).is_some()
	}
}
