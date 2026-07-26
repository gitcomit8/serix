/*
 * init/memory.rs - Kernel Memory Initialization
 *
 * Provides init_memory() which handles:
 * - HHDM offset setup from Limine
 * - Page table initialization
 * - Boot frame allocator population
 * - Kernel heap initialization
 * - MMIO region mapping (LAPIC, I/O APIC)
 * - APIC driver configuration
 * - PS/2 keyboard initialization
 *
 * Also provides map_mmio() and map_mmio_range() for ad-hoc MMIO mapping.
 */

use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};
use limine::request::{HhdmRequest, MemoryMapRequest};

/* Limine request for HHDM offset */
static HHDM_REQ: HhdmRequest = HhdmRequest::new();

/* ------------------------------------------------------------------ */
/*  Full memory initialization                                          */
/* ------------------------------------------------------------------ */

/*
 * MemoryInitResult - Return value of init_memory
 *
 * Holds the physical memory offset and the page table resources
 * that driver_init will consume to set up the global PageAllocator.
 */
pub struct MemoryInitResult {
	pub phys_mem_offset: VirtAddr,
	pub mapper: x86_64::structures::paging::mapper::OffsetPageTable<'static>,
	pub frame_alloc: ::memory::heap::StaticBootFrameAllocator,
}

/*
 * init_memory - Initialize kernel memory management
 * @mmap_req: Limine memory map request (for populating frame allocator)
 *
 * Sets up HHDM, page tables, frame allocator, kernel heap, and MMIO regions.
 * Returns the physical memory offset and owned page table resources
 * for driver_init to consume.
 */
pub unsafe fn init_memory(mmap_req: &MemoryMapRequest) -> MemoryInitResult {
	use x86_64::structures::paging::mapper::OffsetPageTable;

	/* Get Higher Half Direct Map offset from Limine */
	let hhdm_response = HHDM_REQ.get_response().expect("No HHDM response");
	let phys_mem_offset = VirtAddr::new(hhdm_response.offset());
	::memory::set_hhdm_offset(phys_mem_offset);

	/* Initialize page table from HHDM offset */
	let mut mapper: OffsetPageTable<'static> = ::memory::init_offset_page_table(phys_mem_offset);

	/* Populate boot frame allocator from memory map */
	let mmap_response = mmap_req.get_response().expect("No memory map response");
	let entries = mmap_response.entries();

	let mut frame_count = 0;
	for region in entries
		.iter()
		.filter(|r| r.entry_type == limine::memory_map::EntryType::USABLE)
	{
		let start = region.base;
		let end = region.base + region.length;
		let start_frame = PhysFrame::containing_address(PhysAddr::new(start));
		let end_frame = PhysFrame::containing_address(PhysAddr::new(end - 1));
		for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
			if frame_count >= ::memory::heap::MAX_BOOT_FRAMES {
				break;
			}
			::memory::heap::BOOT_FRAMES[frame_count] = Some(frame);
			frame_count += 1;
		}
		if frame_count >= ::memory::heap::MAX_BOOT_FRAMES {
			break;
		}
	}

	let mut frame_alloc = ::memory::heap::StaticBootFrameAllocator::new(frame_count);
	hal::cpu::enable_interrupts();

	/* Initialize kernel heap with identity-mapped pages */
	::memory::heap::init_heap(&mut mapper, &mut frame_alloc);

	/* Map MMIO regions for LAPIC and I/O APIC */
	let lapic_phys = 0xFEE00000u64;
	let ioapic_phys = 0xFEC00000u64;
	let lapic_virt = phys_mem_offset + lapic_phys;
	let ioapic_virt = phys_mem_offset + ioapic_phys;

	map_mmio(&mut mapper, &mut frame_alloc, lapic_phys, lapic_virt);
	map_mmio(&mut mapper, &mut frame_alloc, ioapic_phys, ioapic_virt);

	/* Tell APIC driver to use new virtual addresses */
	apic::set_bases(lapic_virt.as_u64());
	apic::ioapic::set_base(ioapic_virt.as_u64());
	apic::ioapic::init_ioapic();

	/* Initialize PS/2 keyboard controller */
	super::cpu::init_ps2_keyboard();

	MemoryInitResult {
		phys_mem_offset,
		mapper,
		frame_alloc,
	}
}

/* ------------------------------------------------------------------ */
/*  MMIO mapping helpers                                                */
/* ------------------------------------------------------------------ */

/*
 * map_mmio - Map a single MMIO page into the page table
 * @mapper:     Page table mapper
 * @allocator:  Physical frame allocator
 * @phys_addr:  Physical address of the MMIO region
 * @virt_addr:  Virtual address to map at (via HHDM)
 *
 * Maps one 4 KiB page with PRESENT | WRITABLE | NO_CACHE flags.
 */
pub unsafe fn map_mmio(
	mapper: &mut impl Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	phys_addr: u64,
	virt_addr: VirtAddr,
) {
	let page = Page::containing_address(virt_addr);
	let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
	let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;

	if let Ok(map_to) = mapper.map_to(page, frame, flags, allocator) {
		map_to.flush();
	}
}

/*
 * map_mmio_range - Map a contiguous range of MMIO pages
 * @phys_start: Physical start address
 * @virt_start: Virtual start address (via HHDM)
 * @size:       Size in bytes (must be page-aligned)
 *
 * Iterates over all frames in the range and maps them with NO_CACHE.
 */
pub unsafe fn map_mmio_range(
	mapper: &mut impl Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	phys_start: u64,
	virt_start: VirtAddr,
	size: u64,
) {
	let start_frame = PhysFrame::containing_address(PhysAddr::new(phys_start));
	let end_frame = PhysFrame::containing_address(PhysAddr::new(phys_start + size - 1));

	for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
		let offset = frame.start_address().as_u64() - phys_start;
		let page = Page::containing_address(virt_start + offset);

		let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;

		// Ignore error if already mapped (e.g., at page boundary)
		if let Ok(map_to) = mapper.map_to(page, frame, flags, allocator) {
			map_to.flush();
		}
	}
}
