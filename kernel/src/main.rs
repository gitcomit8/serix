/*
 * Serix Kernel Main Entry Point
 *
 * This file contains the kernel initialization sequence and main loop.
 * It sets up the GDT, IDT, APIC, memory management, and task execution.
 */

#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

extern crate alloc;
pub mod fd;
mod gdt;
mod kshell;
pub mod pipe;
pub mod process;
pub mod stdio;
mod syscall;

use capability::CapabilityStore;
use core::panic::PanicInfo;
use drivers::pci;
use drivers::virtio::VirtioBlock;
use graphics::{draw_memory_map, fb_println, fill_screen_blue};
use hal::serial_println;
use limine::BaseRevision;
use limine::request::{FramebufferRequest, HhdmRequest, MemoryMapRequest};
use loader::LoadableSegment;
use memory::heap::{StaticBootFrameAllocator, init_heap};
use spin::{Mutex, Once};
use task::{Scheduler, TaskCB};
use task::init_executor;
use util::panic::halt_loop;
use vfs::INode;
use x86_64::instructions::hlt;
use x86_64::structures::idt::InterruptStackFrame;
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
 * keyboard_interrupt_handler - Handle keyboard interrupts (IRQ 1, vector 33)
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Reads scancode from keyboard controller and sends EOI to APIC.
 * Defined here (not in idt module) to avoid circular dependency with apic.
 */
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
	use x86_64::instructions::port::Port;

	/* Read scancode from keyboard data port (0x60) */
	let mut port = Port::new(0x60);
	let scancode: u8 = unsafe { port.read() };

	/* Process the scancode via keyboard module */
	keyboard::handle_scancode(scancode);

	/* Send End of Interrupt to Local APIC */
	unsafe {
		apic::send_eoi();
	}
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
unsafe fn enter_user_mode(
	entry_point: VirtAddr,
	stack_pointer: VirtAddr,
	user_pml4: x86_64::structures::paging::PhysFrame,
) -> ! {
	use x86_64::registers::rflags::RFlags;

	let selectors = gdt::descriptors();
	let rflags = RFlags::INTERRUPT_FLAG.bits();

	// Cast to u64 to ensure proper stack push size
	let user_ss = selectors.user_data.0 as u64;
	let user_cs = selectors.user_code.0 as u64;

	core::arch::asm!(
	"mov cr3, {cr3}",   // 1. Switch Page Table
	"push {user_ss}",   // 2. Push Stack Segment
	"push {rsp}",       // 3. Push Stack Pointer
	"push {rflags}",    // 4. Push RFLAGS
	"push {user_cs}",   // 5. Push Code Segment
	"push {rip}",       // 6. Push Instruction Pointer
	"iretq",            // 7. Jump!

	cr3     = in(reg) user_pml4.start_address().as_u64(),
	user_ss = in(reg) user_ss,
	rsp     = in(reg) stack_pointer.as_u64(),
	rflags  = in(reg) rflags,
	user_cs = in(reg) user_cs,
	rip     = in(reg) entry_point.as_u64(),
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

unsafe fn map_mmio_range(
	mapper: &mut impl Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	phys_start: u64,
	virt_start: VirtAddr,
	size: u64,
) {
	use x86_64::structures::paging::{Page, PageTableFlags, PhysFrame};

	let start_frame = PhysFrame::containing_address(PhysAddr::new(phys_start));
	let end_frame = PhysFrame::containing_address(PhysAddr::new(phys_start + size - 1));

	// Iterate over all frames in the range
	for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
		let offset = frame.start_address().as_u64() - phys_start;
		let page = Page::containing_address(virt_start + offset);

		let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;

		// Ignore error if already mapped (e.g., if we mapped a page boundary previously)
		if let Ok(map_to) = mapper.map_to(page, frame, flags, allocator) {
			map_to.flush();
		}
	}
}

/*
 * init_ps2_keyboard - Initialize PS/2 keyboard controller
 *
 * Enables the PS/2 keyboard and clears any stale data in the buffer.
 * Required for keyboard interrupts to function properly.
 */
unsafe fn init_ps2_keyboard() {
	use x86_64::instructions::port::Port;

	let mut cmd_port = Port::new(0x64); // PS/2 command port
	let mut data_port = Port::new(0x60); // PS/2 data port

	// Flush output buffer
	let _ = data_port.read();

	// Enable keyboard (command 0xAE)
	cmd_port.write(0xAE_u8);

	// Read controller configuration
	cmd_port.write(0x20_u8);
	for _ in 0..1000 {
		let status: u8 = cmd_port.read();
		if status & 0x01 != 0 {
			break;
		}
	}
	let mut config: u8 = data_port.read();

	// Enable keyboard interrupt (bit 0), keep scancode translation enabled (bit 6)
	config |= 0x01; // Enable keyboard interrupt
	config |= 0x40; // Enable scancode translation (Set 2 → Set 1)

	// Write configuration back
	cmd_port.write(0x60_u8);
	data_port.write(config);

	serial_println!("[PS/2] Keyboard controller initialized");
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
	serial_println!("Serix Kernel v0.0.6 Starting.....");

	/* Initialize Global Descriptor Table */
	gdt::init();
	task::register_switch_hook(|kstack| {
		gdt::set_kernel_stack(kstack);
		gdt::set_syscall_stack(kstack);
	});

	unsafe {
		gdt::init_per_cpu();
		hal::cpu::enable_sse();
		/* Enable APIC and disable legacy PIC */
		apic::enable();
		/* I/O APIC initialization moved to after virtual address remapping */
		/* Register interrupt handlers before IDT is loaded */
		apic::timer::register_handler();
		/* Register keyboard handler (defined in this module to avoid circular deps) */
		idt::register_interrupt_handler(33, keyboard_interrupt_handler);
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
	memory::set_hhdm_offset(phys_mem_offset);
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

		// Now configure I/O APIC with virtual address
		apic::ioapic::init_ioapic();

		// Initialize PS/2 keyboard controller
		init_ps2_keyboard();
	}

	/* Paint screen blue and draw memory map visualization */
	if let Some(fb) = fb_response.framebuffers().next() {
		fill_screen_blue(&fb);
		draw_memory_map(&fb, mmap_response.entries());
	}

	// Initialize frambuffer TTY
	if let Some(fb_response) = FRAMEBUFFER_REQ.get_response() {
		if let Some(fb) = fb_response.framebuffers().next() {
			unsafe {
				graphics::init_console(
					fb.addr(),
					fb.width() as usize,
					fb.height() as usize,
					fb.pitch() as usize,
				);
			}
		}
	}
	graphics::kprintln!("Serix OS v0.0.6");
	graphics::kprintln!("");

	fb_println!("--- System Check ---");
	serial_println!("--- Phase 3 System Check ---");
	let devices = pci::enumerate_pci();
	serial_println!("PCI BUS SCANNED: {} devices found", devices.len());
	fb_println!("PCI: {} devices found", devices.len());

	let mut mmio_mapper = |phys: u64, size: u64| -> *mut u8 {
		let virt = phys_mem_offset + phys;

		// Actually map the physical frames to the virtual HHDM address
		unsafe {
			map_mmio_range(&mut mapper, &mut frame_alloc, phys, virt, size);
		}

		virt.as_mut_ptr()
	};

	for dev in devices {
		/* Phase 1: PCI discovery + feature negotiation (no DMA alloc) */
		if let Some(blk) = unsafe {
			VirtioBlock::init(
				dev, &mut mmio_mapper, phys_mem_offset.as_u64(),
			)
		} {
			drivers::virtio::store_global(blk);
			serial_println!("VirtIO: Phase 1 init complete");
			fb_println!("VirtIO: block device detected");
		}
	}

	/* Transfer mapper + frame allocator to global PageAllocator for SLUB */
	memory::init_page_allocator(mapper, frame_alloc);
	memory::slub::init();

	/* Phase 2: Allocate virtqueues now that SLUB is available */
	drivers::virtio::setup_queues_global();
	drivers::virtio::register_interrupt();

	/* Register filesystem drivers */
	fs::fat32::init();
	fs::ext2::init();

	/* Boot VFS: RamDir at / and /dev/ */
	vfs::mount("/",     alloc::sync::Arc::new(vfs::RamDir::new("/")));
	vfs::mount("/dev/", alloc::sync::Arc::new(vfs::RamDir::new("dev")));
	serial_println!("VFS: mount table initialized");
	fb_println!("VFS: / and /dev/ ready");

	/* Expose VirtIO block device as /dev/sda */
	let sda: alloc::sync::Arc<dyn INode> = alloc::sync::Arc::new(
		fs::BlockDevINode(alloc::sync::Arc::new(fs::VirtioBlockDev))
	);
	if let Some(dev_dir) = vfs::lookup_path("/dev/") {
		dev_dir.insert("sda", sda).ok();
		serial_println!("VFS: /dev/sda available");
		fb_println!("VFS: /dev/sda ready — run 'mount /dev/sda /' to attach ext2");
	}
	/* Wire up fd 0/1/2 for the init task */
	fd::init_stdio(0);
	serial_println!("FD: stdio initialized for task 0");

	/* Initialize global task scheduler */
	Scheduler::init_global();
	task::scheduler::init();
	serial_println!("Kernel task registered");
	fb_println!("Scheduler: initialized");

	fb_println!("Memory: {} regions mapped", mmap_response.entries().len());

	/* Initialize kernel stack allocator before creating user page tables */
	memory::kstack::init_kstack_region().expect("Failed to init kstack region");
	serial_println!("Kernel stack region initialized");

	/*
	 * Seed RunQueue with a boot placeholder as "current".
	 * The first context switch saves _start's context into boot_task
	 * (which is never re-enqueued), then jumps to the first task.
	 */
	let boot_task = alloc::sync::Arc::new(spin::Mutex::new(
		TaskCB::running_task(),
	));
	task::scheduler::global().lock().current = Some(boot_task);

	/* Spawn the built-in kernel shell */
	match kshell::spawn_kshell() {
		Ok(pid) => {
			serial_println!("Kernel shell spawned: PID={}", pid);
			fb_println!("kshell: spawned PID={}", pid);
		}
		Err(e) => {
			serial_println!("Failed to spawn kshell: {}", e);
			fb_println!("kshell: spawn failed");
		}
	}

	unsafe {
		/* Initialize timer hardware — starts preemptive scheduling */
		apic::timer::init_hardware();
	}
	fb_println!("Timer: LAPIC ~625 Hz started");
	fb_println!("");
	fb_println!("Serix OS v0.0.6 ready.");

	/* Idle loop — timer interrupts drive preemptive scheduling */
	loop {
		hlt();
	}
}
