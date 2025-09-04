#![no_std]
#![no_main]
mod boot;

use core::panic::PanicInfo;
use graphics::{draw_memory_map, fill_screen_blue};
use limine::request::{FramebufferRequest, MemoryMapRequest};
use limine::BaseRevision;
use memory::heap::{init_heap, StaticBootFrameAllocator};
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    idt::init_idt(); // Setup CPU exception handlers
    unsafe {
        apic::enable();
    }
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
            if frame_count < memory::heap::MAX_BOOT_FRAMES {
                unsafe {
                    memory::heap::BOOT_FRAMES[frame_count] = Some(frame);
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
