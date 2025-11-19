/*
 * CPU Topology Detection
 *
 * Detects CPU core types (Performance/Efficiency) using CPUID.
 * Useful for hybrid architectures like Intel Alder Lake and later.
 */

#![no_std]

use core::arch::asm;

/*
 * enum CoreType - CPU core type classification
 * @Performance: Performance core (P-core)
 * @Efficiency: Efficiency core (E-core)
 * @Unknown: Unknown or unsupported core type
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreType {
	Performance,
	Efficiency,
	Unknown,
}

/*
 * get_core_type - Detect the current CPU core type
 *
 * Uses CPUID leaf 0x1A to determine if running on a P-core or E-core.
 * Returns CoreType enum indicating the core type.
 */
pub fn get_core_type() -> CoreType {
	let eax: u32;
	let _ecx: u32;
	let _edx: u32;

	unsafe {
		asm!(
			"push rbx",
			"cpuid",
			"pop rbx",
			inout("eax") 0x1A => eax,
			lateout("ecx") _ecx,
			lateout("edx") _edx,
		);
	}

	/* Extract core type from bits 31:24 of EAX */
	match (eax >> 24) & 0xFF {
		0x20 => CoreType::Efficiency,
		0x40 => CoreType::Performance,
		_ => CoreType::Unknown,
	}
}
