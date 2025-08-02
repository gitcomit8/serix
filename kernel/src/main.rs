#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(abi_x86_interrupt)]
extern crate alloc;
mod boot;
mod graphics;
mod hal;
mod heap;
mod idt;
mod memory;
mod util;

use crate::graphics::{draw_memory_map, fill_screen_blue};
use crate::heap::{init_heap, StaticBootFrameAllocator};
use limine::request::{FramebufferRequest, MemoryMapRequest};
use limine::BaseRevision;
use x86_64::structures::paging::{FrameAllocator, Mapper, PhysFrame};
use x86_64::{PhysAddr, VirtAddr};

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    idt::init_idt();
    //Access framebuffer info
    let fb_response = FRAMEBUFFER_REQ
        .get_response()
        .expect("No framebuffer reply");

    let mmap_response = MMAP_REQ.get_response().expect("No memory map response");
    let entries = mmap_response.entries();

    //Get kernel physical memory offset
    let phys_mem_offset = VirtAddr::new(0xffff_8000_0000_0000);
    let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };

    //Preallocate all usable frames before heap mapping
    let mut frame_count = 0;
    for region in entries
        .iter()
        .filter(|r| r.entry_type == limine::memory_map::EntryType::USABLE)
    {
        let start = region.base;
        let end = region.base + region.length;
        let start_frame = PhysFrame::containing_address(PhysAddr::new(start));
        let end_frame = PhysFrame::containing_address(PhysAddr::new(end - 1));
        for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
            if frame_count < crate::heap::MAX_BOOT_FRAMES {
                unsafe {
                    crate::heap::BOOT_FRAMES[frame_count] = Some(frame);
                }
                frame_count += 1;
            }
        }
    }

    let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);

    hal::cpu::enable_interrupts();
    //--- HEAP MAP/INIT ---
    init_heap(&mut mapper, &mut frame_alloc);
    //Paint screen blue
    if let Some(fb) = fb_response.framebuffers().next() {
        fill_screen_blue(&fb);
        draw_memory_map(&fb, mmap_response.entries());
    }
    loop {
        hal::cpu::halt();
    }
}
