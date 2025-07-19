#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

use core::alloc::{GlobalAlloc, Layout};

// This is needed even if your code doesn't allocate, but dependencies ask for a global allocator.
struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static DUMMY: Dummy = Dummy;

#[alloc_error_handler]
fn on_oom(_layout: core::alloc::Layout) -> ! {
    loop {}
}

use core::panic::PanicInfo;
use core::ptr;
use limine::request::FramebufferRequest;
use limine::BaseRevision;

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let fb_response = FRAMEBUFFER_REQ
        .get_response()
        .expect("No framebuffer reply");

    if let Some(fb) = fb_response.framebuffers().next() {
        let width = fb.width() as usize;
        let height = fb.height() as usize;
        let pitch = fb.pitch() as usize;
        let bpp = fb.bpp() as usize;

        let ptr = fb.addr() as *mut u8;
        let blue_pixel = [0xFF, 0x00, 0x00, 0x00]; // BGRA, 32 bits

        for y in 0..height {
            for x in 0..width {
                let offset = y * pitch + x * (bpp / 8);
                unsafe {
                    ptr::copy_nonoverlapping(blue_pixel.as_ptr(), ptr.add(offset), 4);
                }
            }
        }
    }
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
