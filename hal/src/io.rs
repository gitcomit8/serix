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
