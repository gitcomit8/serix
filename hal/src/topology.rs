#![no_std]

use core::arch::asm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreType {
	Performance,
	Efficiency,
	Unknown,
}

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

	match (eax >> 24) & 0xFF {
		0x20 => CoreType::Efficiency,
		0x40 => CoreType::Performance,
		_ => CoreType::Unknown,
	}
}
