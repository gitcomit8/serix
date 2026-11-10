# Boot ABI Crate

The `boot` crate defines the bootloader-independent `BootInfo` struct passed from any x86_64 bootloader to the Serix kernel.

## ABI Contract

The bootloader populates a `BootInfo` struct in its own memory and passes a pointer to it in **RDI** when jumping to the kernel entry point:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn _start(boot_info: *const BootInfo) -> !;
```

### Struct Layout

```
+------------------+-------+
| version          | u32   |  Must be BootInfo::VERSION (1)
+------------------+-------+
| hhdm_offset      | u64   | Kernel's higher-half base (e.g. 0xFFFF_8000_0000_0000)
+------------------+-------+
| memory_regions   | &[MemoryRegion] | Physical memory map
+------------------+-------+
| framebuffer      | Option<&Framebuffer> | May be None for headless boot
+------------------+-------+
| cpus             | &[CpuInfo] | All CPUs including BSP
+------------------+-------+
| rsdp_address     | u64   | 0 if no ACPI
+------------------+-------+
```

### MemoryRegion

```
+------------------+-------+
| base             | u64   | Physical start address
+------------------+-------+
| length           | u64   | Size in bytes
+------------------+-------+
| region_type      | u32   | MemoryRegionType enum
+------------------+-------+
```

### MemoryRegionType

| Value | Name                    | Meaning                           |
|-------|-------------------------|-----------------------------------|
| 0     | Usable                  | Available for frame allocator     |
| 1     | Reserved                | Do not use                        |
| 2     | AcpiReclaimable         | Reclaimable after loading ACPI    |
| 3     | Bad                     | Defective memory                  |
| 4     | BootloaderReclaimable   | Freeable after boot               |
| 5     | Unknown                 | Treat as reserved                 |

### Framebuffer

```
+------------------------+-------+
| address                | u64   | Physical address (map via HHDM)
+------------------------+-------+
| width                  | u32   | Pixels wide
+------------------------+-------+
| height                 | u32   | Pixels tall
+------------------------+-------+
| pitch                  | u32   | Bytes per scanline
+------------------------+-------+
| bits_per_pixel         | u32   | Color depth
+------------------------+-------+
| pixel_format_info      | Option<&[u8]> | UEFI GOP PixelFormat or None
+------------------------+-------+
```

### CpuInfo

```
+------------------+-------+
| apic_id          | u32   | LAPIC ID (hardware)
+------------------+-------+
| logical_id       | u32   | 0-based dense index (assigned by bootloader)
+------------------+-------+
| is_bsp           | bool  | True if this is the bootstrap processor
+------------------+-------+
| extra            | u64   | Bootloader-specific data (e.g., AP stack pointer)
+------------------+-------+
```

## Validation

The kernel calls `boot::validate_boot_info(ptr)` which checks:
- Pointer is not null
- Version matches `BootInfo::VERSION`
- At least one memory region exists
- At least one CPU is reported
- HHDM offset is non-zero

Returns `Result<&'static BootInfo, BootError>`.

## Bootloader Integration

### Limine (with bootstub)

Create a `limine_bootstub` that:
1. Receives Limine boot structure in RDI
2. Parses Limine responses (memory map, HHDM, framebuffers, MP info, RSDP)
3. Populates `BootInfo` on the stack or in a known memory location
4. Jumps to kernel `_start(boot_info_ptr)`

### GRUB2 Multiboot2

Translate multiboot2 info structure:
- Memory map → `MemoryRegion` array
- Multiboot2 `mmap` entries → `MemoryRegionType::Usable` / `Reserved`
- Framebuffer info → `Framebuffer`
- CPU information → `CpuInfo` array (detect BSP via `is_bsp` flag)
- HHDM → typically `0xFFFF_8000_0000_0000`

### EDK2 (UEFI)

Populate from UEFI services:
- `GetMemoryMap()` → `MemoryRegion` array
- `HandleProtocol()` on GOP handle → `Framebuffer`
- ACPI RSDP → read from UEFI configuration table or `EfiRuntimeServices`
- Processor information → `CpuInfo` from UEFI ACPI table

### Custom Bare-Metal

Directly populate `BootInfo` at a well-known physical address (e.g., `0x100000`). The bootloader jumps to kernel entry with RDI pointing to this address.

## Version History

| Version | Date       | Changes                              |
|---------|------------|--------------------------------------|
| 1       | 2026-11-10 | Initial release                      |
