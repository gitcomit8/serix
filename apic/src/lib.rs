#![feature(abi_x86_interrupt)]
#![no_std]

mod timer;

const APIC_BASE: u64 = 0xFEE00000;

fn lapic_reg(offset: u32) -> *mut u32 {
    (APIC_BASE + offset as u64) as *mut u32
}

pub unsafe fn enable() {
    //Enable LAPIC by setting SVR, bit 8
    let svr = lapic_reg(0xF0);
    let val = svr.read_volatile() | 0x100; //Set bit 8
    svr.write_volatile(val);
}

pub unsafe fn set_timer(vector: u8, divide: u32, initial_count: u32) {
    //Divide config, LVT Timer, Initial Count
    lapic_reg(0x3E0).write_volatile(divide);
    lapic_reg(0x320).write_volatile((vector as u32) | 0x200000);
    lapic_reg(0x380).write_volatile(initial_count);
}

pub unsafe fn send_eoi() {
    let eoi = lapic_reg(0xB0);
    eoi.write_volatile(0);
}
