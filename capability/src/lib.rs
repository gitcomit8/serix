/*
 * Capability-based Security System
 *
 * Implements object capabilities for fine-grained access control.
 * Capabilities are unforgeable tokens that grant specific rights to resources.
 */

#![no_std]

extern crate alloc;
pub mod store;
pub mod types;

use spin::{Mutex, Once};

pub use store::CapabilityStore;
pub use types::{Capability, CapabilityHandle, CapabilityType};

/* Global capability store - accessible to all crates */
static GLOBAL_CAP_STORE: Once<Mutex<CapabilityStore>> = Once::new();

/*
 * global_cap_store - Get the global capability store
 *
 * Returns a reference to the global capability store, initializing it if needed.
 */
pub fn global_cap_store() -> &'static Mutex<CapabilityStore> {
	GLOBAL_CAP_STORE.call_once(|| Mutex::new(CapabilityStore::new()))
}
