/*
 * init/smp.rs - SMP & Timer Initialization
 *
 * Provides init_smp() which handles:
 * - LAPIC timer hardware initialization
 * - BootInfo CPU list parsing
 * - AP configuration via IPI
 * - Signaling APs to begin per-CPU setup
 */

use core::sync::atomic::Ordering;
use hal::serial_println;

/*
 * init_smp - Initialize SMP timer and AP boot infrastructure
 * @boot_info: BootInfo from bootloader (validated)
 *
 * Starts the LAPIC timer (~625 Hz) for preemptive scheduling,
 * configures each AP via its APIC ID from BootInfo, and signals
 * APs that BSP initialization is complete.
 */
pub unsafe fn init_smp(boot_info: &'static boot::BootInfo) {
	/* Initialize timer hardware — starts preemptive scheduling */
	apic::timer::init_hardware();

	/* Get CPU info from BootInfo and configure each AP */
	if let Some(bsp_info) = boot_info.cpus.iter().find(|c| c.is_bsp) {
		let total_cpus = boot_info.cpus.len();
		let bsp_apic_id = bsp_info.apic_id;
		serial_println!(
			"[BOOT] BSP: logical=0 apic={} (from BootInfo)",
			bsp_apic_id
		);

		/* Configure BSP */
		let _ = apic::ipi::configure_cpu(0, bsp_apic_id);
		let _ = apic::ipi::set_cpu_online(0);

		/* Assign dense logical IDs to APs, independent of firmware order. */
		let mut next_logical_id = 1usize;
		for cpu in boot_info.cpus.iter() {
			if cpu.apic_id == bsp_apic_id {
				continue;
			}
			let logical_id = next_logical_id;
			next_logical_id += 1;
			if logical_id >= 16 {
				serial_println!(
					"CPU APIC ID {} exceeds configured per-CPU limit; leaving offline",
					cpu.apic_id
				);
				continue;
			}
			let _ = apic::ipi::configure_cpu(logical_id, cpu.apic_id);
			serial_println!(
				"[BOOT] AP: logical={} apic={}",
				logical_id, cpu.apic_id
			);
		}

		if total_cpus > 1 {
			graphics::fb_println!(
				"SMP: {} CPUs detected (BootInfo)",
				total_cpus
			);
		} else {
			graphics::fb_println!("SMP: Single-core mode");
		}
	} else {
		serial_println!("No BSP found in CPU list — single-core mode");
		graphics::fb_println!("SMP: No BSP in BootInfo (single-core mode)");
	}

	/* Signal APs that BSP init is complete — they will proceed with per-CPU setup */
	crate::smp::bsp_signal_aps();
}
