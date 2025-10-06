#![no_std]
#![no_main]

mod boot;

use core::panic::PanicInfo;
use graphics::{draw_memory_map, fill_screen_blue};
use hal::serial_println;
use memory::heap::{StaticBootFrameAllocator, init_heap};
use multiboot2::BootInformation;
use util::panic::halt_loop;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};



#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[KERNEL PANIC]");
    if let Some(loc) = info.location() {
        serial_println!("Location: {}:{}", loc.file(), loc.line());
    } else {
        serial_println!("Failed to get location information");
    }
    halt_loop();
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(multiboot_magic: u32, multiboot_info_addr: u32) -> ! {
    hal::init_serial();
    serial_println!("Serix Kernel Starting...");
    serial_println!("Serial console initialized");

    if multiboot_magic != multiboot2::MAGIC {
        panic!("Invalid multiboot magic number");
    }

    let boot_info = unsafe {
        BootInformation::load(multiboot_info_addr as *const _)
            .expect("Failed to load multiboot info")
    };

    idt::init_idt();

    unsafe {
        apic::enable();
        apic::timer::init();
        apic::timer::init_timer_periodic();
    }

    let mut frame_count = 0;
    if let Some(memory_map_tag) = boot_info.memory_map_tag() {
        for region in memory_map_tag.memory_areas() {
            if <multiboot2::MemoryAreaTypeId as Into<u32>>::into(region.typ()) == 1u32 {
                let start_frame = PhysFrame::containing_address(PhysAddr::new(region.start_address()));
                let end_frame = PhysFrame::containing_address(PhysAddr::new(region.end_address() - 1));
                for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
                    if frame_count < memory::heap::MAX_BOOT_FRAMES {
                        unsafe { memory::heap::BOOT_FRAMES[frame_count] = Some(frame); }
                        frame_count += 1;
                    }
                }
            }
        }
    }
    let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);

    hal::cpu::enable_interrupts();
    let phys_mem_offset = VirtAddr::new(0xffff_8000_0000_0000);
    let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };


    init_heap(&mut mapper, &mut frame_alloc);

    let fb_tag = match boot_info.framebuffer_tag() {
        Some(Ok(fb)) => fb,
        Some(Err(e)) => panic!("Framebuffer found but unsupported: {:?}", e),
        None => panic!("Framebuffer tag not present"),
    };


    fill_screen_blue(&fb_tag);

    if let Some(memory_map_tag) = boot_info.memory_map_tag() {
        draw_memory_map(&fb_tag, memory_map_tag.memory_areas().iter().cloned());
    }

    loop {
        hal::cpu::halt();
    }
}
