/*
 * Serix Kernel Main Entry Point
 *
 * This file contains the kernel initialization sequence and main loop.
 * It sets up the GDT, IDT, APIC, memory management, and task execution.
 * All subsystem initialization is delegated to init::* modules.
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
pub mod smp;
pub mod stdio;
mod syscall;
mod init;

use core::panic::PanicInfo;
use hal::serial_println;
use limine::request::MemoryMapRequest;
use task::init_executor;
use util::panic::halt_loop;
use x86_64::instructions::hlt;

/* Limine protocol requests */
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

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
		gdt::init_per_cpu(0); /* BSP is CPU 0 */
		hal::cpu::enable_sse();
		/* Enable APIC and disable legacy PIC */
		apic::enable();
		/* I/O APIC initialization moved to after virtual address remapping */
		/* Register interrupt handlers before IDT is loaded */
		apic::timer::register_handler();
		/* Register keyboard handler (defined in init/cpu to avoid circular deps) */
		idt::register_interrupt_handler(33, init::cpu::keyboard_interrupt_handler);
	}

	/* Setup CPU exception handlers and load IDT */
	idt::init_idt();

	/* Initialize keyboard */
	serial_println!("Keyboard ready for input!");

	/* Enable interrupts globally */
	x86_64::instructions::interrupts::enable();

	init_executor();

	/* ------------------------------------------------------------------ */
	/* Phase 1: Memory initialization                                       */
	/* ------------------------------------------------------------------ */
	let mem = unsafe { init::memory::init_memory(&MMAP_REQ) };

	/* ------------------------------------------------------------------ */
	/* Phase 2: Graphics initialization                                     */
	/* ------------------------------------------------------------------ */
	unsafe {
		init::graphics::init_graphics(MMAP_REQ.get_response().unwrap().entries());
	}

	/* ------------------------------------------------------------------ */
	/* Phase 3: Driver initialization                                       */
	/* ------------------------------------------------------------------ */
	unsafe {
		init::drivers::driver_init(
			mem.phys_mem_offset,
			mem.mapper,
			mem.frame_alloc,
		);
	}

	/* ------------------------------------------------------------------ */
	/* Phase 4: Filesystem initialization                                   */
	/* ------------------------------------------------------------------ */
	init::filesystem::init_filesystem();

	/* ------------------------------------------------------------------ */
	/* Phase 5: Scheduler initialization                                    */
	/* ------------------------------------------------------------------ */
	let core_type = hal::topology::get_core_type();
	init::scheduler::init_scheduler(core_type);

	/* ------------------------------------------------------------------ */
	/* Phase 6: SMP / Timer initialization                                  */
	/* ------------------------------------------------------------------ */
	unsafe {
		init::smp::init_smp();
	}

	graphics::kprintln!("Timer: LAPIC ~625 Hz started");
	graphics::kprintln!("");
	graphics::kprintln!("Serix OS v0.0.6 ready.");

	/* Idle loop — timer interrupts drive preemptive scheduling */
	loop {
		hlt();
	}
}
