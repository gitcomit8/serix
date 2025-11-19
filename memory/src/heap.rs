/*
 * Kernel Heap Allocator
 *
 * Provides dynamic memory allocation for the kernel using a linked list allocator.
 * Maps a contiguous virtual address range to physical frames for the heap.
 */

use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{
	FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::VirtAddr;

/* Kernel heap virtual address range */
const HEAP_START: usize = 0x4444_4444_0000;
const HEAP_SIZE: usize = 1024 * 1024;		/* 1 MiB heap */

/* Maximum number of boot frames to pre-allocate */
pub const MAX_BOOT_FRAMES: usize = 65536;

/* Static array of pre-allocated physical frames */
pub static mut BOOT_FRAMES: [Option<PhysFrame>; MAX_BOOT_FRAMES] = [None; MAX_BOOT_FRAMES];

/* Global heap allocator instance */
#[global_allocator]
pub static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

/*
 * init_heap - Initialize the kernel heap
 * @mapper: Page table mapper
 * @frame_allocator: Physical frame allocator
 *
 * Maps virtual pages for the heap to physical frames and initializes the allocator.
 */
pub fn init_heap(
	mapper: &mut OffsetPageTable,
	frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
	let page_range = {
		let heap_start = VirtAddr::new(HEAP_START as u64);
		let heap_end = VirtAddr::new((HEAP_START + HEAP_SIZE - 1) as u64);
		let start_page = Page::containing_address(heap_start);
		let end_page = Page::containing_address(heap_end);
		Page::range_inclusive(start_page, end_page)
	};

	/* Map each page in the heap range */
	for page in page_range {
		let frame = frame_allocator
			.allocate_frame()
			.expect("No frames available");
		let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
		unsafe {
			mapper
				.map_to(page, frame, flags, frame_allocator)
				.expect("Mapping failed")
				.flush();
		}
	}

	/* Initialize the heap allocator */
	unsafe {
		HEAP_ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
	}
}

/*
 * struct StaticBootFrameAllocator - Frame allocator using static array
 * @next: Index of next frame to allocate
 * @limit: Total number of frames available
 *
 * Allocates from the pre-populated BOOT_FRAMES array.
 */
pub struct StaticBootFrameAllocator {
	next: usize,
	limit: usize,
}

impl StaticBootFrameAllocator {
	/*
	 * new - Create a frame allocator
	 * @frame_count: Number of frames available in BOOT_FRAMES
	 */
	pub fn new(frame_count: usize) -> Self {
		StaticBootFrameAllocator {
			next: 0,
			limit: frame_count,
		}
	}
}

unsafe impl FrameAllocator<Size4KiB> for StaticBootFrameAllocator {
	fn allocate_frame(&mut self) -> Option<PhysFrame> {
		while self.next < self.limit {
			unsafe {
				if let Some(frame) = BOOT_FRAMES[self.next].take() {
					self.next += 1;
					return Some(frame);
				}
			}
			self.next += 1;
		}
		None
	}
}
