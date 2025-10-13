#![feature(abi_x86_interrupt)]
#![no_std]

use core::cell::UnsafeCell;
use hal::serial_println;
use lazy_static::lazy_static;
use util::panic::oops;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

struct IdtWrapper {
    idt: UnsafeCell<InterruptDescriptorTable>,
    loaded: UnsafeCell<bool>,
}

unsafe impl Sync for IdtWrapper {}

lazy_static! {
	static ref IDT: IdtWrapper = {
		let mut idt = InterruptDescriptorTable::new();
		idt.divide_error.set_handler_fn(divide_by_zero_handler);
		idt.page_fault.set_handler_fn(page_fault_handler);
		idt.double_fault.set_handler_fn(double_fault_handler);
		idt[33].set_handler_fn(keyboard_interrupt_handler);
		IdtWrapper {
			idt: UnsafeCell::new(idt),
			loaded: UnsafeCell::new(false),
		}
	};
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    //Read scancode from keyboard port 0x60
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    //Process scancode
    keyboard::handle_scancode(scancode);

    //Send EOI to APIC directly
    unsafe {
        const APIC_EOI: *mut u32 = 0xFEE000B0 as *mut u32;
        APIC_EOI.write_volatile(0);
    }
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
    unsafe {
        (*IDT.idt.get()).load();
        *IDT.loaded.get() = true;
    }
}

pub fn register_interrupt_handler(
    vector: u8,
    handler: extern "x86-interrupt" fn(InterruptStackFrame),
) {
    unsafe {
        let idt = &mut *IDT.idt.get();
        idt[vector].set_handler_fn(handler);

        // Only reload if IDT was already loaded
        if *IDT.loaded.get() {
            idt.load();
        }
    }
}
