use core::fmt::Write;
use crate::io::*;

//COM1 serial port base address
const COM1: u16 = 0x3F8;

//Serial port register offsets
const DATA_REG: u16 =0;         //Data register
const INT_EN_REG: u16 =1;       //Interrupt enable register
const FIFO_REG: u16 =2;         //FIFO control register
const LINE_CTRL_REG: u16 =3;    //Line control register
const MODEM_CTRL_REG: u16 =4;   //Modem Control register
const LINE_STATUS_REG: u16 =5;  //Line status register

pub struct SerialPort{
	base: u16,
}

impl SerialPort{
	//init COM1
	pub fn new()-> Self{
		let port=SerialPort{base:COM1};
		unsafe{
			port.init();
		}
		port
	}

	//init serial port with 115200 baud, 8N1
	unsafe fn init(&self){
		outb(self.base+INT_EN_REG, 0x00);       //Disable interrupts
		outb(self.base+LINE_CTRL_REG, 0x80);    //Enable DLAB
		outb(self.base+DATA_REG, 0x01);         //Low byte
		outb(self.base+INT_EN_REG, 0x0);        //High Byte
		outb(self.base+LINE_CTRL_REG, 0x3);     //8N1
		outb(self.base+FIFO_REG, 0xC7);         //enable fifo;clear;14 byte threshold
		outb(self.base+MODEM_CTRL_REG, 0x0B);   //IRQ enabled
	}

	//Check if transmit buffer is empty
	fn is_transmit_empty(&self)-> bool{
		unsafe {
			inb(self.base+LINE_STATUS_REG)&0x20!=0
		}
	}

	//Write a single byte to serial port
	pub fn write_byte(&self, byte:u8){
		while !self.is_transmit_empty(){
			core::hint::spin_loop();
		}

		unsafe{
			outb(self.base+DATA_REG, byte);
		}
	}

	//Write a string to serial port
	pub fn write_str(&self, s: &str){
		for byte in s.bytes(){
			self.write_byte(byte);
		}
	}
}

//Global serial port instance (lazy init)
use spin::Mutex;
use spin::Once;

static SERIAL_PORT: Once<Mutex<SerialPort>>=Once::new();

//init global serial port
pub fn init_serial(){
	SERIAL_PORT.call_once(|| Mutex::new(SerialPort::new()));
}

//Write string to serial port (thread-safe)
pub fn serial_print(s: &str){
	if let Some(serial)=SERIAL_PORT.get(){
		let port = serial.lock();
		port.write_str(s);
	}
}

//Serial print macro
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {$crate::serial::_serial_print(format_args!($($arg)*))
    };
}

//Serial println macro
#[macro_export]
macro_rules! serial_println {
	()=>($crate::serial_print!("\n"));
	($($arg:tt)*) => {$crate::serial_print!("{}\n", format_args!($($arg)*))
	};
}

//Internal function for serial printing with formatting
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