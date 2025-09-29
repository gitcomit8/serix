#![feature(abi_x86_interrupt)]
#![no_std]

use hal::serial_println;
use lazy_static::lazy_static;
use util::panic::oops;
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
	oops("Divide by Zero exception");
}

extern "x86-interrupt" fn page_fault_handler(stack: InterruptStackFrame, err: PageFaultErrorCode) {
	serial_println!(
		"Page fault at instruction pointer: {:#x}",
		stack.instruction_pointer.as_u64()
	);

	let cr2: u64;
	unsafe {
		core::arch::asm!("mov {}, cr2", out(reg) cr2);
	}
	serial_println!("Page fault address: {:#x}", cr2);
	serial_println!("Error Code: {:?}", err);

	oops("Page fault exception");
}

extern "x86-interrupt" fn double_fault_handler(_stack: InterruptStackFrame, _err: u64) -> ! {
	serial_println!(
		"Double fault at instruction pointer: {:#x}",
		_stack.instruction_pointer.as_u64()
	);
	panic!("Double fault exception");
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
