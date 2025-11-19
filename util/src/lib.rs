/*
 * Utility Library
 *
 * Provides panic handling and dummy allocator for no_std environment.
 */

#![no_std]
#![feature(alloc_error_handler)]

pub mod panic;

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::ptr;
use linked_list_allocator::LockedHeap;

/*
 * struct Dummy - Dummy global allocator
 *
 * Returns null for all allocations. Used as a placeholder before
 * the real heap allocator is initialized.
 */
pub struct Dummy;

unsafe impl GlobalAlloc for Dummy {
	unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
		ptr::null_mut()
	}
	unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

/*
 * alloc_error_handler - Handle allocation failures
 *
 * Called when memory allocation fails. Enters an infinite loop.
 */
#[alloc_error_handler]
pub fn alloc_error_handler(_: core::alloc::Layout) -> ! {
	loop {}
}
