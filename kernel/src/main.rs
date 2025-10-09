#![no_std]
#![no_main]

use core::panic::PanicInfo;
use graphics::{draw_memory_map, fill_screen_blue};
use hal::serial_println;
use limine::request::{FramebufferRequest, MemoryMapRequest};
use limine::BaseRevision;
use memory::heap::{init_heap, StaticBootFrameAllocator};
use task::{SchedClass, TaskCB, TaskManager};
use util::panic::halt_loop;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
static SCHEDULER: TaskManager = TaskManager::new();

fn idle_task() -> ! {
    serial_println!("Idle task running");
    loop {
        hal::cpu::halt();
    }
}
fn user_task_one() -> ! {
    serial_println!("User task one running");
    loop {
        hal::cpu::halt();
    }
}

#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    serial_println!("[KERNEL PANIC]");
    if let Some(loc) = info.location() {
        serial_println!("Location: {}:{}", loc.file(), loc.line());
    } else {
        serial_println!("Failed to get location information");
    }

    halt_loop();
}
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    hal::init_serial();
    serial_println!("Serix Kernel Starting.....");
    serial_println!("Serial console initialized");

    unsafe {
        apic::enable();
        apic::timer::register_handler(); // Register timer handler before IDT is loaded
    }

    idt::init_idt(); // Setup CPU exception handlers and load IDT

    unsafe {
        apic::timer::init_hardware(); // Initialize timer hardware
    }

    let idle = TaskCB::new(
        "idle",
        VirtAddr::new(idle_task as *const () as u64),
        VirtAddr::new(0xFFFF_FF80_0000_1000),
        SchedClass::Fair(120),
    );
    let task1 = TaskCB::new(
        "idle",
        VirtAddr::new(user_task_one as *const () as u64),
        VirtAddr::new(0xFFFF_FF80_0000_2000),
        SchedClass::Fair(110),
    );

    //SCHEDULER.add_task(idle);
    //SCHEDULER.add_task(task1);

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
        if let Some(task) = SCHEDULER.schedule() {
            serial_println!("Running task: {}", task.name);
            //simply call entr fn
            let task_fn: extern "C" fn() -> ! = unsafe { core::mem::transmute(task.context.rip) };
            task_fn();
        } else {
            // Print timer ticks every so often to show timer is working
            static mut LAST_TICK: u64 = 0;
            let current_tick = apic::timer::ticks();
            unsafe {
                if current_tick > LAST_TICK + 1000 {
                    serial_println!("Timer ticks: {}", current_tick);
                    LAST_TICK = current_tick;
                }
            }
            hal::cpu::halt();
        }
    }
}
