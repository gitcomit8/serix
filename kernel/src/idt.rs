use crate::hal::cpu;
use core::ptr::addr_of_mut;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();
extern "x86-interrupt" fn divide_by_zero(_stack_frame: InterruptStackFrame) {
    cpu::halt();
}

extern "x86-interrupt" fn page_fault(_stack_frame: InterruptStackFrame, _err: PageFaultErrorCode) {
    cpu::halt();
}

extern "x86-interrupt" fn double_fault(_stack_frame: InterruptStackFrame, _err: u64) -> ! {
    loop {}
}

pub fn init_idt() {
    unsafe {
        let idt_ptr: *mut InterruptDescriptorTable = addr_of_mut!(IDT);
        (*idt_ptr).divide_error.set_handler_fn(divide_by_zero);
        (*idt_ptr).page_fault.set_handler_fn(page_fault);
        (*idt_ptr).double_fault.set_handler_fn(double_fault);
        (*idt_ptr).load();
    }
}
