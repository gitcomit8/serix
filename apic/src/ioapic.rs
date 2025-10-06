const IOAPIC_BASE: u64 = 0xFEC00000;

fn ioapic_reg(offset: u32) -> *mut u32 {
	(IOAPIC_BASE + offset as u64) as *mut u32
}

unsafe fn ioapic_read(reg: u32) {
	ioapic_reg(0x00).write_volatile(reg);
	ioapic_reg(0x10).read_volatile();
}
unsafe fn ioapic_write(reg: u32, value: u32) {
	ioapic_reg(0x00).write_volatile(reg);
	ioapic_reg(0x10).write_volatile(value);
}

pub unsafe fn map_irq(irq: u8, vector: u8) {
	let reg = 0x10 + (irq as u32 * 2);
	ioapic_write(reg, vector as u32);
	ioapic_write(reg + 1, 0);
}

pub unsafe fn init_ioapic() {
	//example route irq 1 to vector 33(keyboard)
	map_irq(1, 33);
	//route irq 0 to vector 32 (timer)
	map_irq(0, 32);
}
