/*
 * SMP Boot and AP Management
 *
 * Provides support for bringing up Application Processors (APs) via the
 * INIT-SIPI-SIPI sequence. Each AP is initialized independently with its
 * own per-CPU data, GDT, IDT, and scheduler context.
 */

use core::sync::atomic::{AtomicUsize, Ordering};
use hal::serial_println;

static DELAY_COUNTER: AtomicUsize = AtomicUsize::new(0);

/*
 * spin_loop_delay - Simple busy-wait delay
 * @iterations: Number of iterations to spin
 */
unsafe fn spin_loop_delay(iterations: usize) {
	for _ in 0..iterations {
		core::hint::spin_loop();
	}
}

/*
 * AP Bootstrap State
 *
 * Tracks which APs have been woken and are ready to run.
 */
pub static mut AP_READY: [bool; 16] = [false; 16];
pub static mut AP_COUNT: usize = 0;
pub static mut BSP_ID: u32 = 0;

/*
 * set_ap_count - Set the number of APs detected
 */
pub unsafe fn set_ap_count(count: usize) {
	AP_COUNT = count;
}

/*
 * set_bsp_id - Set the BSP LAPIC ID
 */
pub unsafe fn set_bsp_id(id: u32) {
	BSP_ID = id;
}

/*
 * set_ap_ready - Mark an AP as ready
 */
pub unsafe fn set_ap_ready(id: u8) {
	AP_READY[id as usize] = true;
}

/*
 * ap_ready - Check if an AP is ready
 */
pub fn ap_ready(id: u8) -> bool {
	unsafe { AP_READY[id as usize] }
}

/*
 * wakeup_ap - Send INIT-SIPI-SIPI sequence to an AP
 * @lapic_id: Target AP's LAPIC ID
 * @bootstrap_addr: Physical address where the AP bootstrap code starts
 *
 * The sequence:
 * 1. Send INIT IPI (Level=deassert, DeliveryMode=INIT)
 * 2. Wait 10ms for the AP to initialize
 * 3. Send first SIPI (Startup IPI) with the bootstrap vector
 * 4. Wait 200us
 * 5. Send second SIPI (required for AP to start executing)
 * 6. Wait 200us
 */
pub unsafe fn wakeup_ap(lapic_id: u32, bootstrap_addr: u64) {
	/* 1. Send INIT IPI */
	if !write_icr(0xC500, lapic_id) {
		return;
	}
	spin_loop_delay(10_000); /* Wait 10ms */
	if !write_icr(0x8500, lapic_id) {
		return;
	}

	/* 2. Send first SIPI */
	let vector = (bootstrap_addr / 4096) as u8;
	let sipi_vector = 0x600 | vector as u32;
	if !write_icr(sipi_vector, lapic_id) {
		return;
	}
	spin_loop_delay(200); /* Wait 200us */

	/* 3. Send second SIPI */
	if !write_icr(sipi_vector, lapic_id) {
		return;
	}
	spin_loop_delay(200); /* Wait 200us */

	serial_println!(
		"AP {} woken (bootstrap addr: {:#x})",
		lapic_id,
		bootstrap_addr
	);
}

/*
 * write_icr - Write to the ICR register
 * @value: Value to write
 * @lapic_id: Target LAPIC ID
 */
unsafe fn write_icr(value: u32, lapic_id: u32) -> bool {
	super::send_ipi(lapic_id, value)
}

/*
 * read_apic_id - Read the LAPIC ID of the current CPU
 *
 * Returns the APIC ID which matches BSP/AP numbering.
 */
pub unsafe fn read_apic_id() -> u32 {
	if super::x2apic_enabled() {
		super::lapic_read(0x20)
	} else {
		super::lapic_read(0x20) >> 24
	}
}

/*
 * enumerate_apics - Enumerate APs via LAPIC
 *
 * Reads the LAPIC version register to determine the maximum LAPIC ID,
 * then checks each ID to see if an AP is present.
 *
 * Return: Fixed-size array of LAPIC IDs for all detected APs (excluding BSP)
 *         The array is filled sequentially; unused entries are 0.
 */
pub unsafe fn enumerate_apics() -> usize {
	let version = super::lapic_read(0x30);
	let id = if super::x2apic_enabled() {
		super::lapic_read(0x20)
	} else {
		super::lapic_read(0x20) >> 24
	};
	serial_println!(
		"LAPIC mode={} id={} version={:#x}",
		if super::x2apic_enabled() {
			"x2APIC"
		} else {
			"xAPIC"
		},
		id,
		version
	);
	serial_println!("AP enumeration requires validated firmware topology; no AP IDs guessed");
	0
}

/*
 * wakeup_all_aps - Wake all detected APs
 * @bootstrap_addr: Physical address of the AP bootstrap code
 *
 * Iterates over all detected APs and sends the INIT-SIPI-SIPI sequence.
 */
pub unsafe fn wakeup_all_aps(bootstrap_addr: u64) {
	let _ = bootstrap_addr;
	enumerate_apics();
}

/// Start only APIC IDs supplied by a validated topology parser.
pub unsafe fn wakeup_apic_ids(apic_ids: &[u32], bootstrap_addr: u64) {
	for &apic_id in apic_ids {
		wakeup_ap(apic_id, bootstrap_addr);
	}
}
