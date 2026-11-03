/*
 * CPU Topology Detection
 *
 * Detects CPU core types (Performance/Efficiency) using CPUID.
 * Useful for hybrid architectures like Intel Alder Lake and later.
 */

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreTypeInfo {
	pub kind: CoreType,
	pub leaf_supported: bool,
	pub raw_type: u8,
}

/*
 * get_core_type - Detect the current CPU core type
 *
 * Uses CPUID leaf 0x1A to determine if running on a P-core or E-core.
 * Returns CoreType enum indicating the core type.
 */
pub fn get_core_type() -> CoreType {
	get_core_type_info().kind
}

/// Return the current CPU's CPUID leaf 0x1A classification and raw value.
/// Unknown is a valid result on CPUs and hypervisors that do not expose it.
pub fn get_core_type_info() -> CoreTypeInfo {
	let max_leaf = unsafe { core::arch::x86_64::__cpuid(0) }.eax;
	if max_leaf < 0x1A {
		return CoreTypeInfo { kind: CoreType::Unknown, leaf_supported: false, raw_type: 0 };
	}
	let eax = unsafe { core::arch::x86_64::__cpuid(0x1A) }.eax;
	let raw_type = (eax >> 24) as u8;
	let kind = match raw_type {
		0x20 => CoreType::Efficiency,
		0x40 => CoreType::Performance,
		_ => CoreType::Unknown,
	};
	CoreTypeInfo { kind, leaf_supported: true, raw_type }
}
