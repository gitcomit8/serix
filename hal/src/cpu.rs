/*
 * CPU Control Functions
 *
 * Provides inline functions for CPU halt and interrupt control.
 */

use x86_64::instructions::{hlt, interrupts};

/*
 * halt - Halt the CPU until next interrupt
 *
 * Executes HLT instruction to save power while waiting for interrupts.
 */
#[inline(always)]
pub fn halt() {
	hlt();
}

/*
 * enable_interrupts - Enable CPU interrupts
 *
 * Sets the interrupt flag (IF) in RFLAGS.
 */
#[inline(always)]
pub fn enable_interrupts() {
	interrupts::enable();
}

/*
 * disable_interrupts - Disable CPU interrupts
 *
 * Clears the interrupt flag (IF) in RFLAGS.
 */
#[inline(always)]
pub fn disable_interrupts() {
	interrupts::disable();
}

/*
 * enable_sse - Enable SSE/FPU features in CPU
 *
 * Sets CR0.MP (Monitor Coprocessor), clears CR0.EM (Emulation),
 * and sets CR4.OSFXSR + CR4.OSXMMEXCPT_ENABLE.
 */
pub fn enable_sse() {
	unsafe {
		use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

		let mut cr0 = Cr0::read();
		cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
		cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
		Cr0::write(cr0);

		let mut cr4 = Cr4::read();
		cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
		Cr4::write(cr4);
	}
}
