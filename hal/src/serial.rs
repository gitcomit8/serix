/*
 * Serial Port Driver (COM1)
 *
 * Implements serial console output for debugging via COM1 port.
 * Configured for 115200 baud, 8 data bits, no parity, 1 stop bit (8N1).
 */

use core::fmt::Write;
use crate::io::*;

/* COM1 serial port base address */
const COM1: u16 = 0x3F8;

/* Serial port register offsets from base address */
const DATA_REG: u16 = 0;		/* Data register (read/write) */
const INT_EN_REG: u16 = 1;		/* Interrupt enable register */
const FIFO_REG: u16 = 2;		/* FIFO control register */
const LINE_CTRL_REG: u16 = 3;	/* Line control register */
const MODEM_CTRL_REG: u16 = 4;	/* Modem control register */
const LINE_STATUS_REG: u16 = 5;	/* Line status register */

/*
 * struct SerialPort - Serial port instance
 * @base: Base I/O port address
 */
pub struct SerialPort{
	base: u16,
}

impl SerialPort{
	/*
	 * new - Create and initialize COM1 serial port
	 *
	 * Returns a configured SerialPort instance.
	 */
	pub fn new()-> Self{
		let port=SerialPort{base:COM1};
		unsafe{
			port.init();
		}
		port
	}

	/*
	 * init - Initialize serial port hardware
	 *
	 * Configures the serial port for 115200 baud, 8N1 mode with FIFO enabled.
	 */
	unsafe fn init(&self){
		outb(self.base+INT_EN_REG, 0x00);		/* Disable interrupts */
		outb(self.base+LINE_CTRL_REG, 0x80);	/* Enable DLAB for baud rate */
		outb(self.base+DATA_REG, 0x01);			/* Divisor low byte (115200) */
		outb(self.base+INT_EN_REG, 0x0);		/* Divisor high byte */
		outb(self.base+LINE_CTRL_REG, 0x3);		/* 8N1 mode */
		outb(self.base+FIFO_REG, 0xC7);			/* Enable FIFO, clear, 14-byte threshold */
		outb(self.base+MODEM_CTRL_REG, 0x0B);	/* Enable RTS, DTR, and IRQs */
	}

	/*
	 * is_transmit_empty - Check if transmit buffer is empty
	 *
	 * Returns true if the transmitter is ready for the next byte.
	 */
	fn is_transmit_empty(&self)-> bool{
		unsafe {
			inb(self.base+LINE_STATUS_REG)&0x20!=0
		}
	}

	/*
	 * write_byte - Write a single byte to serial port
	 * @byte: Byte to transmit
	 *
	 * Waits for transmit buffer to be empty before writing.
	 */
	pub fn write_byte(&self, byte:u8){
		while !self.is_transmit_empty(){
			core::hint::spin_loop();
		}

		unsafe{
			outb(self.base+DATA_REG, byte);
		}
	}

	/*
	 * write_str - Write a string to serial port
	 * @s: String slice to write
	 */
	pub fn write_str(&self, s: &str){
		for byte in s.bytes(){
			self.write_byte(byte);
		}
	}
}

use spin::Mutex;
use spin::Once;

/* Global serial port instance */
static SERIAL_PORT: Once<Mutex<SerialPort>>=Once::new();

/*
 * init_serial - Initialize global serial port
 *
 * Must be called before using serial output functions.
 */
pub fn init_serial(){
	SERIAL_PORT.call_once(|| Mutex::new(SerialPort::new()));
}

/*
 * serial_print - Write string to serial port (thread-safe)
 * @s: String slice to write
 */
pub fn serial_print(s: &str){
	if let Some(serial)=SERIAL_PORT.get(){
		let port = serial.lock();
		port.write_str(s);
	}
}

/*
 * serial_print! - Print formatted output to serial console
 *
 * Usage: serial_print!("format string {}", args...)
 */
#[macro_export]
macro_rules! serial_print {
	($($arg:tt)*) => {$crate::serial::_serial_print(format_args!($($arg)*))
	};
}

/*
 * serial_println! - Print formatted output with newline to serial console
 *
 * Usage: serial_println!("format string {}", args...)
 */
#[macro_export]
macro_rules! serial_println {
	()=>($crate::serial_print!("\n"));
	($($arg:tt)*) => {$crate::serial_print!("{}\n", format_args!($($arg)*))
	};
}

/*
 * _serial_print - Internal helper for formatted serial output
 * @args: Format arguments from format_args! macro
 */
pub fn _serial_print(args: core::fmt::Arguments){
	use core::fmt::Write;

	struct SerialWriter;

	impl Write for SerialWriter {
		fn write_str(&mut self, s: &str) -> core::fmt::Result{
			serial_print(s);
			Ok(())
		}
	}
	SerialWriter.write_fmt(args).ok();
}