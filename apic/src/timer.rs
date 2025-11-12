use crate::{lapic_reg, send_eoi};
use x86_64::structures::idt::InterruptStackFrame;

pub const TIMER_VECTOR: u8 = 0x31;
pub const TIMER_DIVIDE_CONFIG: u32 = 0x3; // Divide by 16
pub const TIMER_INITIAL_COUNT: u32 = 100_000; // Reasonable timer interval

static mut TICKS: u64 = 0;

//Timer Interrupt Handler
extern "x86-interrupt" fn timer_interrupt(_stack_frame: InterruptStackFrame) {
	unsafe {
		TICKS += 1;
		//Signal end of interrupt to LAPIC
		send_eoi();
	}
}

pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
	task::preempt_executor();
	unsafe {
		crate::send_eoi();
	}
}
// Register the timer interrupt handler (call before IDT is loaded)
pub unsafe fn register_handler() {
	idt::register_interrupt_handler(TIMER_VECTOR, timer_interrupt);
}

// Initialize the timer hardware (call after IDT is loaded and interrupts can be enabled)
pub unsafe fn init_hardware() {
	// Divide Configuration Register
	lapic_reg(0x3E0).write_volatile(TIMER_DIVIDE_CONFIG);
	// LVT Timer Register - set timer vector and periodic mode (bit 17)
	lapic_reg(0x320).write_volatile((TIMER_VECTOR as u32) | 0x20000);
	// Initial Count Register
	lapic_reg(0x380).write_volatile(TIMER_INITIAL_COUNT);

	// Enable interrupts
	hal::cpu::enable_interrupts();
}
pub fn ticks() -> u64 {
	unsafe { TICKS }
}
