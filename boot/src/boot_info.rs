/// Memory region type (bootloader-independent enum).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Usable RAM — available for frame allocator.
    Usable = 0,
    /// Reserved — do not use.
    Reserved = 1,
    /// ACPI reclaimable — may be reused after OS loads tables.
    AcpiReclaimable = 2,
    /// Bad memory — defective, do not use.
    Bad = 3,
    /// Bootloader reclaimable — can be freed after boot.
    BootloaderReclaimable = 4,
    /// Unknown — treat as reserved.
    Unknown = 5,
}

/// A single memory region from the bootloader's memory map.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub region_type: MemoryRegionType,
}

/// Framebuffer provided by the bootloader.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Framebuffer {
    pub address: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bits_per_pixel: u32,
    /// For UEFI GOP: pointer to PixelFormat info. For BIOS VBE: unused.
    pub pixel_format_info: Option<&'static [u8]>,
}

/// A single CPU as reported by the bootloader.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CpuInfo {
    pub apic_id: u32,
    pub logical_id: u32,
    pub is_bsp: bool,
    pub extra: u64,
}

/// Main boot information struct — passed in RDI by bootloader.
#[repr(C)]
pub struct BootInfo {
    /// Version of this struct — for forward/backward compatibility.
    pub version: u32,
    /// HHDM (identity map) offset — kernel's higher-half base.
    pub hhdm_offset: u64,
    /// Memory regions for frame allocator.
    pub memory_regions: &'static [MemoryRegion],
    /// Framebuffer (may be None if headless).
    pub framebuffer: Option<&'static Framebuffer>,
    /// CPUs on the system.
    pub cpus: &'static [CpuInfo],
    /// RSDP address for ACPI (0 = none).
    pub rsdp_address: u64,
}

impl BootInfo {
    pub const VERSION: u32 = 1;

    pub fn is_valid(&self) -> bool {
        self.version == BootInfo::VERSION
            && !self.memory_regions.is_empty()
            && !self.cpus.is_empty()
            && self.hhdm_offset != 0
    }
}
