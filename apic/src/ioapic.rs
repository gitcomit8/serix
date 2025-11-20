/*
 * I/O APIC Driver
 *
 * Provides interrupt routing from external devices to Local APICs.
 */

/* I/O APIC base address in memory */
use core::sync::atomic::{AtomicU64, Ordering};

static IOAPIC_BASE: AtomicU64 = AtomicU64::new(0xFEC00000);
pub fn set_base(addr: u64) {
	IOAPIC_BASE.store(addr, Ordering::Relaxed);
}

/*
 * ioapic_reg - Get pointer to I/O APIC register
 * @offset: Register offset from base address
 *
 * Returns a pointer to the specified I/O APIC register.
 */
fn ioapic_reg(offset: u32) -> *mut u32 {
	(IOAPIC_BASE.load(Ordering::Relaxed) + offset as u64) as *mut u32
}

/*
 * ioapic_read - Read from I/O APIC register
 * @reg: Register number to read
 *
 * Returns the value of the specified register.
 */
unsafe fn ioapic_read(reg: u32) -> u32 {
	ioapic_reg(0x00).write_volatile(reg);
	ioapic_reg(0x10).read_volatile()
}

/*
 * ioapic_write - Write to I/O APIC register
 * @reg: Register number to write
 * @value: Value to write
 */
unsafe fn ioapic_write(reg: u32, value: u32) {
	ioapic_reg(0x00).write_volatile(reg);
	ioapic_reg(0x10).write_volatile(value);
}

/*
 * map_irq - Map IRQ line to interrupt vector
 * @irq: IRQ line number
 * @vector: Interrupt vector to map to
 *
 * Routes the specified IRQ to the given interrupt vector.
 */
pub unsafe fn map_irq(irq: u8, vector: u8) {
	let reg = 0x10 + (irq as u32 * 2);
	ioapic_write(reg, vector as u32);
	ioapic_write(reg + 1, 0);
}

/*
 * init_ioapic - Initialize I/O APIC
 *
 * Sets up interrupt routing for keyboard (IRQ 1) and timer (IRQ 0).
 */
pub unsafe fn init_ioapic() {
	/* Route IRQ 1 to vector 33 (keyboard) */
	map_irq(1, 33);
	/* Route IRQ 0 to vector 32 (timer) */
	map_irq(0, 32);
}
