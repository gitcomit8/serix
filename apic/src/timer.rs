use crate::{enable, send_eoi, set_timer};
use hal::cpu;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub const TIMER_VECTOR: u8 = 0x31;

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
        cpu::enable_interrupts();
    }
}

pub fn ticks() -> u64 {
    unsafe { TICKS }
}
