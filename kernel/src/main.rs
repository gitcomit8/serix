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
use loader::LoadableSegment;
use memory::heap::{init_heap, StaticBootFrameAllocator};
use spin::{Mutex, Once};
use task::{init_executor, poll_executor, spawn_task};
use task::{Scheduler, TaskCB};
use util::panic::halt_loop;
use vfs::{INode, RamFile};
use x86_64::instructions::hlt;
use x86_64::registers::rflags::RFlags;
use x86_64::structures::paging::{
	FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
};
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
 * map_segment - Map an ELF segment into a page table
 * @mapper: The mapper for the target address space
 * @allocator: Frame allocator
 * @segment: The segment to map
 * @phys_mem_offset: HHDM offset for copying data
 */
unsafe fn map_segment(
	mapper: &mut impl Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	segment: &LoadableSegment,
	phys_mem_offset: VirtAddr,
) {
	use x86_64::structures::paging::mapper::MapToError;

	let start = segment.virtual_address;
	let end = start + segment.size;
	let start_page = Page::<Size4KiB>::containing_address(start);
	let end_page = Page::<Size4KiB>::containing_address(end - 1u64);

	let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
	if segment.flags.writable {
		flags |= PageTableFlags::WRITABLE;
	}
	if !segment.flags.executable {
		flags |= PageTableFlags::NO_EXECUTE;
	}

	for page in Page::range_inclusive(start_page, end_page) {
		let frame;

		// 1. Allocate a frame speculatively
		let new_frame = allocator.allocate_frame().expect("OOM during segment load");

		// 2. Try to map the page
		match mapper.map_to(page, new_frame, flags, allocator) {
			Ok(flusher) => {
				// Success! Use the new frame
				frame = new_frame;
				flusher.flush();

				// Zero out the new frame
				let ptr =
					(phys_mem_offset + frame.start_address().as_u64()).as_mut_ptr() as *mut u8;
				ptr.write_bytes(0, 4096);
			}
			Err(MapToError::PageAlreadyMapped(existing_frame)) => {
				// FIX: Page exists (shared by code/data segments). Use existing frame.
				frame = existing_frame;
			}
			Err(e) => panic!("Map failed: {:?}", e),
		}

		// 3. Copy Data (Always copy, even if overlapping)
		let frame_virt = phys_mem_offset + frame.start_address().as_u64();
		let ptr = frame_virt.as_mut_ptr() as *mut u8;

		let page_addr = page.start_address().as_u64();
		let seg_addr = start.as_u64();

		let data_start = if page_addr < seg_addr {
			0
		} else {
			page_addr - seg_addr
		};
		let data_end = core::cmp::min(segment.data.len() as u64, (page_addr + 4096) - seg_addr);

		if data_start < data_end {
			let dest_offset = if page_addr < seg_addr {
				seg_addr - page_addr
			} else {
				0
			};
			let count = (data_end - data_start) as usize;

			core::ptr::copy_nonoverlapping(
				segment.data.as_ptr().add(data_start as usize),
				ptr.add(dest_offset as usize),
				count,
			);
		}
	}
}

/*
 * allocate_user_stack - Map a stack for the user process
 * @mapper: User address space mapper
 * @allocator: Frame allocator
 *
 * Maps 16KB of stack at the top of the user address space.
 * Returns the virtual address of the stack top (initial RSP).
 */
unsafe fn allocate_user_stack(
	mapper: &mut impl Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	phys_mem_offset: VirtAddr,
) -> VirtAddr {
	// Define stack top at the end of the lower half canonical address space
	let stack_top = VirtAddr::new(0x0000_7FFF_FFFF_F000);
	let stack_bottom = stack_top - 16384u64; // 16 KB stack

	let start_page = x86_64::structures::paging::Page::containing_address(stack_bottom);
	let end_page = x86_64::structures::paging::Page::containing_address(stack_top - 1u64);

	let flags =
		PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

	for page in x86_64::structures::paging::Page::range_inclusive(start_page, end_page) {
		let frame = allocator.allocate_frame().expect("OOM for user stack");
		if let Ok(map_result) = mapper.map_to(page, frame, flags, allocator) {
			map_result.flush();
		}

		// Zero the stack memory (good practice)
		let frame_virt = phys_mem_offset + frame.start_address().as_u64();
		let ptr = frame_virt.as_mut_ptr() as *mut u8;
		ptr.write_bytes(0, 4096);
	}

	stack_top
}

/*
 * enter_user_mode - Jump to Ring 3
 * @entry_point: Virtual address of the user program entry
 * @stack_pointer: Virtual address of the user stack top
 *
 * Performs the delicate IRETQ dance to switch privilege levels.
 * DOES NOT RETURN.
 */
unsafe fn enter_user_mode(entry_point: VirtAddr, stack_pointer: VirtAddr) -> ! {
	let selectors = gdt::descriptors();

	// 1. Enable Interrupts in User Mode (RFLAGS.IF = 1)
	let rflags = RFlags::INTERRUPT_FLAG.bits();

	// 2. Prepare the stack frame for IRETQ
	// Stack Layout: [SS, RSP, RFLAGS, CS, RIP]
	core::arch::asm!(
	"push {user_ds}",   // SS (User Data Segment)
	"push {rsp}",       // RSP (User Stack Pointer)
	"push {rflags}",    // RFLAGS
	"push {user_cs}",   // CS (User Code Segment)
	"push {rip}",       // RIP (Entry Point)
	"iretq",            // Interrupt Return (Jump to Ring 3)
	user_ds = in(reg) selectors.user_data.0,
	rsp = in(reg) stack_pointer.as_u64(),
	rflags = in(reg) rflags,
	user_cs = in(reg) selectors.user_code.0,
	rip = in(reg) entry_point.as_u64(),
	options(noreturn)
	)
}

unsafe fn map_mmio(
	mapper: &mut impl x86_64::structures::paging::Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	phys_addr: u64,
	virt_addr: VirtAddr,
) {
	use x86_64::structures::paging::{Page, PageTableFlags, PhysFrame};

	let page = Page::containing_address(virt_addr);
	let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
	// MMIO needs to be Writable and Cache Disable (though usually Strong Uncacheable by MTRR)
	let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;

	if let Ok(map_to) = mapper.map_to(page, frame, flags, allocator) {
		map_to.flush();
	}
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
		gdt::init_per_cpu();
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

	let lapic_phys = 0xFEE00000u64;
	let ioapic_phys = 0xFEC00000u64;

	let lapic_virt = phys_mem_offset + lapic_phys;
	let ioapic_virt = phys_mem_offset + ioapic_phys;

	unsafe {
		map_mmio(&mut mapper, &mut frame_alloc, lapic_phys, lapic_virt);
		map_mmio(&mut mapper, &mut frame_alloc, ioapic_phys, ioapic_virt);

		// Tell APIC driver to use these new virtual addresses
		apic::set_bases(lapic_virt.as_u64());
		apic::ioapic::set_base(ioapic_virt.as_u64());
	}

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
	// --- PHASE 4: Loading & Execution ---
	serial_println!("--- Phase 4: User Mode Transition ---");

	// 1. Create a valid ELF executable (Hardcoded for testing)
	// This allows us to test the loader without a disk driver or compiler.
	//
	// Assembly:
	//   mov rax, 0xDEADBEEF  ; Just a marker
	//   jmp $                ; Infinite loop (don't return)
	//
	// This byte array is a valid ELF64-x86-64 binary containing that code.
	let shellcode_elf = include_bytes!("../../target/x86_64-unknown-none/release/examples/init");

	// 2. Write to VFS
	let init_file = vfs::RamFile::new("init");
	init_file.write(0, shellcode_elf);
	serial_println!("Created /init (Size: {} bytes)", init_file.size());

	// 3. Read back from VFS (simulating loading from disk)
	let mut file_buffer = alloc::vec::Vec::new();
	file_buffer.resize(init_file.size(), 0);
	init_file.read(0, &mut file_buffer);

	// 4. Parse ELF
	let image = loader::load_elf(&file_buffer).expect("Failed to parse init ELF");
	serial_println!("ELF Entry Point: {:#x}", image.entry_point.as_u64());

	// 5. Create User Address Space
	let new_pml4_frame =
		unsafe { memory::create_user_page_table(&mut frame_alloc, phys_mem_offset) }
			.expect("Failed to create user page table");

	let mut user_mapper = unsafe { memory::create_mapper(new_pml4_frame, phys_mem_offset) };

	// 6. Map Segments
	for segment in image.segments {
		unsafe {
			map_segment(
				&mut user_mapper,
				&mut frame_alloc,
				&segment,
				phys_mem_offset,
			);
		}
	}
	serial_println!("Segments mapped.");

	// 7. Allocate User Stack
	let user_stack =
		unsafe { allocate_user_stack(&mut user_mapper, &mut frame_alloc, phys_mem_offset) };
	serial_println!("User Stack allocated at {:#x}", user_stack.as_u64());

	// 8. Switch Page Table (CR3)
	unsafe {
		use x86_64::registers::control::{Cr3, Cr3Flags};
		Cr3::write(new_pml4_frame, Cr3Flags::empty());
	}
	serial_println!("Switched CR3 to User Table.");

	let current_rsp: u64;
	unsafe {
		core::arch::asm!("mov {}, rsp", out(reg) current_rsp);
	}
	let stack_addr = VirtAddr::new(current_rsp);

	// Set BOTH TSS (interrupts) and GS (syscalls)
	gdt::set_kernel_stack(stack_addr);
	gdt::set_syscall_stack(stack_addr);

	serial_println!("Jumping to Ring 3...");
	unsafe {
		enter_user_mode(image.entry_point, user_stack);
	}

	/* Main kernel loop: poll executor and halt CPU until next interrupt */
	loop {
		poll_executor();
		hlt();
	}
}
