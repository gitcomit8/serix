/*
 * lib.rs - Device drivers subsystem
 *
 * Provides hardware device drivers including:
 * - Console device for VFS
 * - PCI bus enumeration and configuration
 * - VirtIO block device driver
 */

#![no_std]
pub mod console;
pub mod pci;
pub mod virtio;
