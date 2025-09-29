#![no_std]
#![feature(alloc_error_handler)]

pub mod panic;

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::ptr;
use linked_list_allocator::LockedHeap;

//Dummy global allocator that returns null
pub struct Dummy;
unsafe impl GlobalAlloc for Dummy {
	unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
		ptr::null_mut()
	}
	unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

//Allocation error handler loops infinitely
#[alloc_error_handler]
pub fn alloc_error_handler(_: core::alloc::Layout) -> ! {
	loop {}
}

//Panic handler also loops infinitely
