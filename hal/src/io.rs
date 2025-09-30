//Write a byte to an I/O port
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
	core::arch::asm!("out dx, al", in("dx") port, in("al") value);
}

//Read a byte from an I/O port
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
	let value: u8;
	core::arch::asm!("in al, dx", out("al") value, in("dx") port);
	value
}
