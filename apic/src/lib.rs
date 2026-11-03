/*
 * apic/lib.rs - Advanced Programmable Interrupt Controller (APIC) Driver
 *
 * Provides Local APIC and I/O APIC support for interrupt handling.
 * Replaces the legacy 8259 PIC with modern APIC architecture.
 */

#![feature(abi_x86_interrupt)]
#![no_std]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use hal::serial_println;

pub mod ioapic;
pub mod ipi;
pub mod smp;
pub mod timer;

/*
 * APIC Base Addresses (Physical/Virtual)
 * Default to physical addresses for early boot compatibility.
 * These should be updated to virtual higher-half addresses once paging is up.
 */
static APIC_BASE: AtomicU64 = AtomicU64::new(0xFEE00000);
static X2APIC_ENABLED: AtomicBool = AtomicBool::new(false);

/*
 * set_bases - Update the base addresses for APIC components
 * @lapic: Virtual address of Local APIC
 */
pub fn set_bases(lapic: u64) {
	APIC_BASE.store(lapic, Ordering::Relaxed);
}

/*
 * lapic_reg - Get pointer to Local APIC register
 * @offset: Register offset from base address
 *
 * Return: Pointer to the specified Local APIC register
 */
fn lapic_reg(offset: u32) -> *mut u32 {
	(APIC_BASE.load(Ordering::Relaxed) + offset as u64) as *mut u32
}

fn rdmsr(msr: u32) -> u64 {
	let low: u32;
	let high: u32;
	unsafe {
		core::arch::asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nostack, nomem));
	}
	((high as u64) << 32) | low as u64
}

fn wrmsr(msr: u32, value: u64) {
	unsafe {
		core::arch::asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nostack, nomem));
	}
}

pub fn lapic_read(offset: u32) -> u32 {
	if X2APIC_ENABLED.load(Ordering::Acquire) {
		rdmsr(0x800 + offset / 16) as u32
	} else {
		unsafe { lapic_reg(offset).read_volatile() }
	}
}

pub fn lapic_write(offset: u32, value: u32) {
	if X2APIC_ENABLED.load(Ordering::Acquire) {
		wrmsr(0x800 + offset / 16, value as u64);
	} else {
		unsafe {
			lapic_reg(offset).write_volatile(value);
		}
	}
}

pub fn x2apic_enabled() -> bool {
	X2APIC_ENABLED.load(Ordering::Acquire)
}

/// Send a fixed-delivery IPI to a physical APIC ID. Returns false on an
/// unsupported xAPIC destination or a bounded xAPIC delivery timeout.
pub fn send_ipi(destination_id: u32, command: u32) -> bool {
	if x2apic_enabled() {
		wrmsr(0x830, ((destination_id as u64) << 32) | command as u64);
		return true;
	}
	if destination_id > u8::MAX as u32 {
		return false;
	}
	let mut spins = 1_000_000usize;
	while lapic_read(0x300) & (1 << 12) != 0 {
		if spins == 0 {
			return false;
		}
		spins -= 1;
		core::hint::spin_loop();
	}
	lapic_write(0x310, destination_id << 24);
	lapic_write(0x300, command);
	spins = 1_000_000;
	while lapic_read(0x300) & (1 << 12) != 0 {
		if spins == 0 {
			return false;
		}
		spins -= 1;
		core::hint::spin_loop();
	}
	true
}
/*
 * disable_pic - Disable legacy 8259 PIC
 *
 * Remaps and masks the legacy PIC to prevent conflicts with APIC.
 * This is necessary before enabling APIC on systems that have both.
 */
pub unsafe fn disable_pic() {
	use x86_64::instructions::port::Port;

	let mut pic1_cmd: Port<u8> = Port::new(0x20);
	let mut pic1_data: Port<u8> = Port::new(0x21);
	let mut pic2_cmd: Port<u8> = Port::new(0xA0);
	let mut pic2_data: Port<u8> = Port::new(0xA1);

	/* Start initialization sequence */
	pic1_cmd.write(0x11);
	pic2_cmd.write(0x11);

	/* Remap IRQs to vectors 32-47 */
	pic1_data.write(0x20);
	pic2_data.write(0x28);

	/* Set up cascading */
	pic1_data.write(0x04);
	pic2_data.write(0x02);

	/* Set 8086 mode */
	pic1_data.write(0x01);
	pic2_data.write(0x01);

	/* Mask all interrupts */
	pic1_data.write(0xFF);
	pic2_data.write(0xFF);

	serial_println!("Legacy PIC disabled");
}

/*
 * enable - Enable Local APIC
 *
 * Disables the legacy PIC and enables the Local APIC by setting
 * the appropriate bits in the IA32_APIC_BASE MSR and SVR register.
 */
pub unsafe fn enable() {
	/* Disable legacy PIC first */
	disable_pic();

	let features = core::arch::x86_64::__cpuid(1);
	if features.ecx & (1 << 21) != 0 {
		/* Record a system-wide mode before APs are started. Each CPU enables it locally. */
		X2APIC_ENABLED.store(true, Ordering::Release);
	} else {
		X2APIC_ENABLED.store(false, Ordering::Release);
	}
	let _ = enable_current_cpu();
	serial_println!("{} enabled", if x2apic_enabled() { "x2APIC" } else { "xAPIC" });
	serial_println!("APIC enabled");
}


/// Enable the already-selected APIC mode on the current logical CPU.
/// The BSP selects mode with `enable()` before any AP is released.
pub unsafe fn enable_current_cpu() -> bool {
	let mut apic_base = rdmsr(0x1B) | (1 << 11);
	if x2apic_enabled() {
		let features = core::arch::x86_64::__cpuid(1);
		if features.ecx & (1 << 21) == 0 {
			serial_println!("x2APIC selected but unsupported on this CPU");
			return false;
		}
		apic_base |= 1 << 10;
	}
	wrmsr(0x1B, apic_base);
	/* Enable LAPIC by setting bit 8 in Spurious Interrupt Vector Register */
	let val = lapic_read(0xF0) | 0x100;
	lapic_write(0xF0, val);
	true
}

/*
 * set_timer - Configure Local APIC timer
 * @vector: Interrupt vector to use for timer
 * @divide: Timer divider configuration
 * @initial_count: Initial count value
 *
 * Configures the Local APIC timer with the specified parameters.
 */
pub unsafe fn set_timer(vector: u8, divide: u32, initial_count: u32) {
	lapic_write(0x3E0, divide);
	lapic_write(0x320, (vector as u32) | 0x20000);
	lapic_write(0x380, initial_count);
}

/*
 * send_eoi - Send End of Interrupt signal
 *
 * Signals to the Local APIC that interrupt processing is complete.
 */
pub unsafe fn send_eoi() {
	lapic_write(0xB0, 0);
}
