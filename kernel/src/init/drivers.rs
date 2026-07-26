/*
 * init/drivers.rs - Hardware Driver Initialization
 *
 * Provides driver_init() which handles:
 * - PCI bus enumeration
 * - VirtIO block device detection and Phase 1 init
 * - SLUB allocator setup (consumes mapper + allocator)
 * - VirtIO queue setup and interrupt registration
 */

use x86_64::structures::paging::Size4KiB;
use x86_64::VirtAddr;
use drivers::pci;
use drivers::virtio::VirtioBlock;
use hal::serial_println;

/*
 * driver_init - Initialize hardware drivers
 * @phys_mem_offset: HHDM virtual address offset
 * @mapper: Page table mapper (consumed — transferred to global PageAllocator)
 * @allocator: Physical frame allocator (consumed — transferred to global PageAllocator)
 *
 * Enumerates PCI devices, detects VirtIO block devices,
 * initializes the SLUB allocator, and sets up VirtIO queues.
 */
pub unsafe fn driver_init(
	phys_mem_offset: VirtAddr,
	mut mapper: x86_64::structures::paging::mapper::OffsetPageTable<'static>,
	mut allocator: ::memory::heap::StaticBootFrameAllocator,
) {
	serial_println!("--- Driver Initialization ---");

	/* Enumerate PCI bus */
	let devices = pci::enumerate_pci();
	serial_println!("PCI BUS SCANNED: {} devices found", devices.len());
	graphics::fb_println!("PCI: {} devices found", devices.len());

	/* MMIO mapper closure — maps physical bars to virtual addresses */
	let mut mmio_mapper = |phys: u64, size: u64| -> *mut u8 {
		let virt = phys_mem_offset + phys;
		super::memory::map_mmio_range(&mut mapper, &mut allocator, phys, virt, size);
		virt.as_mut_ptr()
	};

	/* Phase 1: PCI discovery + VirtIO feature negotiation (no DMA) */
	for dev in devices {
		if let Some(blk) = VirtioBlock::init(dev, &mut mmio_mapper, phys_mem_offset.as_u64()) {
			drivers::virtio::store_global(blk);
			serial_println!("VirtIO: Phase 1 init complete");
			graphics::fb_println!("VirtIO: block device detected");
		}
	}

	/* Transfer mapper + frame allocator to global PageAllocator for SLUB */
	::memory::init_page_allocator(mapper, allocator);
	::memory::slub::init();

	/* Phase 2: Allocate virtqueues now that SLUB is available */
	drivers::virtio::setup_queues_global();
	drivers::virtio::register_interrupt();
}
