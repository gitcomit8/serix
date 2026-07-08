/*
 * SMP (Symmetric Multiprocessing) Support
 *
 * Provides support for bringing up Application Processors (APs) via the
 * INIT-SIPI-SIPI sequence. Each AP is initialized independently with its
 * own per-CPU data, GDT, IDT, and scheduler context.
 */

/* Physical address where the AP bootstrap code is loaded */
pub const AP_BOOTSTRAP_ADDR: u64 = 0x1000;

/*
 * wakeup_all_aps - Wake all detected APs
 *
 * Iterates over all detected APs and sends the INIT-SIPI-SIPI sequence.
 * The AP bootstrap code at AP_BOOTSTRAP_ADDR is loaded into low memory.
 */
pub unsafe fn wakeup_all_aps() {
	let ap_count = apic::smp::enumerate_apics();
	for ap_id in 0..ap_count {
		apic::smp::wakeup_ap(ap_id as u8, AP_BOOTSTRAP_ADDR);
	}
}
