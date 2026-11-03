#![no_std]

extern crate alloc;

use alloc::vec::Vec;

pub const MAX_CPUS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
	Truncated,
	BadSignature,
	BadChecksum,
	BadLength,
	BadRevision,
	BadEntry,
	TooManyCpus,
	DuplicateApicId,
	Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rsdp<'a> {
	pub revision: u8,
	pub rsdt_address: u32,
	pub xsdt_address: Option<u64>,
	bytes: &'a [u8],
}

impl<'a> Rsdp<'a> {
	pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
		if bytes.len() < 20 {
			return Err(Error::Truncated);
		}
		if &bytes[..8] != b"RSD PTR " {
			return Err(Error::BadSignature);
		}
		if checksum(&bytes[..20]) != 0 {
			return Err(Error::BadChecksum);
		}
		let revision = bytes[15];
		if revision == 0 {
			return Ok(Self {
				revision,
				rsdt_address: le32(bytes, 16)?,
				xsdt_address: None,
				bytes: &bytes[..20],
			});
		}
		if bytes.len() < 36 {
			return Err(Error::Truncated);
		}
		let length = le32(bytes, 20)? as usize;
		if !(36..=bytes.len()).contains(&length) {
			return Err(Error::BadLength);
		}
		if checksum(&bytes[..length]) != 0 {
			return Err(Error::BadChecksum);
		}
		Ok(Self {
			revision,
			rsdt_address: le32(bytes, 16)?,
			xsdt_address: Some(le64(bytes, 24)?),
			bytes: &bytes[..length],
		})
	}

	pub fn raw_bytes(&self) -> &'a [u8] {
		self.bytes
	}
}

#[derive(Clone, Copy, Debug)]
pub struct Sdt<'a> {
	pub signature: [u8; 4],
	pub revision: u8,
	pub body: &'a [u8],
}

impl<'a> Sdt<'a> {
	pub fn parse(bytes: &'a [u8], expected: Option<[u8; 4]>) -> Result<Self, Error> {
		if bytes.len() < 36 {
			return Err(Error::Truncated);
		}
		let length = le32(bytes, 4)? as usize;
		if length < 36 || length > bytes.len() {
			return Err(Error::BadLength);
		}
		if checksum(&bytes[..length]) != 0 {
			return Err(Error::BadChecksum);
		}
		let signature: [u8; 4] = bytes[..4].try_into().map_err(|_| Error::Truncated)?;
		if expected.is_some_and(|e| e != signature) {
			return Err(Error::BadSignature);
		}
		Ok(Self {
			signature,
			revision: bytes[8],
			body: &bytes[36..length],
		})
	}

	pub fn root_entries(&self) -> Result<Vec<u64>, Error> {
		let width = match &self.signature {
			b"XSDT" => 8,
			b"RSDT" => 4,
			_ => return Err(Error::BadSignature),
		};
		if self.body.len() % width != 0 {
			return Err(Error::BadLength);
		}
		let entries: Vec<u64> = self.body
			.chunks_exact(width)
			.map(|c| {
				if width == 8 {
					Ok(u64::from_le_bytes(
						c.try_into().map_err(|_| Error::Truncated)?,
					))
				} else {
					Ok(u32::from_le_bytes(c.try_into().map_err(|_| Error::Truncated)?) as u64)
				}
			})
			.collect::<Result<_, _>>()?;
		if entries.iter().any(|entry| *entry == 0) {
			return Err(Error::BadLength);
		}
		Ok(entries)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cpu {
	pub apic_id: u32,
	pub uid: u32,
	pub enabled: bool,
	pub online_capable: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IoApic {
	pub id: u8,
	pub address: u32,
	pub gsi_base: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqOverride {
	pub bus: u8,
	pub source: u8,
	pub gsi: u32,
	pub flags: u16,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlatformTopology {
	pub lapic_address: u64,
	pub cpus: Vec<Cpu>,
	pub io_apics: Vec<IoApic>,
	pub irq_overrides: Vec<IrqOverride>,
}

impl PlatformTopology {
	pub fn parse_madt(table: Sdt<'_>) -> Result<Self, Error> {
		if table.signature != *b"APIC" {
			return Err(Error::BadSignature);
		}
		if table.body.len() < 8 {
			return Err(Error::Truncated);
		}
		let mut out = Self {
			lapic_address: le32(table.body, 0)? as u64,
			..Self::default()
		};
		let mut pos = 8;
		while pos < table.body.len() {
			if pos + 2 > table.body.len() {
				return Err(Error::BadEntry);
			}
			let ty = table.body[pos];
			let len = table.body[pos + 1] as usize;
			if len < 2
				|| pos
					.checked_add(len)
					.filter(|end| *end <= table.body.len())
					.is_none()
			{
				return Err(Error::BadEntry);
			}
			let e = &table.body[pos..pos + len];
			match ty {
				0 if len >= 8 => out.add_cpu(Cpu {
					uid: e[2] as u32,
					apic_id: e[3] as u32,
					enabled: le32(e, 4)? & 1 != 0,
					online_capable: false,
				})?,
				1 if len >= 12 => out.io_apics.push(IoApic {
					id: e[2],
					address: le32(e, 4)?,
					gsi_base: le32(e, 8)?,
				}),
				2 if len >= 10 => out.irq_overrides.push(IrqOverride {
					bus: e[2],
					source: e[3],
					gsi: le32(e, 4)?,
					flags: le16(e, 8)?,
				}),
				5 if len >= 12 => out.lapic_address = le64(e, 4)?,
				9 if len >= 16 => {
					let flags = le32(e, 8)?;
					out.add_cpu(Cpu {
						apic_id: le32(e, 4)?,
						uid: le32(e, 12)?,
						enabled: flags & 1 != 0,
						online_capable: flags & 2 != 0,
					})?;
				}
				_ => {}
			}
			pos += len;
		}
		Ok(out)
	}

	fn add_cpu(&mut self, cpu: Cpu) -> Result<(), Error> {
		if self.cpus.iter().any(|old| {
			old.apic_id == cpu.apic_id
				&& (old.enabled || old.online_capable)
				&& (cpu.enabled || cpu.online_capable)
		}) {
			return Err(Error::DuplicateApicId);
		}
		if self.cpus.len() == MAX_CPUS {
			return Err(Error::TooManyCpus);
		}
		self.cpus.push(cpu);
		Ok(())
	}

	pub fn cpu_by_apic_id(&self, id: u32) -> Option<(usize, &Cpu)> {
		self.cpus
			.iter()
			.enumerate()
			.find(|(_, cpu)| cpu.apic_id == id)
	}
	pub fn irq_to_gsi(&self, irq: u8) -> (u32, u16) {
		self.irq_overrides
			.iter()
			.find(|o| o.bus == 0 && o.source == irq)
			.map(|o| (o.gsi, o.flags))
			.unwrap_or((irq as u32, 0))
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmarUnit {
	pub segment: u16,
	pub flags: u8,
	pub register_base: u64,
}

pub fn parse_dmar(table: Sdt<'_>) -> Result<Vec<DmarUnit>, Error> {
	if table.signature != *b"DMAR" {
		return Err(Error::BadSignature);
	}
	if table.body.len() < 12 {
		return Err(Error::Truncated);
	}
	let mut units = Vec::new();
	let mut pos = 12;
	while pos < table.body.len() {
		if pos + 4 > table.body.len() {
			return Err(Error::BadEntry);
		}
		let ty = le16(table.body, pos)?;
		let len = le16(table.body, pos + 2)? as usize;
		if len < 4
			|| pos
				.checked_add(len)
				.filter(|end| *end <= table.body.len())
				.is_none()
		{
			return Err(Error::BadEntry);
		}
		if ty == 0 {
			if len < 16 {
				return Err(Error::BadEntry);
			}
			let e = &table.body[pos..pos + len];
			units.push(DmarUnit {
				flags: e[4],
				segment: le16(e, 6)?,
				register_base: le64(e, 8)?,
			});
		}
		pos += len;
	}
	Ok(units)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenericAddress {
	pub space_id: u8,
	pub bit_width: u8,
	pub bit_offset: u8,
	pub access_size: u8,
	pub address: u64,
}

pub fn parse_fadt(table: Sdt<'_>) -> Result<Fadt, Error> {
	if table.signature != *b"FACP" {
		return Err(Error::BadSignature);
	}
	let b = table.body;
	if b.len() < 80 {
		return Err(Error::BadLength);
	}
	let reset = if b.len() >= 92 && b[80] != 0 {
		Some(GenericAddress {
			space_id: b[80],
			bit_width: b[81],
			bit_offset: b[82],
			access_size: b[83],
			address: le64(b, 84)?,
		})
	} else {
		None
	};
	let reset_value = if b.len() > 92 { Some(b[92]) } else { None };
	Ok(Fadt {
		pm1a_control_block: le32(b, 28)?,
		pm1a_control_length: b[53],
		reset_register: reset,
		reset_value,
	})
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fadt {
	pub pm1a_control_block: u32,
	pub pm1a_control_length: u8,
	pub reset_register: Option<GenericAddress>,
	pub reset_value: Option<u8>,
}

fn checksum(bytes: &[u8]) -> u8 {
	bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b))
}
fn le16(b: &[u8], o: usize) -> Result<u16, Error> {
	Ok(u16::from_le_bytes(
		b.get(o..o + 2)
			.ok_or(Error::Truncated)?
			.try_into()
			.map_err(|_| Error::Truncated)?,
	))
}
fn le32(b: &[u8], o: usize) -> Result<u32, Error> {
	Ok(u32::from_le_bytes(
		b.get(o..o + 4)
			.ok_or(Error::Truncated)?
			.try_into()
			.map_err(|_| Error::Truncated)?,
	))
}
fn le64(b: &[u8], o: usize) -> Result<u64, Error> {
	Ok(u64::from_le_bytes(
		b.get(o..o + 8)
			.ok_or(Error::Truncated)?
			.try_into()
			.map_err(|_| Error::Truncated)?,
	))
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
	use super::*;
	use alloc::vec;
	fn sdt(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
		let mut t = vec![0u8; 36];
		t[..4].copy_from_slice(sig);
		t[8] = 1;
		t.extend_from_slice(body);
		let n = t.len() as u32;
		t[4..8].copy_from_slice(&n.to_le_bytes());
		let sum = checksum(&t);
		t[9] = 0u8.wrapping_sub(sum);
		t
	}
	#[test]
	fn validates_legacy_and_extended_rsdp_checksums() {
		let mut legacy = vec![0u8; 20];
		legacy[..8].copy_from_slice(b"RSD PTR ");
		legacy[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
		legacy[8] = 0u8.wrapping_sub(checksum(&legacy));
		assert_eq!(Rsdp::parse(&legacy).unwrap().rsdt_address, 0x1000);

		let mut extended = vec![0u8; 36];
		extended[..8].copy_from_slice(b"RSD PTR ");
		extended[15] = 2;
		extended[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
		extended[20..24].copy_from_slice(&36u32.to_le_bytes());
		extended[24..32].copy_from_slice(&0x2000u64.to_le_bytes());
		extended[8] = 0u8.wrapping_sub(checksum(&extended[..20]));
		extended[32] = 0u8.wrapping_sub(checksum(&extended));
		assert_eq!(Rsdp::parse(&extended).unwrap().xsdt_address, Some(0x2000));
		extended[32] ^= 1;
		assert!(matches!(Rsdp::parse(&extended), Err(Error::BadChecksum)));
	}
	#[test]
	fn validates_sdt_signature_length_checksum() {
		let t = sdt(b"TEST", &[]);
		assert!(Sdt::parse(&t, Some(*b"TEST")).is_ok());
		assert!(matches!(
			Sdt::parse(&t, Some(*b"NOPE")),
			Err(Error::BadSignature)
		));
		let mut bad = t;
		bad[9] ^= 1;
		assert!(matches!(Sdt::parse(&bad, None), Err(Error::BadChecksum)));
	}
	#[test]
	fn madt_keeps_sparse_apic_ids_and_overrides() {
		let mut body = vec![0u8; 8];
		body[..4].copy_from_slice(&0xfee00000u32.to_le_bytes());
		body.extend_from_slice(&[9, 16, 0, 0]);
		body.extend_from_slice(&0x80000001u32.to_le_bytes());
		body.extend_from_slice(&1u32.to_le_bytes());
		body.extend_from_slice(&44u32.to_le_bytes());
		body.extend_from_slice(&[2, 10, 0, 1]);
		body.extend_from_slice(&9u32.to_le_bytes());
		body.extend_from_slice(&0u16.to_le_bytes());
		let t = sdt(b"APIC", &body);
		let parsed = PlatformTopology::parse_madt(Sdt::parse(&t, Some(*b"APIC")).unwrap()).unwrap();
		assert_eq!(parsed.cpu_by_apic_id(0x80000001).unwrap().0, 0);
		assert_eq!(parsed.irq_to_gsi(1).0, 9);
	}
	#[test]
	fn rejects_invalid_madt_entry_and_duplicate_cpu() {
		let mut body = vec![0u8; 8];
		body.extend_from_slice(&[0, 1]);
		let t = sdt(b"APIC", &body);
		assert_eq!(
			PlatformTopology::parse_madt(Sdt::parse(&t, None).unwrap()),
			Err(Error::BadEntry)
		);
		let mut body = vec![0u8; 8];
		for _ in 0..2 {
			body.extend_from_slice(&[0, 8, 1, 2, 1, 0, 0, 0]);
		}
		let t = sdt(b"APIC", &body);
		assert_eq!(
			PlatformTopology::parse_madt(Sdt::parse(&t, None).unwrap()),
			Err(Error::DuplicateApicId)
		);
	}
	#[test]
	fn rejects_short_dmar_unit() {
		let mut body = vec![0u8; 12];
		body.extend_from_slice(&[0, 0, 4, 0]);
		let t = sdt(b"DMAR", &body);
		assert_eq!(
			parse_dmar(Sdt::parse(&t, None).unwrap()),
			Err(Error::BadEntry)
		);
	}
	#[test]
	fn parses_fadt_reset_gas_and_rejects_null_xsdt_pointer() {
		let mut body = vec![0u8; 93];
		body[28..32].copy_from_slice(&0x404u32.to_le_bytes());
		body[53] = 2;
		body[80] = 1;
		body[81] = 32;
		body[84..92].copy_from_slice(&0x1234u64.to_le_bytes());
		body[92] = 0xFE;
		let fadt_bytes = sdt(b"FACP", &body);
		let table = Sdt::parse(&fadt_bytes, None).unwrap();
		let fadt = parse_fadt(table).unwrap();
		assert_eq!(fadt.pm1a_control_block, 0x404);
		assert_eq!(fadt.reset_register.unwrap().address, 0x1234);
		assert_eq!(fadt.reset_value, Some(0xFE));

		let root_bytes = sdt(b"XSDT", &0u64.to_le_bytes());
		let bad_root = Sdt::parse(&root_bytes, None).unwrap();
		assert_eq!(bad_root.root_entries(), Err(Error::BadLength));
	}
}
