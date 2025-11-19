/*
 * APIC Timer Driver
 *
 * Implements Local APIC timer for periodic interrupts and timekeeping.
 */

use crate::{lapic_reg, send_eoi};
use task;
use x86_64::structures::idt::InterruptStackFrame;

/* Timer configuration constants */
pub const TIMER_VECTOR: u8 = 0x31;
pub const TIMER_DIVIDE_CONFIG: u32 = 0x3; /* Divide by 16 */
pub const TIMER_INITIAL_COUNT: u32 = 100_000; /* Timer interval */

/* Global tick counter */
static mut TICKS: u64 = 0;

/*
 * timer_interrupt - Timer interrupt handler
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Increments the global tick counter and sends EOI to LAPIC.
 */
extern "x86-interrupt" fn timer_interrupt(_stack_frame: InterruptStackFrame) {
	unsafe {
		TICKS += 1;
		/* Signal end of interrupt to LAPIC */
		send_eoi();
	}
	task::schedule();
}

/*
 * timer_interrupt_handler - Timer interrupt handler with task preemption
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Preempts the current task and sends EOI to LAPIC.
 */
pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
	task::preempt_executor();
	unsafe {
		crate::send_eoi();
	}
}

/*
 * register_handler - Register timer interrupt handler
 *
 * Must be called before IDT is loaded.
 */
pub unsafe fn register_handler() {
	idt::register_interrupt_handler(TIMER_VECTOR, timer_interrupt);
}

/*
 * init_hardware - Initialize APIC timer hardware
 *
 * Configures the Local APIC timer in periodic mode.
 * Must be called after IDT is loaded and interrupts can be enabled.
 */
pub unsafe fn init_hardware() {
	/* Configure timer divider */
	lapic_reg(0x3E0).write_volatile(TIMER_DIVIDE_CONFIG);

	/* Set timer vector and enable periodic mode (bit 17) */
	lapic_reg(0x320).write_volatile((TIMER_VECTOR as u32) | 0x20000);

	/* Set initial count to start timer */
	lapic_reg(0x380).write_volatile(TIMER_INITIAL_COUNT);

	/* Enable interrupts */
	hal::cpu::enable_interrupts();
}

/*
 * ticks - Get current tick count
 *
 * Returns the number of timer ticks since boot.
 */
pub fn ticks() -> u64 {
	unsafe { TICKS }
}
