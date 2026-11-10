/*
 * Serix Kernel Main Entry Point
 *
 * This file contains the kernel initialization sequence and main loop.
 * It sets up the GDT, IDT, APIC, memory management, and task execution.
 * All subsystem initialization is delegated to init::* modules.
 *
 * Entry point: extern "C" fn _start(boot_info: *const BootInfo) -> !
 * The bootloader passes a BootInfo struct pointer in RDI.
 */

#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

extern crate alloc;
pub mod fd;
mod gdt;
mod kshell;
pub mod les_bridge;
pub mod pipe;
pub mod process;
pub mod smp;
pub mod stdio;
pub mod procfs;
mod syscall;
mod init;
mod bootstub;

use boot::BootInfo;
use core::panic::PanicInfo;
use hal::serial_println;
use task::init_executor;
use util::panic::halt_loop;
use x86_64::instructions::hlt;

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
 * Called by the bootloader stub (limine_boot, grub2_boot, etc.) with
 * a pointer to BootInfo in RDI. The stub has already parsed the
 * bootloader's native format and constructed the BootInfo struct.
 */
#[unsafe(no_mangle)]
pub extern "C" fn _start(boot_info: *const BootInfo) -> ! {
	hal::init_serial();
	serial_println!("Serix Kernel v0.0.6 Starting.....");

	/* Validate the BootInfo passed by the bootstub */
	let boot_info_ref: &'static BootInfo = unsafe {
		boot::validate_boot_info(boot_info)
			.expect("Invalid or missing boot info")
	};

	serial_println!("[BOOT] HHDM offset: {:#x}", boot_info_ref.hhdm_offset);
	serial_println!(
		"[BOOT] Memory regions: {}, CPUs: {}",
		boot_info_ref.memory_regions.len(),
		boot_info_ref.cpus.len()
	);

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
		apic::ipi::register_handlers();
		/* Register keyboard handler (defined in init/cpu to avoid circular deps) */
		idt::register_interrupt_handler(33, init::cpu::keyboard_interrupt_handler);
	}

	/* Setup CPU exception handlers and load IDT */
	idt::init_idt();

	/* Initialize PS/2 keyboard controller (must happen before interrupts) */
	serial_println!("Initializing PS/2 keyboard...");
	unsafe { init::cpu::init_ps2_keyboard(); }
	serial_println!("PS/2 init complete, enabling interrupts...");

	/* Enable interrupts globally */
	x86_64::instructions::interrupts::enable();
	serial_println!("Global interrupts ENABLED");

	init_executor();
	serial_println!("Executor initialized");

	/* ------------------------------------------------------------------ */
	/* Phase 1: Memory initialization                                       */
	/* ------------------------------------------------------------------ */
	serial_println!("Phase 1: init_memory...");
	let mem = unsafe { init::memory::init_memory(boot_info_ref) };
	let _platform_topology = unsafe {
		init::acpi::discover(boot_info_ref, mem.phys_mem_offset.as_u64())
	};
	serial_println!("Phase 1 complete");

	/* Initialize global audit ring (before any capability operations).
	 * AuditRing::new() creates a ~6KB local array — must fit on boot stack. */
	capability::init_audit();

	/* ------------------------------------------------------------------ */
	/* Phase 2: Graphics initialization                                     */
	/* ------------------------------------------------------------------ */
	unsafe {
		init::graphics::init_graphics(boot_info_ref);
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
	/* LES bridge: load default deny policy (after /proc is created)        */
	/* ------------------------------------------------------------------ */
	les_bridge::init_default_policy();

	/* ------------------------------------------------------------------ */
	/* Phase 5: Scheduler initialization                                    */
	/* ------------------------------------------------------------------ */
	let core_info = hal::topology::get_core_type_info();
	let bsp_apic_id = unsafe { apic::smp::read_apic_id() };
	serial_println!(
		"[CPU] logical=0 apic={} core_type={:?} leaf_1a={} raw={:#04x}",
		bsp_apic_id,
		core_info.kind,
		core_info.leaf_supported,
		core_info.raw_type
	);
	init::scheduler::init_scheduler();

	/* ------------------------------------------------------------------ */
	/* Phase 6: SMP / Timer initialization                                  */
	/* ------------------------------------------------------------------ */
	unsafe {
		init::smp::init_smp(boot_info_ref);
	}

	graphics::kprintln!("Timer: LAPIC ~625 Hz started");
	graphics::kprintln!("");
	graphics::kprintln!("Serix OS v0.0.6 ready.");

	/* Diagnostic: check if keyboard ISR has fired */
	let isr_count = keyboard::get_isr_fire_count();
	let last_scancode = keyboard::get_last_scancode();
	serial_println!(
		"[DIAG] Keyboard ISR fire count: {}, last scancode: {:#04x}",
		isr_count,
		last_scancode
	);

	/* Idle loop — timer interrupts drive preemptive scheduling */
	loop {
		hlt();
	}
}
