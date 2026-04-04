/*
 * heap.rs - Bump allocator for userspace processes
 *
 * Provides a simple 64 KiB bump allocator backed by a static array.
 * Dealloc is a no-op; memory is never reclaimed. Suitable for short-lived
 * processes like rsh where total allocation is bounded.
 */

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

const HEAP_SIZE: usize = 65536;

#[repr(align(16))]
struct HeapStorage([u8; HEAP_SIZE]);

static mut HEAP: HeapStorage = HeapStorage([0u8; HEAP_SIZE]);
static HEAP_POS: AtomicUsize = AtomicUsize::new(0);

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		loop {
			let pos = HEAP_POS.load(Ordering::Relaxed);
			let aligned = (pos + layout.align() - 1) & !(layout.align() - 1);
			let new_pos = aligned + layout.size();
			if new_pos > HEAP_SIZE {
				return core::ptr::null_mut();
			}
			if HEAP_POS
				.compare_exchange(pos, new_pos, Ordering::SeqCst, Ordering::Relaxed)
				.is_ok()
			{
				return unsafe { core::ptr::addr_of_mut!(HEAP).cast::<u8>().add(aligned) };
			}
		}
	}

	unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
		/* bump allocator: dealloc is a no-op */
	}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;
