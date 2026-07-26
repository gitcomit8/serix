/*
 * init/smp.rs - SMP & Timer Initialization
 *
 * Provides init_smp() which handles:
 * - LAPIC timer hardware initialization
 * - Limine MP response parsing
 * - AP callback registration
 * - Signaling APs to begin per-CPU setup
 */

use core::sync::atomic::Ordering;
use hal::serial_println;
use limine::request::MpRequest;

/* Limine request for multiprocessor info */
static MP_REQ: MpRequest = MpRequest::new();

/*
 * init_smp - Initialize SMP timer and AP boot infrastructure
 *
 * Starts the LAPIC timer (~625 Hz) for preemptive scheduling,
 * registers each AP's boot callback via Limine MP, and signals
 * APs that BSP initialization is complete.
 */
pub unsafe fn init_smp() {
	/* Initialize timer hardware — starts preemptive scheduling */
	apic::timer::init_hardware();

	/* Get CPU count from Limine MP response and register AP callback */
	if let Some(mp_response) = MP_REQ.get_response() {
		let total_cpus = mp_response.cpus().len();
		let bsp_id = apic::smp::read_apic_id();
		serial_println!("BSP LAPIC ID: {}, Total CPUs: {}", bsp_id, total_cpus);

		/* Write ap_init_callback to each AP's goto_address, set CPU index in extra */
		for (i, cpu) in mp_response.cpus().iter().enumerate() {
			if i == 0 {
				/* BSP — skip, BSP is already executing */
				continue;
			}
			cpu.extra.store(i as u64, Ordering::Relaxed);
			cpu.goto_address.write(super::cpu::ap_init_callback);
		}

		if total_cpus > 1 {
			graphics::fb_println!("SMP: {} CPUs detected (Limine MP)", total_cpus);
		} else {
			graphics::fb_println!("SMP: Single-core mode");
		}
	} else {
		serial_println!("No MP response — single-core mode");
		graphics::fb_println!("SMP: No MP response (single-core mode)");
	}

	/* Signal APs that BSP init is complete — they will proceed with per-CPU setup */
	crate::smp::bsp_signal_aps();
}
