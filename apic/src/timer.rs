use crate::{enable, lapic_reg, send_eoi, set_timer};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub const TIMER_VECTOR: u8 = 0x31;
pub const TIMER_DIVIDE_CONFIG: u32 = 0x3;
pub const TIMER_INITIAL_COUNT: u32 = 0xFFFF_FFFF;

static mut TICKS: u64 = 0;

//Timer Interrupt Handler
extern "x86-interrupt" fn timer_interrupt(_stack_frame: InterruptStackFrame) {
	unsafe {
		TICKS += 1;
		//Signal end of interrupt to LAPIC
		send_eoi();
	}
}

// Initialize the timer
pub fn init() {
	unsafe {
		let mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();
		idt::register_interrupt_handler(&mut IDT, TIMER_VECTOR, timer_interrupt);
		enable();

		set_timer(TIMER_VECTOR, 0b1011, 10_000_000);
		hal::cpu::enable_interrupts();
	}
}

pub unsafe fn init_timer_periodic() {
	// Divide Configuration Register
	lapic_reg(0x3E0).write_volatile(TIMER_DIVIDE_CONFIG);
	// LVT Timer Register - set timer vector and periodic mode (bit 17)
	lapic_reg(0x320).write_volatile((TIMER_VECTOR as u32) | 0x20000);
	// Initial Count Register
	lapic_reg(0x380).write_volatile(TIMER_INITIAL_COUNT);
}
pub fn ticks() -> u64 {
	unsafe { TICKS }
}
