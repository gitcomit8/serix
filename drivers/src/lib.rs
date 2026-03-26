/*
 * lib.rs - Device drivers subsystem
 *
 * Provides hardware device drivers including:
 * - Console device for VFS
 * - PCI bus enumeration and configuration
 * - VirtIO block device driver
 */

#![feature(abi_x86_interrupt)]
#![no_std]
extern crate alloc;
pub mod console;
pub mod pci;
pub mod block;
pub mod virtio;
pub mod virtqueue;
