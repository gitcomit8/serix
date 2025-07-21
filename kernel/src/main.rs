#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
use core::alloc::{GlobalAlloc, Layout};
struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        ptr::null_mut()
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
use limine::memory_map::EntryType;
use limine::request::{FramebufferRequest, MemoryMapRequest};
use limine::BaseRevision;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    //Access framebuffer info
    let fb_response = FRAMEBUFFER_REQ
        .get_response()
        .expect("No framebuffer reply");

    let mmap_response = MMAP_REQ.get_response().expect("No memory map response");
    let entries = mmap_response.entries();

    //Paint screen blue
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

        //Visualize mmap - draw pixel mid-screen with color indicating type
        let count = entries.len();
        let max_count = width.min(count);

        //Thick vertical bar at bottom of screen
        let bar_width = width / max_count.max(1);

        for (i, entry) in entries.iter().enumerate() {
            let color = match entry.entry_type {
                EntryType::USABLE => [0x00, 0xFF, 0x00, 0x00], // green
                EntryType::BOOTLOADER_RECLAIMABLE => [0xFF, 0xFF, 0x00, 0x00], // cyan
                _ => [0x80, 0x80, 0x80, 0x00],                 // gray
            };
            let x_start = i * bar_width;
            for x in x_start..(x_start + bar_width) {
                for y in (height - 40)..height {
                    let offset = y * pitch + x * (bpp / 8);
                    unsafe {
                        ptr::copy_nonoverlapping(color.as_ptr(), ptr.add(offset), 4);
                    }
                }
            }
        }
    }
    loop {}
}

//Returns a mutable reference to the active level-4 page table
unsafe fn active_level_table(offset: VirtAddr) -> &'static mut PageTable {
    let (frame, _) = Cr3::read();
    let phys = frame.start_address().as_u64();
    let virt = offset.as_u64() + phys;
    &mut *(virt as *mut PageTable)
}

unsafe fn init_offset_page_table(offset: VirtAddr) -> OffsetPageTable<'static> {
    OffsetPageTable::new(active_level_table(offset), offset)
}

//On panic fall into infinite loop
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
