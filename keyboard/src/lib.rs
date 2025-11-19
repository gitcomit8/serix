/*
 * PS/2 Keyboard Driver
 *
 * Handles keyboard input via PS/2 controller and scancode translation.
 * Provides basic US QWERTY layout support.
 */

#![no_std]

/*
 * US QWERTY scancode Set 1 to ASCII mapping table
 * Index is the scancode, value is the ASCII character.
 * Zero entries represent non-printable keys or unsupported scancodes.
 */
const SCANDCODE_TO_ASCII: [u8; 128] = [
	0, 27, b'1', b'2', b'3', b'4', b'5', b'6',
	b'7', b'8', b'9', b'0', b'-', b'=', 8, b'\t',
	b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i',
	b'o', b'p', b'[', b']', b'\n', 0, b'a', b's',
	b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';',
	b'\'', b'`', 0, b'\\', b'z', b'x', b'c', b'v',
	b'b', b'n', b'm', b',', b'.', b'/', 0, b'*',
	0, b' ', 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/*
 * handle_scancode - Process keyboard scancode
 * @scancode: Raw scancode from keyboard controller
 *
 * Translates scancode to ASCII and outputs to serial and framebuffer.
 * Ignores break codes (key release events).
 */
pub fn handle_scancode(scancode: u8) {
	/* Ignore break codes (bit 7 set) */
	if scancode & 0x80 != 0 {
		return;
	}

	/* Translate and output printable characters */
	if let Some(&ascii) = SCANDCODE_TO_ASCII.get(scancode as usize) {
		if ascii != 0 {
			hal::serial_print!("{}", ascii as char);
			graphics::fb_print!("{}", ascii as char);
		}
	}
}

/*
 * enable_keyboard_interrupt - Enable keyboard IRQ in PIC
 *
 * Unmasks IRQ1 (keyboard) in the legacy PIC.
 * Note: This function is for legacy PIC; with APIC, use I/O APIC routing instead.
 */
pub fn enable_keyboard_interrupt() {
	unsafe {
		let mut port = x86_64::instructions::port::Port::new(0x21);
		let mask: u8 = port.read();
		port.write(mask & !0x02);
	}
}