/*
 * PS/2 Keyboard Driver
 *
 * Handles keyboard input via PS/2 controller and scancode translation.
 * Uses a fixed-size Ring Buffer to be Interrupt-Safe (No Heap Allocations).
 */

#![no_std]

use spin::Mutex;

/*
 * US QWERTY scancode Set 1 to ASCII mapping table
 */
const SCANDCODE_TO_ASCII: [u8; 128] = [
	0, 27, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', 8, b'\t', b'q',
	b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\n', 0, b'a', b's', b'd',
	b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`', 0, b'\\', b'z', b'x', b'c', b'v', b'b',
	b'n', b'm', b',', b'.', b'/', 0, b'*', 0, b' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/* * Ring Buffer Implementation
 * Fixed size, no allocation, interrupt safe with Mutex.
 */
const BUF_SIZE: usize = 128;

pub struct RingBuffer {
	data: [u8; BUF_SIZE],
	head: usize,
	tail: usize,
}

impl RingBuffer {
	const fn new() -> Self {
		Self {
			data: [0; BUF_SIZE],
			head: 0,
			tail: 0,
		}
	}

	fn push(&mut self, val: u8) {
		let next = (self.head + 1) % BUF_SIZE;
		if next != self.tail {
			self.data[self.head] = val;
			self.head = next;
		}
		// If full, drop the key (better than overwriting or allocating)
	}

	fn pop(&mut self) -> Option<u8> {
		if self.head == self.tail {
			None
		} else {
			let val = self.data[self.tail];
			self.tail = (self.tail + 1) % BUF_SIZE;
			Some(val)
		}
	}
}

// Global static instance
static INPUT_BUF: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());

/*
 * handle_scancode - Process keyboard scancode
 */
pub fn handle_scancode(scancode: u8) {
	/* Ignore break codes (bit 7 set) */
	if scancode & 0x80 != 0 {
		return;
	}

	/* Translate and buffer printable characters */
	if let Some(&ascii) = SCANDCODE_TO_ASCII.get(scancode as usize) {
		if ascii != 0 {
			// Push to ring buffer (Interrupt Safe)
			x86_64::instructions::interrupts::without_interrupts(|| {
				INPUT_BUF.lock().push(ascii);
			});
		}
	}
}

/*
 * pop_key - Retrieve the next key from the buffer
 */
pub fn pop_key() -> Option<u8> {
	x86_64::instructions::interrupts::without_interrupts(|| INPUT_BUF.lock().pop())
}

/*
 * enable_keyboard_interrupt - Legacy PIC helper
 */
pub fn enable_keyboard_interrupt() {
	unsafe {
		let mut port = x86_64::instructions::port::Port::new(0x21);
		let mask: u8 = port.read();
		port.write(mask & !0x02);
	}
}
