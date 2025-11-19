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
