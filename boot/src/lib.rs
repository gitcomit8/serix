//! Boot ABI crate — bootloader-independent information passed to the kernel.
//!
//! This crate defines the `BootInfo` struct and related types that the bootloader
//! populates before jumping to the kernel entry point. It has **zero dependencies**
//! — no `limine`, no `x86_64`, no external crates.

#![no_std]

pub mod boot_info;
pub mod validation;

pub use boot_info::{BootInfo, CpuInfo, Framebuffer, MemoryRegion, MemoryRegionType};
pub use validation::{validate_boot_info, BootError};
