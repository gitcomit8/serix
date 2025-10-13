#![no_std]

//Basic US QWERTY scancode to ASCII mapping
const SCANDCODE_TO_ASCII: [u8; 128] = [
    0, 27, b'1', b'2', b'3', b'4', b'5', b'6',  // 0x00-0x07
    b'7', b'8', b'9', b'0', b'-', b'=', 8, b'\t', // 0x08-0x0F (backspace=8)
    b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i',  // 0x10-0x17
    b'o', b'p', b'[', b']', b'\n', 0, b'a', b's',  // 0x18-0x1F (enter=\n, ctrl=0)
    b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';',  // 0x20-0x27
    b'\'', b'`', 0, b'\\', b'z', b'x', b'c', b'v',  // 0x28-0x2F (lshift=0)
    b'b', b'n', b'm', b',', b'.', b'/', 0, b'*',   // 0x30-0x37 (rshift=0)
    0, b' ', 0, 0, 0, 0, 0, 0,           // 0x38-0x3F (alt=0, space)
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x40-0x4F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x50-0x5F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x60-0x6F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x70-0x7F
];

pub fn handle_scancode(scancode: u8) {
    if scancode & 0x80 != 0 {
        return; //Break code, ignore for now
    }

    if let Some(&ascii) = SCANDCODE_TO_ASCII.get(scancode as usize) {
        if ascii != 0 {
            hal::serial_print!("{}", ascii as char);
            graphics::fb_print!("{}", ascii as char);
        }
    }
}

pub fn enable_keyboard_interrupt() {
    unsafe {
        let mut port = x86_64::instructions::port::Port::new(0x21); //PIC1 data port
        let mask: u8 = port.read();
        port.write(mask & !0x02); //Clear bit 1 (IRQ1)
    }
}