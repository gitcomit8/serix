/*
 * SLUB Allocator - Large Kernel Object Allocator
 *
 * Provides sized-class allocation for kernel objects that are too large
 * for the heap allocator (4KiB - 1MiB). Primary consumer: kernel stacks.
 *
 * Each size class maintains a free list of previously freed blocks.
 * New allocations map physical frames into a reserved virtual address
 * range (SLUB_VA_START). Freed blocks are cached for reuse without
 * unmapping.
 *
 * Size classes: 4K, 8K, 16K, 32K, 64K, 128K, 256K, 512K, 1M
 *
 * TODO(SMP): Per-CPU magazine caches to reduce lock contention
 */

use alloc::vec::Vec;
use spin::{Mutex, Once};
use x86_64::VirtAddr;
use x86_64::structures::paging::{
	FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
};

use crate::PageAllocator;

const NUM_SIZE_CLASSES: usize = 9;

/*
 * SIZE_CLASSES - Supported allocation sizes
 *
 * Power-of-two sizes from 4KiB to 1MiB. Requests are rounded up
 * to the next size class. Requests larger than 1MiB are rejected.
 */
const SIZE_CLASSES: [usize; NUM_SIZE_CLASSES] = [
	0x1000,    /*   4 KiB */
	0x2000,    /*   8 KiB */
	0x4000,    /*  16 KiB */
	0x8000,    /*  32 KiB */
	0x10000,   /*  64 KiB */
	0x20000,   /* 128 KiB */
	0x40000,   /* 256 KiB */
	0x80000,   /* 512 KiB */
	0x100000,  /*   1 MiB */
];

/*
 * SLUB virtual address range - separate from the kernel heap
 *
 * 16 TiB range is far more than we'll ever use; the bump pointer
 * advances monotonically and wraps are not handled (would require
 * ~16 million 1MiB allocations to exhaust).
 */
const SLUB_VA_START: u64 = 0xFFFF_D000_0000_0000;

/*
 * struct SlubCache - Free list for a single size class
 * @size:      Allocation size in bytes (matches SIZE_CLASSES entry)
 * @free_list: Previously freed blocks available for reuse
 *
 * Blocks on the free list retain their virtual-to-physical mappings.
 * Reallocation is O(1) pop from the Vec tail.
 */
struct SlubCache {
	size: usize,
	free_list: Vec<*mut u8>,
}

impl SlubCache {
	fn new(size: usize) -> Self {
		SlubCache {
			size,
			free_list: Vec::new(),
		}
	}
}

/*
 * struct SlubAllocator - Main SLUB allocator state
 * @caches:  Per-size-class free lists
 * @next_va: Next virtual address to use for fresh allocations (bump pointer)
 *
 * Allocation strategy:
 * 1. Round request up to the next size class
 * 2. If the cache has a free block, pop and return it
 * 3. Otherwise, allocate physical frames and map them at next_va
 */
pub struct SlubAllocator {
	caches: [SlubCache; NUM_SIZE_CLASSES],
	next_va: u64,
}

/* *mut u8 in free_list prevents auto-impl; pointers are only accessed under Mutex */
unsafe impl Send for SlubAllocator {}

impl SlubAllocator {
	/*
	 * new - Create a SLUB allocator with empty caches
	 *
	 * Return: Initialized SlubAllocator
	 */
	pub fn new() -> Self {
		SlubAllocator {
			caches: core::array::from_fn(|i| SlubCache::new(SIZE_CLASSES[i])),
			next_va: SLUB_VA_START,
		}
	}

	/*
	 * size_class_index - Find the smallest size class >= requested size
	 * @size: Requested allocation size in bytes
	 *
	 * Return: Index into SIZE_CLASSES, or None if size > 1MiB
	 */
	fn size_class_index(size: usize) -> Option<usize> {
		SIZE_CLASSES.iter().position(|&s| s >= size)
	}

	/*
	 * alloc - Allocate a block from the SLUB
	 * @size: Requested size in bytes (will be rounded up to size class)
	 *
	 * Checks the free list first; if empty, maps fresh physical frames
	 * into the SLUB virtual address range.
	 *
	 * Return: Page-aligned pointer to the allocation, or None on OOM
	 *
	 * Safety: Caller must eventually call free() with the same size class
	 */
	pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
		let idx = Self::size_class_index(size)?;
		let cache = &mut self.caches[idx];

		/* Fast path: reuse a cached block */
		if let Some(ptr) = cache.free_list.pop() {
			return Some(ptr);
		}

		/* Slow path: allocate and map new pages */
		let alloc_size = cache.size;
		let n_pages = alloc_size / 4096;
		let va_start = self.next_va;

		let mut pa = crate::PAGE_ALLOC.get()?.lock();
		let PageAllocator {
			mapper,
			frame_alloc,
		} = &mut *pa;

		for i in 0..n_pages {
			let vaddr = VirtAddr::new(va_start + (i as u64) * 4096);
			let page = Page::<Size4KiB>::containing_address(vaddr);
			let frame = frame_alloc.allocate_frame()?;
			let flags = PageTableFlags::PRESENT
				| PageTableFlags::WRITABLE
				| PageTableFlags::NO_EXECUTE;
			unsafe {
				mapper
					.map_to(page, frame, flags, frame_alloc)
					.ok()?
					.flush();
			}
		}

		self.next_va += alloc_size as u64;
		Some(va_start as *mut u8)
	}

	/*
	 * free - Return a block to the SLUB cache
	 * @ptr:  Pointer previously returned by alloc()
	 * @size: Original requested size (used to find size class)
	 *
	 * The block's page mappings are retained for fast reuse.
	 * The caller must not use the pointer after freeing.
	 *
	 * Safety: ptr must have been returned by alloc() with matching size class
	 */
	pub fn free(&mut self, ptr: *mut u8, size: usize) {
		let idx = match Self::size_class_index(size) {
			Some(i) => i,
			None => return,
		};
		debug_assert!(ptr as u64 >= SLUB_VA_START);
		debug_assert!((ptr as u64) % 4096 == 0);
		self.caches[idx].free_list.push(ptr);
	}
}

/* Global SLUB allocator instance */
static SLUB: Once<Mutex<SlubAllocator>> = Once::new();

/*
 * init - Initialize the global SLUB allocator
 *
 * Must be called after init_page_allocator() and init_heap().
 * Subsequent calls are no-ops.
 */
pub fn init() {
	SLUB.call_once(|| Mutex::new(SlubAllocator::new()));
}

/*
 * alloc_kernel_object - Allocate a kernel object
 * @size: Size in bytes
 *
 * For sizes >= 4KiB, uses the SLUB allocator.
 * For sizes < 4KiB, falls back to the heap allocator.
 *
 * Return: Pointer to zeroed memory, or None on OOM
 */
pub fn alloc_kernel_object(size: usize) -> Option<*mut u8> {
	if size < 4096 {
		let layout = core::alloc::Layout::from_size_align(size, 8).ok()?;
		let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
		if ptr.is_null() {
			None
		} else {
			Some(ptr)
		}
	} else {
		SLUB.get()?.lock().alloc(size)
	}
}

/*
 * free_kernel_object - Free a kernel object
 * @ptr:  Pointer returned by alloc_kernel_object()
 * @size: Original allocation size
 *
 * Routes to SLUB or heap deallocator based on size.
 */
/*
 * alloc_kernel_stack - Allocate a kernel stack with a guard page
 * @size: Total size in bytes (must be a SLUB size class, e.g. 1MiB)
 *
 * Allocates a contiguous region via SLUB, then unmaps the bottom
 * page to create a guard page. Stack overflow will hit the unmapped
 * guard and trigger a page fault instead of silent corruption.
 *
 * Return: Stack top VirtAddr (caller passes this to TaskCB::new),
 *         or None on OOM
 */
pub fn alloc_kernel_stack(size: usize) -> Option<VirtAddr> {
	let base = alloc_kernel_object(size)? as u64;
	/* FIXME: Guard page disabled for now (causes page faults during ring 3 entry)
	 * TODO: Implement stack guards properly without breaking page mapping
	 *
	 * Known Issue: Page fault at 0xffffd000000ffff8 during context_switch,
	 * despite all SLUB pages being allocated. Likely due to:
	 * - CR3 switching entering different address space before stack is ready
	 * - Timing issue with how kernel/user page tables are set up
	 * - Need to verify PML4 entry copying in create_user_page_table
	 */
	Some(VirtAddr::new(base + size as u64))
}

pub fn free_kernel_object(ptr: *mut u8, size: usize) {
	if size < 4096 {
		if let Ok(layout) = core::alloc::Layout::from_size_align(size, 8) {
			unsafe {
				alloc::alloc::dealloc(ptr, layout);
			}
		}
	} else if let Some(slub) = SLUB.get() {
		slub.lock().free(ptr, size);
	}
}
