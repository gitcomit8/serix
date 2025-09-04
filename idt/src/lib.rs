#![feature(abi_x86_interrupt)]
#![no_std]

use hal::cpu;
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.divide_error.set_handler_fn(divide_by_zero_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt
    };
}
extern "x86-interrupt" fn divide_by_zero_handler(_stack: InterruptStackFrame) {
    //TODO: Log/panic as needed
    loop {
        cpu::halt();
    }
}

extern "x86-interrupt" fn page_fault_handler(
    _stack: InterruptStackFrame,
    _err: PageFaultErrorCode,
) {
    //TODO: Log info, show faulting address
    loop {
        cpu::halt();
    }
}

extern "x86-interrupt" fn double_fault_handler(_stack: InterruptStackFrame, _err: u64) -> ! {
    //TODO: Log and maybe show stack frame
    loop {
        cpu::halt();
    }
}

pub fn init_idt() {
    IDT.load();
}

pub unsafe fn register_interrupt_handler(
    idt: &mut InterruptDescriptorTable,
    vector: u8,
    handler: extern "x86-interrupt" fn(InterruptStackFrame),
) {
    idt[vector].set_handler_fn(handler);
}
