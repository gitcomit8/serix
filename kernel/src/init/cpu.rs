
/*
 * ap_init_callback - Boot entry point for each AP
 *
 * Called by the bootloader (or boot stub) for each AP. The logical CPU ID
 * is passed in RSI by the bootloader. Each AP initializes per-CPU data,
 * allocates a kernel stack, sets up the scheduler, and enters the idle loop.
 *
 * Uses AP_READY flag for synchronization: APs spin-wait until the BSP
 * signals that kernel initialization is complete.
 *
 * NOTE: This function must NEVER RETURN.
 */
use core::arch::asm;
use x86_64::structures::idt::InterruptStackFrame;
use hal::serial_println;
use crate::gdt::{init_per_cpu, set_kernel_stack};
use crate::smp;

#[unsafe(no_mangle)]
pub extern "C" fn ap_init_callback(cpu_id: usize) -> ! {
	/* Select the BSP's APIC mode on this CPU before any APIC register access. */
	if !unsafe { apic::enable_current_cpu() } {
		hal::serial_println!("AP startup stopped: local APIC mode mismatch");
		x86_64::instructions::interrupts::disable();
		loop { x86_64::instructions::hlt(); }
	}

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
		use crate::gdt::PER_CPU_DATA;
		task::scheduler::init(core::ptr::addr_of!(PER_CPU_DATA) as usize, cpu_id as u8);
	}
	let core_info = hal::topology::get_core_type_info();
	serial_println!(
		"[CPU] logical={} core_type={:?} leaf_1a={} raw={:#04x}",
		cpu_id,
		core_info.kind,
		core_info.leaf_supported,
		core_info.raw_type
	);

	/* Enable interrupts for this AP */
	x86_64::instructions::interrupts::enable();
	let _ = apic::ipi::set_cpu_online(cpu_id);

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

    /* Record that the ISR fired */
    keyboard::isr_interrupt_handler_called();

    /* Read scancode from keyboard data port (0x60) */
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    keyboard::record_scancode(scancode);

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
    use core::arch::x86_64::_mm_pause;

    /* Small spin delay to let PS/2 controller process commands. */
    fn delay_ps2() {
        for _ in 0..100 {
            _mm_pause();
        }
    }

    /* Read from an I/O port using inline assembly. */
    #[inline]
    unsafe fn io_in(port: u16) -> u8 {
        let val: u8;
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") val,
            options(preserves_flags, nostack),
        );
        val
    }

    /* Write to an I/O port using inline assembly. */
    #[inline]
    unsafe fn io_out(port: u16, val: u8) {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") val,
            options(preserves_flags, nostack),
        );
    }

    /* Wait for command buffer to be empty (bit 2 = 0). */
    fn wait_cmd_ready() {
        for _ in 0..10_000 {
            let status: u8 = unsafe { io_in(0x64) };
            if status & 0x02 == 0 {
                return;
            }
            _mm_pause();
        }
    }

    /* Wait for output buffer to have data (bit 0 = 1). */
    fn wait_output_ready() {
        for _ in 0..10_000 {
            let status: u8 = unsafe { io_in(0x64) };
            if status & 0x01 != 0 {
                return;
            }
            _mm_pause();
        }
    }

    serial_println!("[PS/2] Starting init sequence (raw I/O)...");

    /* Drain any pending output */
    for _ in 0..1000 {
        let status: u8 = unsafe { io_in(0x64) };
        if status & 0x01 != 0 {
            let _ = unsafe { io_in(0x60) };
            break;
        }
        _mm_pause();
    }

    /* Controller self-test (0xAA) → expect 0x55 */
    wait_cmd_ready();
    unsafe { io_out(0x64, 0xAA) };
    delay_ps2();
    wait_output_ready();
    let st_result: u8 = unsafe { io_in(0x60) };
    serial_println!("[PS/2] Self-test: {:#04x}", st_result);

    /* Disable keyboard (0xAD) */
    wait_cmd_ready();
    unsafe { io_out(0x64, 0xAD) };
    delay_ps2();

    /* Enable keyboard interface (0xAE) */
    wait_cmd_ready();
    unsafe { io_out(0x64, 0xAE) };
    delay_ps2();
    serial_println!("[PS/2] Sent 0xAE");

    /* Read the 8042 configuration byte (0x20). Bit 0 enables IRQ1;
     * bit 4 disables the first-port clock when set. */
    wait_cmd_ready();
    unsafe { io_out(0x64, 0x20) };
    delay_ps2();
    wait_output_ready();
    let config: u8 = unsafe { io_in(0x60) };
    let configured = (config | 0x01) & !0x10;
    serial_println!(
        "[PS/2] Config before: {:#04x} (IRQ1={}, keyboard clock={})",
        config,
        config & 0x01 != 0,
        config & 0x10 == 0
    );

    /* Explicitly enable IRQ1 and the keyboard clock in the controller. */
    wait_cmd_ready();
    unsafe { io_out(0x64, 0x60) };
    delay_ps2();
    wait_cmd_ready();
    unsafe { io_out(0x60, configured) };
    delay_ps2();
    wait_cmd_ready();
    unsafe { io_out(0x64, 0x20) };
    delay_ps2();
    wait_output_ready();
    let verified: u8 = unsafe { io_in(0x60) };
    serial_println!(
        "[PS/2] Config after: {:#04x} (IRQ1={}, keyboard clock={})",
        verified,
        verified & 0x01 != 0,
        verified & 0x10 == 0
    );

    /* Tell the keyboard device to start scanning and verify its ACK.
     * The controller's 0xAE command enables the port, but is not the
     * keyboard's Enable Scanning command. */
    wait_cmd_ready();
    unsafe { io_out(0x60, 0xF4) };
    let mut ack = None;
    for _ in 0..100_000 {
        let status: u8 = unsafe { io_in(0x64) };
        if status & 0x01 != 0 {
            ack = Some(unsafe { io_in(0x60) });
            break;
        }
        _mm_pause();
    }
    match ack {
        Some(0xFA) => serial_println!("[PS/2] Enable scanning acknowledged (0xfa)"),
        Some(byte) => serial_println!("[PS/2] Enable scanning response: {:#04x}", byte),
        None => serial_println!("[PS/2] Enable scanning timed out (no response)"),
    }
}
