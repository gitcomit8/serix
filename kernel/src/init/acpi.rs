use boot::MemoryRegion;
use boot::MemoryRegionType;
use acpi::{PlatformTopology, Rsdp, Sdt};
use hal::serial_println;

/// Discover firmware tables after the HHDM is active. Invalid firmware disables
/// ACPI-dependent setup, while the existing BSP path remains available.
pub unsafe fn discover(boot_info: &'static boot::BootInfo, hhdm_offset: u64) -> Option<PlatformTopology> {
	let rsdp_address = boot_info.rsdp_address;
	if rsdp_address == 0 {
		serial_println!("[ACPI] No RSDP provided by bootloader; firmware features disabled");
		return None;
	}

	let entries = boot_info.memory_regions;
	let (rsdp_phys, rsdp_virt) = if hhdm_offset != 0 && rsdp_address >= hhdm_offset {
		(rsdp_address - hhdm_offset, rsdp_address)
	} else {
		let Some(virt) = hhdm_offset.checked_add(rsdp_address) else { return None; };
		(rsdp_address, virt)
	};
	serial_println!("[ACPI] RSDP virtual={:#x} physical={:#x}", rsdp_virt, rsdp_phys);
	if !covered(entries, rsdp_phys, 36) {
		serial_println!("[ACPI] RSDP address outside memory map");
		return None;
	}
	let rsdp_bytes = core::slice::from_raw_parts(rsdp_virt as *const u8, 36);
	let revision = *rsdp_bytes.get(15)?;
	let rsdp_len = if revision == 0 { 20 } else { 36 };
	let rsdp = match Rsdp::parse(&rsdp_bytes[..rsdp_len]) {
		Ok(rsdp) => rsdp,
		Err(err) => { serial_println!("[ACPI] invalid RSDP: {:?}", err); return None; }
	};
	let (root_phys, root_sig) = rsdp.xsdt_address.filter(|a| *a != 0).map(|a| (a, *b"XSDT"))
		.unwrap_or((rsdp.rsdt_address as u64, *b"RSDT"));
	let root = match read_sdt(root_phys, root_sig, entries, hhdm_offset) {
		Ok(root) => root,
		Err(err) => { serial_println!("[ACPI] invalid root table: {:?}", err); return None; }
	};
	let addresses = match root.root_entries() {
		Ok(entries) => entries,
		Err(err) => { serial_println!("[ACPI] invalid root entries: {:?}", err); return None; }
	};
	let mut topology = None;
	let mut dmar_found = false;
	let mut fadt_found = false;
	for address in addresses {
		if !covered(entries, address, 36) { continue; }
		let header_virt = match hhdm_offset.checked_add(address) { Some(v) => v, None => continue };
		let header = core::slice::from_raw_parts(header_virt as *const u8, 36);
		let sig: [u8; 4] = match header[..4].try_into() { Ok(s) => s, Err(_) => continue };
		if !matches!(&sig, b"APIC" | b"DMAR" | b"FACP") { continue; }
		match read_sdt(address, sig, entries, hhdm_offset) {
			Ok(table) if sig == *b"APIC" => match PlatformTopology::parse_madt(table) {
				Ok(parsed) => topology = Some(parsed),
				Err(err) => serial_println!("[ACPI] MADT rejected: {:?}", err),
			},
			Ok(table) if sig == *b"DMAR" => {
				dmar_found = true;
				match acpi::parse_dmar(table) { Ok(units) => serial_println!("[ACPI] validated DMAR: {} remapping unit(s), backend not enabled", units.len()), Err(err) => serial_println!("[ACPI] DMAR rejected: {:?}", err) }
			},
			Ok(table) if sig == *b"FACP" => {
				fadt_found = true;
				match acpi::parse_fadt(table) { Ok(fadt) => serial_println!("[ACPI] validated FADT: PM1a control {:#x}, reset register {}", fadt.pm1a_control_block, fadt.reset_register.is_some()), Err(err) => serial_println!("[ACPI] FADT rejected: {:?}", err) }
			},
			Ok(_) => {},
			Err(err) => serial_println!("[ACPI] child table rejected: {:?}", err),
		}
	}
	if !dmar_found { serial_println!("[ACPI] no DMAR table; firmware has not exposed an IOMMU"); }
	if !fadt_found { serial_println!("[ACPI] no FADT table; ACPI power controls unavailable"); }
	if let Some(ref t) = topology {
		serial_println!("[ACPI] MADT topology: {} CPU entries, {} I/O APIC(s), LAPIC {:#x}", t.cpus.len(), t.io_apics.len(), t.lapic_address);
		for (logical, cpu) in t.cpus.iter().enumerate() {
			serial_println!("[ACPI] cpu logical={} apic={} enabled={} hotplug_capable={}", logical, cpu.apic_id, cpu.enabled, cpu.online_capable);
		}
	} else {
		serial_println!("[ACPI] no valid MADT; retain degraded legacy APIC setup");
	}
	topology
}

unsafe fn read_sdt<'a>(phys: u64, expected: [u8; 4], entries: &[MemoryRegion], hhdm: u64) -> Result<Sdt<'a>, acpi::Error> {
	if !covered(entries, phys, 36) { return Err(acpi::Error::Truncated); }
	let address = hhdm.checked_add(phys).ok_or(acpi::Error::BadLength)?;
	let header = core::slice::from_raw_parts(address as *const u8, 36);
	let length = u32::from_le_bytes(header[4..8].try_into().map_err(|_| acpi::Error::Truncated)?) as usize;
	if length < 36 || length > 1024 * 1024 || !covered(entries, phys, length) { return Err(acpi::Error::BadLength); }
	let bytes = core::slice::from_raw_parts(address as *const u8, length);
	Sdt::parse(bytes, Some(expected))
}

fn covered(entries: &[MemoryRegion], base: u64, length: usize) -> bool {
	let Some(end) = base.checked_add(length as u64) else { return false; };
	entries.iter().any(|entry| {
		let Some(region_end) = entry.base.checked_add(entry.length) else { return false; };
		base >= entry.base && end <= region_end && entry.region_type != MemoryRegionType::Bad
	})
}
