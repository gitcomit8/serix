#![no_std]

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

//Declare global allocator instance
#[global_allocator]
pub static DUMMY: Dummy = Dummy;
pub static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

//Allocation error handler loops infinitely
#[alloc_error_handler]
pub fn alloc_error_handler(_: core::alloc::Layout) -> ! {
    loop {}
}

//Panic handler also loops infinitely
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
