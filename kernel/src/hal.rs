#![allow(dead_code)]

pub mod cpu {
    use x86_64::instructions::*;

    #[inline(always)]
    pub fn halt() {
        hlt();
    }

    #[inline(always)]
    pub fn enable_interrupts() {
        interrupts::enable();
    }

    #[inline(always)]
    pub fn disable_interrupts() {
        interrupts::disable();
    }
}

pub mod io {
    use x86_64::instructions::port::Port;

    #[inline(always)]
    pub unsafe fn outb(port: u16, data: u8) {
        let mut port: Port<u8> = Port::new(port);
        port.write(data);
    }

    #[inline(always)]
    pub unsafe fn inb(port: u16) -> u8 {
        let mut port: Port<u8> = Port::new(port);
        port.read()
    }
}
