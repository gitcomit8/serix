#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use graphics::{draw_memory_map, fill_screen_blue};
use hal::serial_println;
use limine::request::{FramebufferRequest, MemoryMapRequest};
use limine::BaseRevision;
use memory::heap::{init_heap, StaticBootFrameAllocator};
use task::{SchedClass, Scheduler, TaskCB, TaskManager};
use util::panic::halt_loop;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
static SCHEDULER: TaskManager = TaskManager::new();

/*unsafe extern "C" {
    fn context_switch(old: *mut CPUContext, new: *const CPUContext) -> !;
}*/

unsafe extern "C" fn idle_task() -> ! {
    serial_println!("Idle task started!");
    loop {
        serial_println!("Idle task running");
        for _ in 0..10000000 {
            core::hint::spin_loop();
        }
        serial_println!("Idle task calling yield");
        task::task_yield();
        serial_println!("Idle task returned from yield");
    }
}

unsafe extern "C" fn task1() -> ! {
    serial_println!("Task 1 started!");
    loop {
        serial_println!("Task 1 running");
        for _ in 0..10000000 {
            core::hint::spin_loop();
        }
        task::task_yield();
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
    
    // Initialize global scheduler
    Scheduler::init_global();
    {
        let mut scheduler = Scheduler::global().lock();

        // Allocate proper stacks for tasks (8KB each)
        const STACK_SIZE: usize = 8192;
        
        // Create kernel boot task - this represents the current execution context
        let kernel_stack_mem = alloc::vec![0u8; STACK_SIZE];
        let kernel_stack = VirtAddr::from_ptr(kernel_stack_mem.as_ptr()) + STACK_SIZE as u64;
        core::mem::forget(kernel_stack_mem);
        
        let idle_stack_mem = alloc::vec![0u8; STACK_SIZE];
        let task1_stack_mem = alloc::vec![0u8; STACK_SIZE];

        // Get stack top addresses (stacks grow downward)
        let idle_stack = VirtAddr::from_ptr(idle_stack_mem.as_ptr()) + STACK_SIZE as u64;
        let task1_stack = VirtAddr::from_ptr(task1_stack_mem.as_ptr()) + STACK_SIZE as u64;

        // Prevent stack memory from being deallocated
        core::mem::forget(idle_stack_mem);
        core::mem::forget(task1_stack_mem);

        // Add kernel boot task as first task (will never actually run, just holds boot context)
        let kernel_task = TaskCB::new("kernel_boot", idle_task, kernel_stack, SchedClass::Fair(0));
        scheduler.add_task(kernel_task);
        
        let idle = TaskCB::new("idle", idle_task, idle_stack, SchedClass::Fair(120));
        let task1 = TaskCB::new("task1", task1, task1_stack, SchedClass::Fair(110));
        scheduler.add_task(idle);
        scheduler.add_task(task1);

        serial_println!("Starting scheduler with {} tasks", scheduler.task_count());
    } // Lock is dropped here
    
    // Jump into scheduler - this never returns
    unsafe { Scheduler::start(); }
}
