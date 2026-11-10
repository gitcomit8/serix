/*
 * bootstub/limine.rs - Limine bootstub
 *
 * Defines Limine request statics (so the Limine bootstrap code
 * populates them) and constructs a BootInfo struct for the kernel
 * entry point. Called by limine_entry.S when Limine boots the kernel.
 */

use core::mem::MaybeUninit;
use boot::{BootInfo, CpuInfo, Framebuffer, MemoryRegion, MemoryRegionType};
use limine::request::{FramebufferRequest, HhdmRequest, MemoryMapRequest, MpRequest, RsdpRequest};
use limine::mp::RequestFlags;

/* Limine request statics — populated by the bootstrap code */
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
static HHDM_REQ: HhdmRequest = HhdmRequest::new();
static FB_REQ: FramebufferRequest = FramebufferRequest::new();
static MP_REQ: MpRequest = MpRequest::new().with_flags(RequestFlags::X2APIC);
static RSDP_REQ: RsdpRequest = RsdpRequest::new();

/*
 * Static arrays for boot data — allocated in BSS.
 * These are the backing storage for BootInfo fields.
 */
static mut REGIONS: [MaybeUninit<MemoryRegion>; 256] =
	[const { MaybeUninit::uninit() }; 256];
static mut CPUS: [MaybeUninit<CpuInfo>; 16] =
	[const { MaybeUninit::uninit() }; 16];
static mut FRAMEBUFFER: MaybeUninit<Framebuffer> = MaybeUninit::uninit();

/*
 * parse_limine_responses - Parse Limine responses into static storage
 */
pub unsafe fn parse_limine_responses() {
	/* Get BSP LAPIC ID */
	let bsp_lapic_id = MP_REQ
		.get_response()
		.as_ref()
		.map(|r| r.bsp_lapic_id())
		.unwrap_or(0);

	/* Get memory map */
	let entries = MMAP_REQ
		.get_response()
		.as_ref()
		.map(|r| r.entries())
		.unwrap_or(&[]);

	/* Get CPUs */
	let mp_cpus = MP_REQ
		.get_response()
		.as_ref()
		.map(|r| r.cpus())
		.unwrap_or(&[]);

	/* Get HHDM offset */
	let hhdm_offset = HHDM_REQ
		.get_response()
		.as_ref()
		.map(|r| r.offset() as u64)
		.unwrap_or(0);

	/* Get framebuffer */
	let fb_opt = FB_REQ
		.get_response()
		.as_ref()
		.and_then(|r| r.framebuffers().next())
		.map(|fb| Framebuffer {
			address: fb.addr() as u64,
			width: fb.width() as u32,
			height: fb.height() as u32,
			pitch: fb.pitch() as u32,
			bits_per_pixel: fb.bpp() as u32,
			pixel_format_info: None,
		});

	/* Get RSDP address */
	let rsdp_address = RSDP_REQ
		.get_response()
		.map(|r| r.address() as u64)
		.unwrap_or(0);

	/* Populate static arrays */
	let region_count = core::cmp::min(entries.len(), 256);
	let cpu_count = core::cmp::min(mp_cpus.len(), 16);

		for (i, entry) in entries.iter().take(region_count).enumerate() {
		let region_type = match entry.entry_type {
			limine::memory_map::EntryType::USABLE => MemoryRegionType::Usable,
			limine::memory_map::EntryType::RESERVED => MemoryRegionType::Reserved,
			limine::memory_map::EntryType::ACPI_RECLAIMABLE => MemoryRegionType::AcpiReclaimable,
			limine::memory_map::EntryType::BAD_MEMORY => MemoryRegionType::Bad,
			limine::memory_map::EntryType::BOOTLOADER_RECLAIMABLE => {
				MemoryRegionType::BootloaderReclaimable
			}
			_ => MemoryRegionType::Unknown,
		};
		unsafe {
			core::ptr::write(
				core::ptr::addr_of_mut!(REGIONS[i]),
				MaybeUninit::new(MemoryRegion {
					base: entry.base,
					length: entry.length,
					region_type,
				}),
			);
		}
	}

	for (i, cpu) in mp_cpus.iter().take(cpu_count).enumerate() {
		unsafe {
			core::ptr::write(
				core::ptr::addr_of_mut!(CPUS[i]),
				MaybeUninit::new(CpuInfo {
					apic_id: cpu.lapic_id,
					logical_id: i as u32,
					is_bsp: cpu.lapic_id == bsp_lapic_id,
					extra: 0,
				}),
			);
		}
	}

	/* Store framebuffer */
	if let Some(fb) = fb_opt {
		unsafe {
			core::ptr::write(core::ptr::addr_of_mut!(FRAMEBUFFER), MaybeUninit::new(fb));
		}
	}

	/* Build BootInfo on the stack (will be used immediately) */
	let regions_ptr = unsafe { core::ptr::addr_of!(REGIONS) as *const MemoryRegion };
	let cpus_ptr = unsafe { core::ptr::addr_of!(CPUS) as *const CpuInfo };
	let fb_ptr = if fb_opt.is_some() {
		unsafe { core::ptr::addr_of!(FRAMEBUFFER) as *const Framebuffer }
	} else {
		core::ptr::null()
	};

	/*
	 * Construct BootInfo and call _start directly.
	 * We use a stack-allocated BootInfo that lives only for this function.
	 */
	let boot_info = BootInfo {
		version: BootInfo::VERSION,
		hhdm_offset,
		memory_regions: core::slice::from_raw_parts(regions_ptr, region_count),
		framebuffer: if fb_opt.is_some() {
			Some(&*fb_ptr)
		} else {
			None
		},
		cpus: core::slice::from_raw_parts(cpus_ptr, cpu_count),
		rsdp_address,
	};

	/* Call _start with the BootInfo pointer */
	let start: extern "C" fn(*const BootInfo) -> ! =
		core::mem::transmute::<_, extern "C" fn(*const BootInfo) -> !>(crate::_start as usize);
	start(&boot_info);
}

/*
 * limine_boot - Limine entry point (called from limine_entry.S)
 */
#[unsafe(no_mangle)]
pub extern "C" fn limine_boot() -> ! {
	unsafe {
		parse_limine_responses();
		/* parse_limine_responses never returns */
		core::hint::unreachable_unchecked();
	}
}
