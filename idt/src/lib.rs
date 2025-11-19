/*
 * Interrupt Descriptor Table (IDT) Setup
 *
 * Configures CPU exception handlers and device interrupt handlers.
 * Provides a global IDT for handling faults, traps, and interrupts.
 */

#![feature(abi_x86_interrupt)]
#![no_std]

use core::cell::UnsafeCell;
use hal::serial_println;
use lazy_static::lazy_static;
use util::panic::oops;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

/*
 * struct IdtWrapper - Thread-safe IDT wrapper
 * @idt: The actual interrupt descriptor table
 * @loaded: Flag indicating whether IDT has been loaded into CPU
 *
 * Wraps IDT in UnsafeCell to allow mutable access from static context.
 */
struct IdtWrapper {
	idt: UnsafeCell<InterruptDescriptorTable>,
	loaded: UnsafeCell<bool>,
}

unsafe impl Sync for IdtWrapper {}

/* Global IDT instance with default exception handlers */
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

/*
 * keyboard_interrupt_handler - Handle keyboard interrupts
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Reads scancode from keyboard controller and sends EOI to APIC.
 */
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
	use x86_64::instructions::port::Port;

	/* Read scancode from keyboard data port */
	let mut port = Port::new(0x60);
	let scancode: u8 = unsafe { port.read() };

	/* Process the scancode */
	keyboard::handle_scancode(scancode);

	/* Send End of Interrupt to Local APIC */
	unsafe {
		const APIC_EOI: *mut u32 = 0xFEE000B0 as *mut u32;
		APIC_EOI.write_volatile(0);
	}
}

/*
 * divide_by_zero_handler - Handle division by zero exception
 * @_stack: Interrupt stack frame (unused)
 */
extern "x86-interrupt" fn divide_by_zero_handler(_stack: InterruptStackFrame) {
	oops("Divide by Zero exception");
}

/*
 * page_fault_handler - Handle page fault exception
 * @stack: Interrupt stack frame with fault information
 * @err: Page fault error code
 *
 * Prints diagnostic information and halts the system.
 */
extern "x86-interrupt" fn page_fault_handler(stack: InterruptStackFrame, err: PageFaultErrorCode) {
	serial_println!(
		"Page fault at instruction pointer: {:#x}",
		stack.instruction_pointer.as_u64()
	);

	/* Read CR2 to get faulting address */
	let cr2: u64;
	unsafe {
		core::arch::asm!("mov {}, cr2", out(reg) cr2);
	}
	serial_println!("Page fault address: {:#x}", cr2);
	serial_println!("Error Code: {:?}", err);

	oops("Page fault exception");
}

/*
 * double_fault_handler - Handle double fault exception
 * @_stack: Interrupt stack frame
 * @_err: Error code
 *
 * Double faults are critical errors that occur when handling another exception.
 */
extern "x86-interrupt" fn double_fault_handler(_stack: InterruptStackFrame, _err: u64) -> ! {
	serial_println!(
		"Double fault at instruction pointer: {:#x}",
		_stack.instruction_pointer.as_u64()
	);
	panic!("Double fault exception");
}

/*
 * init_idt - Initialize and load the IDT
 *
 * Loads the IDT into the CPU and marks it as loaded.
 */
pub fn init_idt() {
	unsafe {
		(*IDT.idt.get()).load();
		*IDT.loaded.get() = true;
	}
}

/*
 * register_interrupt_handler - Register a custom interrupt handler
 * @vector: Interrupt vector number (0-255)
 * @handler: Handler function to register
 *
 * Dynamically registers an interrupt handler for the specified vector.
 * Reloads the IDT if it was already loaded.
 */
pub fn register_interrupt_handler(
	vector: u8,
	handler: extern "x86-interrupt" fn(InterruptStackFrame),
) {
	unsafe {
		let idt = &mut *IDT.idt.get();
		idt[vector].set_handler_fn(handler);

		/* Reload IDT if already loaded */
		if *IDT.loaded.get() {
			idt.load();
		}
	}
}
