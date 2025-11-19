/*
 * Advanced Programmable Interrupt Controller (APIC) Driver
 *
 * Provides Local APIC and I/O APIC support for interrupt handling.
 * Replaces the legacy 8259 PIC with modern APIC architecture.
 */

#![feature(abi_x86_interrupt)]
#![no_std]

use hal::serial_println;

pub mod ioapic;
pub mod timer;

/* Local APIC base address in memory */
const APIC_BASE: u64 = 0xFEE00000;

/*
 * lapic_reg - Get pointer to Local APIC register
 * @offset: Register offset from base address
 *
 * Returns a pointer to the specified Local APIC register.
 */
fn lapic_reg(offset: u32) -> *mut u32 {
	(APIC_BASE + offset as u64) as *mut u32
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

	/* Enable LAPIC in IA32_APIC_BASE MSR (MSR 0x1B) */
	let mut apic_base: u64;
	core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, lateout("eax") apic_base, lateout("edx") _);

	/* Set bit 11 (APIC Global Enable) */
	if (apic_base & (1 << 11)) == 0 {
		apic_base |= 1 << 11;
		let eax = (apic_base & 0xFFFFFFFF) as u32;
		let edx = ((apic_base >> 32) & 0xFFFFFFFF) as u32;
		core::arch::asm!("wrmsr", in("ecx") 0x1Bu32, in("eax") eax, in("edx") edx, options(nostack, nomem));
	}

	/* Enable LAPIC by setting bit 8 in Spurious Interrupt Vector Register */
	let svr = lapic_reg(0xF0);
	let val = svr.read_volatile() | 0x100;
	svr.write_volatile(val);

	serial_println!("APIC enabled");
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
	lapic_reg(0x3E0).write_volatile(divide);
	lapic_reg(0x320).write_volatile((vector as u32) | 0x20000);
	lapic_reg(0x380).write_volatile(initial_count);
}

/*
 * send_eoi - Send End of Interrupt signal
 *
 * Signals to the Local APIC that interrupt processing is complete.
 */
pub unsafe fn send_eoi() {
	let eoi = lapic_reg(0xB0);
	eoi.write_volatile(0);
}
