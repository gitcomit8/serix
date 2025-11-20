/*
 * Serix Kernel Main Entry Point
 *
 * This file contains the kernel initialization sequence and main loop.
 * It sets up the GDT, IDT, APIC, memory management, and task execution.
 */

#![no_std]
#![no_main]

extern crate alloc;
mod gdt;
mod syscall;

use capability::CapabilityStore;
use core::panic::PanicInfo;
use drivers::pci;
use drivers::virtio::VirtioBlock;
use graphics::console::init_console;
use graphics::{draw_memory_map, fb_println, fill_screen_blue};
use hal::serial_println;
use limine::request::{FramebufferRequest, HhdmRequest, MemoryMapRequest};
use limine::BaseRevision;
use memory::heap::{init_heap, StaticBootFrameAllocator};
use spin::{Mutex, Once};
use task::{init_executor, poll_executor, spawn_task};
use task::{Scheduler, TaskCB};
use util::panic::halt_loop;
use vfs::{INode, RamFile};
use x86_64::instructions::hlt;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};
/* Limine protocol requests */
static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
static HHDM_REQ: HhdmRequest = HhdmRequest::new();

/* Global capability store */
static CAP_STORE_ONCE: Once<Mutex<CapabilityStore>> = Once::new();

/*
 * panic - Kernel panic handler
 * @info: Panic information containing location and message
 *
 * Handles kernel panics by printing diagnostic information and halting.
 */
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

/*
 * global_cap_store - Get the global capability store
 *
 * Returns a reference to the global capability store, initializing it if needed.
 */
pub fn global_cap_store() -> &'static Mutex<CapabilityStore> {
	CAP_STORE_ONCE.call_once(|| Mutex::new(CapabilityStore::new()))
}

/*
 * _start - Kernel entry point
 *
 * This is the main kernel initialization function called by the bootloader.
 * It initializes all subsystems and enters the main execution loop.
 */
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	hal::init_serial();
	serial_println!("Serix Kernel Starting.....");
	serial_println!("Serial console initialized");

	/* Initialize Global Descriptor Table */
	gdt::init();

	unsafe {
		/* Enable APIC and disable legacy PIC */
		apic::enable();
		/* Route IRQs through IOAPIC */
		apic::ioapic::init_ioapic();
		/* Register timer handler before IDT is loaded */
		apic::timer::register_handler();
	}

	/* Setup CPU exception handlers and load IDT */
	idt::init_idt();

	/* Initialize keyboard */
	serial_println!("Keyboard ready for input!");

	/* Enable interrupts globally */
	x86_64::instructions::interrupts::enable();

	init_executor();

	let core_type = hal::topology::get_core_type();
	serial_println!("CORE TYPE: {:?}", core_type);
	syscall::init_syscalls();
	let cap = capability::CapabilityHandle::generate();
	serial_println!("Generated Secure Capability Handle: {:?}", cap);

	/* Access framebuffer information from Limine */
	let fb_response = FRAMEBUFFER_REQ
		.get_response()
		.expect("No framebuffer reply");

	let mmap_response = MMAP_REQ.get_response().expect("No memory map response");
	let entries = mmap_response.entries();

	/* Get Higher Half Direct Map offset from Limine */
	let hhdm_response = HHDM_REQ.get_response().expect("No HHDM response");
	let phys_mem_offset = VirtAddr::new(hhdm_response.offset());
	let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };

	/*
	 * Preallocate all usable physical frames before heap mapping.
	 * This populates the boot frame allocator with available memory.
	 */
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

	/* Initialize kernel heap with identity-mapped pages */
	init_heap(&mut mapper, &mut frame_alloc);

	/* Paint screen blue and draw memory map visualization */
	if let Some(fb) = fb_response.framebuffers().next() {
		fill_screen_blue(&fb);
		draw_memory_map(&fb, mmap_response.entries());
	}

	/* Initialize framebuffer console for kernel output */
	let fb = fb_response.framebuffers().next().expect("No framebuffer");
	init_console(
		fb.addr(),
		fb.width() as usize,
		fb.height() as usize,
		fb.pitch() as usize,
	);

	serial_println!("--- Phase 3 System Check ---");
	let devices = pci::enumerate_pci();
	serial_println!("PCI BUS SCANNED: {} devices found", devices.len());

	for dev in devices {
		//Check for VirtIO Block Device
		if VirtioBlock::init(dev).is_some() {
			serial_println!(
				"> Driver Loaded: VirtIO Block Device (Bus {}, Slot {})",
				dev.bus,
				dev.device
			);
		}
	}

	let file = RamFile::new("system.log");
	file.write(0, b"Serix Kernel Phase 3 OK");

	let mut read_buf = [0u8; 23];
	file.read(0, &mut read_buf);
	if let Ok(msg) = core::str::from_utf8(&read_buf) {
		serial_println!("VFS Readback: {}", msg);
	}
	serial_println!("--- Phase 4: ELF Loader Check ---");

	let dummy_elf: [u8; 64] = [
		0x7F, 0x45, 0x4C, 0x46, 0x02, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x00, 0x02, 0x00, 0x3E, 0x00, 0x01, 0x00, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00,
		0x00, 0x00, // Entry point 0x12345678
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x38, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x00, 0x00,
	];

	match loader::load_elf(&dummy_elf) {
		Ok(image) => {
			serial_println!("ELF Header Parsed Successfully!");
			serial_println!("Entry Point: {:#x}", image.entry_point.as_u64());
			// Expecting 0x12345678 based on our dummy bytes
			if image.entry_point.as_u64() == 0x12345678 {
				serial_println!("Sanity Check: PASSED");
			} else {
				serial_println!("Sanity Check: FAILED (Wrong Entry Point)");
			}
		}
		Err(e) => serial_println!("ELF Load Error: {}", e),
	}
	/* Initialize global task scheduler */
	Scheduler::init_global();
	Scheduler::global().lock().add_task(TaskCB::running_task());
	serial_println!("Kernel task registered");

	/* Display welcome message */
	fb_println!("Welcome to Serix OS!");
	fb_println!("Memory map entries: {}", mmap_response.entries().len());

	/* Spawn test async tasks to demonstrate cooperative multitasking */
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

	unsafe {
		/* Initialize timer hardware */
		apic::timer::init_hardware();
	}

	/* Main kernel loop: poll executor and halt CPU until next interrupt */
	loop {
		poll_executor();
		hlt();
	}
}
