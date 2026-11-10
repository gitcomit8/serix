/*
 * bootstub/edk2.rs - EDK2/UEFI bootstub
 *
 * Called when the kernel is loaded as a UEFI application.
 * Queries UEFI boot services for memory map, GOP, and ACPI info
 * and constructs a BootInfo struct.
 *
 * UEFI application entry point:
 *   ImageHandle = first arg (RDI)
 *   SystemTable = second arg (RSI)
 *
 * Note: This is a stub. Full EDK2 support requires the uefi crate
 * and building as a .efi file. For now, this provides the
 * BootInfo structure layout and validation.
 */

use boot::{BootError, BootInfo, CpuInfo, Framebuffer, MemoryRegion, MemoryRegionType};

/*
 * edk2_boot - UEFI application entry point (stub)
 *
 * In a full implementation, this would use the `uefi` crate to:
 * 1. Call GetMemoryMap() to build memory regions
 * 2. Query GOP for framebuffer info
 * 3. Read ACPI RSDP from UEFI configuration tables
 *
 * For now, this returns an error indicating the stub is incomplete.
 */
pub extern "C" fn edk2_boot(_image_handle: *mut u8, _system_table: *const u8) -> ! {
    super::serial_println!("[BOOTSTUB] EDK2/UEFI bootstub is a placeholder");
    super::serial_println!("[BOOTSTUB] Full support requires the uefi crate and .efi build");

    /*
     * TODO: Implement full UEFI bootstub:
     *
     * use uefi::prelude::*;
     * use uefi::table::boot::MemoryType;
     *
     * fn parse_uefi_memory_map() -> &'static [MemoryRegion] { ... }
     * fn parse_uefi_gop() -> Option<Framebuffer> { ... }
     * fn parse_uefi_acpi() -> u64 { ... }
     *
     * Then call _start with the constructed BootInfo.
     */

    /* Halt for now */
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}
