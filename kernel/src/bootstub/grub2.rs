/*
 * bootstub/grub2.rs - GRUB2 Multiboot2 bootstub
 *
 * Parses Multiboot2 information structure tags and constructs a BootInfo
 * struct. Called by grub2_entry.S when GRUB2 loads the kernel.
 *
 * Multiboot2 calling convention:
 *   eax = 0 (multiboot2 magic)
 *   rsi = pointer to multiboot2 info structure
 */

use core::mem::MaybeUninit;
use boot::{BootError, BootInfo, CpuInfo, Framebuffer, MemoryRegion, MemoryRegionType};

/* Multiboot2 tag types */
const MB_TAG_END: u16 = 0;
const MB_TAG_MEMORY_MAP: u16 = 6;
const MB_TAG_FRAMEBUFFER: u16 = 3;
const MB_TAG_ACPI1: u16 = 14;
const MB_TAG_ACPI2: u16 = 15;

/* Multiboot2 memory map entry */
#[repr(C)]
struct MbMemoryMapEntry {
    size: u32,
    base_addr_low: u32,
    base_addr_high: u32,
    length_low: u32,
    length_high: u32,
    type_: u32,
}

impl MbMemoryMapEntry {
    fn base(&self) -> u64 {
        ((self.base_addr_high as u64) << 32) | (self.base_addr_low as u64)
    }
    fn length(&self) -> u64 {
        ((self.length_high as u64) << 32) | (self.length_low as u64)
    }
}

/* Multiboot2 memory map tag */
#[repr(C)]
struct MbMemoryMapTag {
    tag_type: u16,
    tag_size: u16,
    mmap_entry_size: u32,
    mmap_entry_version: u32,
}

/* Framebuffer tag */
#[repr(C)]
struct MbFramebufferTag {
    tag_type: u16,
    tag_size: u16,
    framebuffer_addr: u64,
    framebuffer_pitch: u32,
    framebuffer_width: u32,
    framebuffer_height: u32,
    bpp: u8,
}

/* ACPI1 (RSDP) tag */
#[repr(C)]
struct MbAcpi1Tag {
    tag_type: u16,
    tag_size: u16,
    rsdp: u64,
}

/* ACPI2 (RSDP64) tag */
#[repr(C)]
struct MbAcpi2Tag {
    tag_type: u16,
    tag_size: u16,
    rsdp64: u64,
}

/* Multiboot2 info structure */
#[repr(C)]
pub struct Mb2Info {
    total_size: u32,
}

/* Static storage */
static mut MB_REGIONS: [MaybeUninit<MemoryRegion>; 256] =
    [const { MaybeUninit::uninit() }; 256];
static mut MB_CPUS: [MaybeUninit<CpuInfo>; 16] =
    [const { MaybeUninit::uninit() }; 16];
static mut MB_FRAMEBUFFER: MaybeUninit<Framebuffer> = MaybeUninit::uninit();
static mut MB_REGION_COUNT: usize = 0;
static mut MB_RSDP_ADDRESS: u64 = 0;

/*
 * parse_grub2_info - Parse Multiboot2 tags into static storage
 */
pub unsafe fn parse_grub2_info(info_ptr: *const Mb2Info) -> Result<(), BootError> {
    if info_ptr.is_null() {
        return Err(BootError::NullPointer);
    }
    let info = &*info_ptr;
    let mut ptr = (info_ptr as usize + core::mem::size_of::<Mb2Info>()) as *mut u8;
    let end = ptr.add(info.total_size as usize);

    let mut region_count = 0usize;
    let mut has_framebuffer = false;
    let mut rsdp_address: u64 = 0;

    while ptr < end {
        let tag_type = *(ptr as *const u16);
        let tag_size = *(ptr.add(2) as *const u16) as usize;

        if tag_type == MB_TAG_END {
            break;
        }

        match tag_type {
            MB_TAG_MEMORY_MAP => {
                let mmap_tag = &*(ptr as *const MbMemoryMapTag);
                let entry_ptr = (ptr as usize + core::mem::size_of::<MbMemoryMapTag>()) as *const MbMemoryMapEntry;
                let entry_count = if mmap_tag.mmap_entry_size > 0 {
                    (tag_size - core::mem::size_of::<MbMemoryMapTag>()) / mmap_tag.mmap_entry_size as usize
                } else {
                    0
                };

                for i in 0..entry_count {
                    if region_count >= 256 { break; }
                    let entry = &*entry_ptr.add(i);
                    let base = entry.base();
                    let length = entry.length();
                    if length == 0 { continue; }

                    let region_type = match entry.type_ {
                        1 => MemoryRegionType::Usable,
                        2 => MemoryRegionType::Reserved,
                        3 => MemoryRegionType::AcpiReclaimable,
                        4 => MemoryRegionType::Bad,
                        6 => MemoryRegionType::BootloaderReclaimable,
                        _ => MemoryRegionType::Unknown,
                    };
                    core::ptr::write(
                        core::ptr::addr_of_mut!(MB_REGIONS[region_count]),
                        MaybeUninit::new(MemoryRegion {
                            base,
                            length,
                            region_type,
                        }),
                    );
                    region_count += 1;
                }
            }
            MB_TAG_FRAMEBUFFER => {
                let fb_tag = &*(ptr as *const MbFramebufferTag);
                core::ptr::write(
                    core::ptr::addr_of_mut!(MB_FRAMEBUFFER),
                    MaybeUninit::new(Framebuffer {
                        address: fb_tag.framebuffer_addr,
                        width: fb_tag.framebuffer_width,
                        height: fb_tag.framebuffer_height,
                        pitch: fb_tag.framebuffer_pitch,
                        bits_per_pixel: fb_tag.bpp as u32,
                        pixel_format_info: None,
                    }),
                );
                has_framebuffer = true;
            }
            MB_TAG_ACPI1 => {
                let acpi_tag = &*(ptr as *const MbAcpi1Tag);
                rsdp_address = acpi_tag.rsdp;
            }
            MB_TAG_ACPI2 => {
                let acpi_tag = &*(ptr as *const MbAcpi2Tag);
                rsdp_address = acpi_tag.rsdp64;
            }
            _ => {}
        }

        /* Align to 8 bytes and advance */
        ptr = ptr.add(tag_size);
        let align = ptr.align_offset(8);
        ptr = ptr.add(align);
    }

    if region_count == 0 {
        return Err(BootError::NoMemoryRegions);
    }

    /* Initialize single CPU (BSP) */
    core::ptr::write(
        core::ptr::addr_of_mut!(MB_CPUS[0]),
        MaybeUninit::new(CpuInfo {
            apic_id: 0,
            logical_id: 0,
            is_bsp: true,
            extra: 0,
        }),
    );

    /* Store counters in statics */
    MB_REGION_COUNT = region_count;
    MB_RSDP_ADDRESS = rsdp_address;

    Ok(())
}

/*
 * grub2_boot - GRUB2 entry point (called from grub2_entry.S)
 */
#[unsafe(no_mangle)]
pub extern "C" fn grub2_boot(info_ptr: *const Mb2Info) -> ! {
    use core::arch::asm;

    let result = unsafe { parse_grub2_info(info_ptr) };
    if let Err(e) = result {
        super::serial_println!("[BOOTSTUB] GRUB2 boot failed: {:?}", e);
        loop { unsafe { core::arch::asm!("hlt"); } }
    }

    unsafe {
        let fb_ptr = if MB_REGION_COUNT > 0 && unsafe { core::ptr::addr_of_mut!(MB_FRAMEBUFFER) as usize } != 0 {
            core::ptr::addr_of!(MB_FRAMEBUFFER) as *const Framebuffer
        } else {
            core::ptr::null()
        };

        let boot_info = BootInfo {
            version: BootInfo::VERSION,
            hhdm_offset: 0xFFFF_8000_0000_0000u64 as u64,
            memory_regions: core::slice::from_raw_parts(
                core::ptr::addr_of!(MB_REGIONS) as *const MemoryRegion,
                MB_REGION_COUNT,
            ),
            framebuffer: if fb_ptr.is_null() { None } else { Some(&*fb_ptr) },
            cpus: core::slice::from_raw_parts(
                core::ptr::addr_of!(MB_CPUS) as *const CpuInfo,
                1,
            ),
            rsdp_address: MB_RSDP_ADDRESS,
        };

        let start: extern "C" fn(*const BootInfo) -> ! =
            core::mem::transmute::<_, extern "C" fn(*const BootInfo) -> !>(crate::_start as usize);
        start(&boot_info);
    }
}
