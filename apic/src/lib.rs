#![feature(abi_x86_interrupt)]
#![no_std]

use hal::serial_println;

pub mod ioapic;
pub mod timer;

const APIC_BASE: u64 = 0xFEE00000;

fn lapic_reg(offset: u32) -> *mut u32 {
    (APIC_BASE + offset as u64) as *mut u32
}

pub unsafe fn disable_pic() {
    use x86_64::instructions::port::Port;
    
    // Remap PIC to avoid conflicts, then mask all interrupts
    let mut pic1_cmd: Port<u8> = Port::new(0x20);
    let mut pic1_data: Port<u8> = Port::new(0x21);
    let mut pic2_cmd: Port<u8> = Port::new(0xA0);
    let mut pic2_data: Port<u8> = Port::new(0xA1);
    
    // Start initialization sequence
    pic1_cmd.write(0x11);
    pic2_cmd.write(0x11);
    
    // Remap IRQs to vectors 32-47
    pic1_data.write(0x20);
    pic2_data.write(0x28);
    
    // Set up cascading
    pic1_data.write(0x04);
    pic2_data.write(0x02);
    
    // Set 8086 mode
    pic1_data.write(0x01);
    pic2_data.write(0x01);
    
    // Mask all interrupts
    pic1_data.write(0xFF);
    pic2_data.write(0xFF);
    
    serial_println!("Legacy PIC disabled");
}

pub unsafe fn enable() {
    // Disable legacy PIC first
    disable_pic();
    
    // Check and enable LAPIC in IA32_APIC_BASE MSR (MSR 0x1B)
    let mut apic_base: u64;
    core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, lateout("eax") apic_base, lateout("edx") _);
    
    // Bit 11 = APIC Global Enable
    if (apic_base & (1 << 11)) == 0 {
        apic_base |= 1 << 11;
        let eax = (apic_base & 0xFFFFFFFF) as u32;
        let edx = ((apic_base >> 32) & 0xFFFFFFFF) as u32;
        core::arch::asm!("wrmsr", in("ecx") 0x1Bu32, in("eax") eax, in("edx") edx, options(nostack, nomem));
    }
    
    //Enable LAPIC by setting SVR, bit 8
    let svr = lapic_reg(0xF0);
    let val = svr.read_volatile() | 0x100; //Set bit 8
    svr.write_volatile(val);
    
    serial_println!("APIC enabled");
}

pub unsafe fn set_timer(vector: u8, divide: u32, initial_count: u32) {
    //Divide config, LVT Timer, Initial Count
    lapic_reg(0x3E0).write_volatile(divide);
    lapic_reg(0x320).write_volatile((vector as u32) | 0x20000);
    lapic_reg(0x380).write_volatile(initial_count);
}

pub unsafe fn send_eoi() {
    let eoi = lapic_reg(0xB0);
    eoi.write_volatile(0);
}
