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

pub use store::CapabilityStore;
pub use types::{Capability, CapabilityHandle, CapabilityType};
