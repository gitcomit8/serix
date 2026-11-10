use crate::boot_info::BootInfo;

/// Error from boot info validation.
#[derive(Debug)]
pub enum BootError {
    /// Null pointer passed as boot info.
    NullPointer,
    /// Version mismatch — bootloader and kernel disagree on struct layout.
    VersionMismatch { expected: u32, got: u32 },
    /// No usable memory regions in memory map.
    NoMemoryRegions,
    /// No CPUs reported.
    NoCpus,
    /// HHDM offset is zero.
    ZeroHhdm,
}

impl core::fmt::Display for BootError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BootError::NullPointer => write!(f, "boot info pointer is null"),
            BootError::VersionMismatch { expected, got } => {
                write!(f, "version mismatch: expected {}, got {}", expected, got)
            }
            BootError::NoMemoryRegions => write!(f, "no memory regions in map"),
            BootError::NoCpus => write!(f, "no CPUs reported"),
            BootError::ZeroHhdm => write!(f, "HHDM offset is zero"),
        }
    }
}

/*
 * validate_boot_info - Validate and map boot info from bootloader
 * @ptr: Physical address of BootInfo struct (set by bootloader in RDI)
 *
 * Checks that the pointer is valid, the version matches, and all fields
 * are sane. Returns a &'static BootInfo mapped into kernel virtual address
 * space using HHDM (Higher Half Direct Map).
 *
 * The HHDM offset is read from the first valid memory region's base address
 * heuristic — or the caller must set up HHDM before calling this function.
 * For now, we simply cast the physical address to a virtual address assuming
 * the bootloader placed BootInfo within a region accessible via identity mapping
 * or the caller ensures HHDM is set up before this call.
 */
pub fn validate_boot_info(ptr: *const BootInfo) -> Result<&'static BootInfo, BootError> {
    if ptr.is_null() {
        return Err(BootError::NullPointer);
    }

    let info = unsafe { &*ptr };

    if info.version != BootInfo::VERSION {
        return Err(BootError::VersionMismatch {
            expected: BootInfo::VERSION,
            got: info.version,
        });
    }

    if info.memory_regions.is_empty() {
        return Err(BootError::NoMemoryRegions);
    }

    if info.cpus.is_empty() {
        return Err(BootError::NoCpus);
    }

    if info.hhdm_offset == 0 {
        return Err(BootError::ZeroHhdm);
    }

    Ok(info)
}
