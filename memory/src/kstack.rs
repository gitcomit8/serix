/*
 * Kernel Stack Allocator
 *
 * Provides kernel stacks from a fixed virtual address range that's always
 * mapped in all page tables (kernel and user). This avoids issues with SLUB
 * stacks not being visible in user address spaces.
 *
 * Allocates from a fixed bump pointer in the range 0xFFFF_B000_0000_0000,
 * just below SLUB_VA_START. These addresses are in the shared kernel-upper
 * half and will be inherited by all user page tables via PML4 entry 511.
 */

use spin::Mutex;
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use crate::heap::StaticBootFrameAllocator;

/* Kernel stack region: just below SLUB */
pub const KSTACK_VA_START: u64 = 0xFFFF_B000_0000_0000;
pub const KSTACK_VA_END: u64 = 0xFFFF_D000_0000_0000;
pub const KSTACK_REGION_SIZE: u64 = KSTACK_VA_END - KSTACK_VA_START;

/*
 * struct KStackAllocator - Simple bump allocator for kernel stacks
 * @next_va: Next virtual address to allocate
 *
 * Allocates kernel stacks from a fixed range. The region is pre-mapped
 * by the kernel, so all allocations are immediately visible.
 *
 * Safety: Single-threaded or protected by Mutex at call site.
 */
pub struct KStackAllocator {
	next_va: u64,
}

impl KStackAllocator {
	pub fn new() -> Self {
		KStackAllocator {
			next_va: KSTACK_VA_START,
		}
	}

	/*
	 * alloc - Allocate a kernel stack
	 * @mapper: Page table mapper
	 * @frame_alloc: Frame allocator
	 * @size: Size in bytes (must be page-aligned)
	 *
	 * Returns the stack top address (caller should use this as initial RSP).
	 */
	pub fn alloc(
		&mut self,
		mapper: &mut impl Mapper<Size4KiB>,
		frame_alloc: &mut impl FrameAllocator<Size4KiB>,
		size: usize,
	) -> Option<VirtAddr> {
		if size == 0 || size % 4096 != 0 {
			return None;
		}

		let base = self.next_va;
		if base + size as u64 > KSTACK_VA_END {
			return None; /* Out of stack space */
		}

		/* Map all pages for this stack */
		let n_pages = size / 4096;
		for i in 0..n_pages {
			let vaddr = VirtAddr::new(base + (i as u64) * 4096);
			let page = Page::<Size4KiB>::containing_address(vaddr);
			let frame = frame_alloc.allocate_frame()?;
			let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

			unsafe {
				mapper.map_to(page, frame, flags, frame_alloc).ok()?.flush();
			}
		}

		self.next_va += size as u64;

		/* Return stack top (RSP will point here) */
		Some(VirtAddr::new(base + size as u64))
	}
}

static KSTACK_ALLOC: Mutex<KStackAllocator> = Mutex::new(KStackAllocator {
	next_va: KSTACK_VA_START,
});

/*
 * init_kstack_region - Initialize KSTACK page tables
 *
 * Must be called during kernel init, before spawning user tasks.
 * Allocates one dummy stack to force KSTACK page table structures
 * to be created. This ensures the page tables exist when user PML4s
 * copy the upper-half entries.
 */
pub fn init_kstack_region() -> Option<()> {
	/* Allocate a dummy stack to initialize page table structures */
	let _ = alloc_kernel_stack(1024 * 1024)?;
	Some(())
}

/*
 * alloc_kernel_stack - Public API for allocating a kernel stack
 * @size: Size in bytes (should be 1MB for user stacks)
 *
 * Maps all stack pages into the active page table and returns the stack top.
 * The allocated memory is guaranteed to be visible in all page tables.
 */
pub fn alloc_kernel_stack(size: usize) -> Option<VirtAddr> {
	let mut alloc = KSTACK_ALLOC.lock();

	/* Allocate VA space */
	if size == 0 || size % 4096 != 0 {
		return None;
	}

	let base = alloc.next_va;
	if base + size as u64 > KSTACK_VA_END {
		return None; /* Out of stack space */
	}

	alloc.next_va += size as u64;
	let stack_top = base + size as u64;

	/* Release the alloc lock before taking PAGE_ALLOC lock to avoid deadlock */
	drop(alloc);

	/* Map the pages using PAGE_ALLOC */
	let pa = crate::PAGE_ALLOC.get()?;

	let n_pages = size / 4096;
	for i in 0..n_pages {
		let vaddr = VirtAddr::new(base + (i as u64) * 4096);
		let page = Page::<Size4KiB>::containing_address(vaddr);

		/* Allocate frame in one lock scope */
		let frame = {
			let mut pa_guard = pa.lock();
			pa_guard.frame_alloc.allocate_frame()?
		};

		/* Map page in another lock scope using unsafe to bypass borrow checker */
		{
			let mut pa_guard = pa.lock();
			let flags =
				PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

			unsafe {
				/* Use raw pointers to work around borrow checker limitations */
				let mapper_ptr: *mut OffsetPageTable = &mut pa_guard.mapper as *mut _;
				let frame_alloc_ptr: *mut StaticBootFrameAllocator =
					&mut pa_guard.frame_alloc as *mut _;

				(*mapper_ptr)
					.map_to(page, frame, flags, &mut *frame_alloc_ptr)
					.ok()?
					.flush();
			}
		}
	}

	Some(VirtAddr::new(stack_top))
}
