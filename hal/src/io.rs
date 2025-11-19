/*
 * Port I/O Operations
 *
 * Provides inline assembly functions for x86 port I/O instructions.
 */

/*
 * outb - Write a byte to an I/O port
 * @port: Port address
 * @value: Byte value to write
 */
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
	core::arch::asm!("out dx, al", in("dx") port, in("al") value);
}

/*
 * inb - Read a byte from an I/O port
 * @port: Port address
 *
 * Returns the byte value read from the port.
 */
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
	let value: u8;
	core::arch::asm!("in al, dx", out("al") value, in("dx") port);
	value
}

/*
 * outw - Write a word (16-bits) to an I/O port
 */
#[inline]
pub unsafe fn outw(port: u16, value: u16) {
	core::arch::asm!("out dx, ax", in("dx") port, in("ax") value);
}

/*
 * inw - Read a word (16-bits) from an I/O port
 */
#[inline]
pub unsafe fn inw(port: u16) -> u16 {
	let value: u16;
	core::arch::asm!("in ax, dx", out("ax") value, in("dx") port);
	value
}

/*
 * outl - Write a double word (32-bits) to an I/O port
 */
#[inline]
pub unsafe fn outl(port: u16, value: u32) {
	core::arch::asm!("out dx, eax", in("dx") port, in("eax") value);
}

/*
 * inl - Read a double word (32-bits) from an I/O port
 */
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
	let value: u32;
	core::arch::asm!("in eax, dx", out("eax") value, in("dx") port);
	value
}
