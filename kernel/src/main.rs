#![no_std]
#![no_main]

extern crate alloc;
mod gdt;
mod syscall;

use capability::CapabilityStore;
use core::panic::PanicInfo;
use graphics::console::init_console;
use graphics::{draw_memory_map, fb_println, fill_screen_blue};
use hal::serial_println;
use limine::request::{FramebufferRequest, HhdmRequest, MemoryMapRequest};
use limine::BaseRevision;
use memory::heap::{init_heap, StaticBootFrameAllocator};
use spin::{Mutex, Once};
use task::Scheduler;
use task::{init_executor, poll_executor, spawn_task};
use util::panic::halt_loop;
use x86_64::instructions::hlt;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
static HHDM_REQ: HhdmRequest = HhdmRequest::new();
static CAP_STORE_ONCE: Once<Mutex<CapabilityStore>> = Once::new();

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

pub fn global_cap_store() -> &'static Mutex<CapabilityStore> {
	CAP_STORE_ONCE.call_once(|| Mutex::new(CapabilityStore::new()))
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	hal::init_serial();
	serial_println!("Serix Kernel Starting.....");
	serial_println!("Serial console initialized");

	gdt::init();

	unsafe {
		apic::enable(); // This now also disables PIC
		apic::ioapic::init_ioapic(); // Route IRQs through IOAPIC
		apic::timer::register_handler(); // Register timer handler before IDT is loaded
	}

	idt::init_idt(); // Setup CPU exception handlers and load IDT

	//Init keyboard
	serial_println!("Keyboard ready for input!");

	//Enable interrupts globally
	x86_64::instructions::interrupts::enable();

	unsafe {
		apic::timer::init_hardware(); // Initialize timer hardware
	}

	init_executor();

	//Access framebuffer info
	let fb_response = FRAMEBUFFER_REQ
		.get_response()
		.expect("No framebuffer reply");

	let mmap_response = MMAP_REQ.get_response().expect("No memory map response");
	let entries = mmap_response.entries();

	//Get dynamic offset from Limine
	let hhdm_response = HHDM_REQ.get_response().expect("No HHDM response");
	let phys_mem_offset = VirtAddr::new(hhdm_response.offset());
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
			if frame_count >= memory::heap::MAX_BOOT_FRAMES {
				break;
			}
			unsafe {
				memory::heap::BOOT_FRAMES[frame_count] = Some(frame);
			}
			frame_count += 1;
		}
		if frame_count >= memory::heap::MAX_BOOT_FRAMES {
			break;
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

	let fb = fb_response.framebuffers().next().expect("No framebuffer");
	init_console(
		fb.addr(),
		fb.width() as usize,
		fb.height() as usize,
		fb.pitch() as usize,
	);

	// Initialize global scheduler
	Scheduler::init_global();

	// Test framebuffer console with macros
	fb_println!("Welcome to Serix OS!");
	fb_println!("Memory map entries: {}", mmap_response.entries().len());

	spawn_task(async {
		for i in 0..5 {
			serial_println!("Async task 1 iteration {}", i);
			for _ in 0..10_000_000 {
				core::hint::spin_loop();
			}
			task::yield_now::yield_now().await;
		}
	});
	spawn_task(async {
		for i in 0..5 {
			serial_println!("Async task 2 iteration {}", i);
			for _ in 0..10_000_000 {
				core::hint::spin_loop();
			}
			task::yield_now::yield_now().await;
		}
	});

	loop {
		poll_executor();
		hlt();
	}
}
