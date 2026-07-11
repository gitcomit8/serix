/*
 * SMP (Symmetric Multiprocessing) Support
 *
 * Provides synchronization primitives for AP boot management.
 * The AP_READY_PTR flag signals to AP callbacks that BSP initialization
 * is complete and they may proceed with per-CPU setup.
 */

use core::sync::atomic::{AtomicBool, Ordering};

/* Signal to APs that BSP init is complete */
pub static AP_READY_PTR: AtomicBool = AtomicBool::new(false);

/*
 * set_ap_ready - Mark a specific AP as initialized
 * @id: LAPIC ID of the AP
 */
pub fn set_ap_ready(id: u8) {
	/* Currently unused — APs self-report via the callback */
	let _ = id;
}

/*
 * bsp_signal_aps - Signal all APs that init is complete
 *
 * Called by the BSP after all kernel subsystems are initialized.
 * APs waiting on AP_READY_PTR will proceed.
 */
pub fn bsp_signal_aps() {
	AP_READY_PTR.store(true, Ordering::Release);
}
