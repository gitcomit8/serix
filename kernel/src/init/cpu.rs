
/*
 * ap_init_callback - Limine callback for each AP
 *
 * Called by Limine when an AP is booted. Each AP reads its CPU index
 * from the Cpu.extra field (set by BSP during registration), initializes
 * per-CPU data, allocates a kernel stack, sets up the scheduler, and
 * enters the idle loop.
 *
 * Uses AP_READY flag for synchronization: APs spin-wait until the BSP
 * signals that kernel initialization is complete.
 *
 * NOTE: This function must NEVER RETURN.
 */
use x86_64::structures::idt::InterruptStackFrame;
use hal::serial_println;
use crate::gdt::{init_per_cpu, set_kernel_stack};
use crate::smp;

#[unsafe(no_mangle)]
pub extern "C" fn ap_init_callback(cpu: &limine::mp::Cpu) -> ! {
    use core::arch::asm;
    use crate::gdt::{init_per_cpu, set_kernel_stack, PER_CPU_DATA};
    use task::scheduler;

    /* CPU index set by BSP in Cpu.extra */
    let cpu_id = cpu.extra.load(core::sync::atomic::Ordering::Relaxed) as usize;

    /* Mask timer interrupt — prevent premature timer IRQ before init complete */
    unsafe { apic::timer::mask_timer() };

    /* Wait for BSP to signal kernel init is complete */
    while !unsafe { smp::AP_READY_PTR.load(core::sync::atomic::Ordering::Acquire) } {
        core::hint::spin_loop();
    }

    /* Initialize per-CPU data */
    unsafe {
        init_per_cpu(cpu_id);
    }

    /* Allocate a kernel stack for this AP */
    let stack_top = memory::kstack::alloc_kernel_stack(4096)
        .expect("Failed to allocate AP kernel stack");

    /* Set kernel stack and TSS.RSP0 */
    set_kernel_stack(stack_top);

    /* Initialize scheduler for this CPU */
    unsafe {
        scheduler::init(core::ptr::addr_of!(PER_CPU_DATA) as usize, cpu_id as u8);
    }

    /* Enable interrupts for this AP */
    x86_64::instructions::interrupts::enable();

    /* Unmask timer interrupt — now safe to receive timer IRQs */
    unsafe { apic::timer::unmask_timer() };

    /* Mark this AP as ready in the SMP module */
    unsafe { smp::set_ap_ready(cpu_id as u8) };

    serial_println!("AP {} initialized, entering idle loop", cpu_id);

    /* Enter idle loop — timer interrupts drive preemptive scheduling */
    loop {
        x86_64::instructions::hlt();
    }
}



/*
 * keyboard_interrupt_handler - Handle keyboard interrupts (IRQ 1, vector 33)
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Reads scancode from keyboard controller and sends EOI to APIC.
 * Defined here (not in idt module) to avoid circular dependency with apic.
 */
pub extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    /* Read scancode from keyboard data port (0x60) */
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    /* Process the scancode via keyboard module */
    keyboard::handle_scancode(scancode);

    /* Send End of Interrupt to Local APIC */
    unsafe {
        apic::send_eoi();
    }
}


/*
 * init_ps2_keyboard - Initialize PS/2 keyboard controller
 *
 * Enables the PS/2 keyboard and clears any stale data in the buffer.
 * Required for keyboard interrupts to function properly.
 */
pub unsafe fn init_ps2_keyboard() {
    use x86_64::instructions::port::Port;

    let mut cmd_port = Port::new(0x64); // PS/2 command port
    let mut data_port = Port::new(0x60); // PS/2 data port

    // Flush output buffer
    let _ = data_port.read();

    // Enable keyboard (command 0xAE)
    cmd_port.write(0xAE_u8);

    // Read controller configuration
    cmd_port.write(0x20_u8);
    for _ in 0..1000 {
        let status: u8 = cmd_port.read();
        if status & 0x01 != 0 {
            break;
        }
    }
    let mut config: u8 = data_port.read();

    // Enable keyboard interrupt (bit 0), keep scancode translation enabled (bit 6)
    config |= 0x01; // Enable keyboard interrupt
    config |= 0x40; // Enable scancode translation (Set 2 → Set 1)

    // Write configuration back
    cmd_port.write(0x60_u8);
    data_port.write(config);

    serial_println!("[PS/2] Keyboard controller initialized");
}

