/*
 * elf.rs - ELF file format definitions
 *
 * Defines structures and constants for parsing ELF64 executables.
 */

// ELF Magic Number: 0x7F 'E' 'L' 'F'
pub const ELF_MAGIC: [u8; 4] = [0x7F, 0x45, 0x4C, 0x46];

/*
 * enum Machine - ELF machine architecture types
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Machine {
	X86_64 = 0x3E,
}

/*
 * enum SegmentType - ELF program header types
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SegmentType {
	Null = 0,
	Load = 1,
	Dynamic = 2,
	Interp = 3,
	Note = 4,
	Shlib = 5,
	Phdr = 6,
	Tls = 7,
}

/*
 * struct Elf64Header - ELF64 file header
 */
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Elf64Header {
	pub e_ident: [u8; 16],
	pub e_type: u16,
	pub e_machine: u16,
	pub e_version: u32,
	pub e_entry: u64, // Entry point virtual address
	pub e_phoff: u64, // Program header table file offset
	pub e_shoff: u64, // Section header table file offset
	pub e_flags: u32,
	pub e_ehsize: u16,
	pub e_phentsize: u16,
	pub e_phnum: u16,
	pub e_shentsize: u16,
	pub e_shnum: u16,
	pub e_shstrndx: u16,
}

/*
 * struct ProgramHeader - ELF64 program header
 */
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ProgramHeader {
	pub p_type: u32,
	pub p_flags: u32,
	pub p_offset: u64, // Segment file offset
	pub p_vaddr: u64,  // Segment virtual address
	pub p_paddr: u64,  // Segment physical address (ignored)
	pub p_filesz: u64, // Segment size in file
	pub p_memsz: u64,  // Segment size in memory
	pub p_align: u64,  // Segment alignment
}

/*
 * Permission flags for program headers
 */
pub const PF_X: u32 = 1; // Execute
pub const PF_W: u32 = 2; // Write
pub const PF_R: u32 = 4; // Read

impl Elf64Header {
	/*
	 * validate - Validate ELF header
	 *
	 * Checks magic number, class (64-bit), and machine type.
	 *
	 * Return: Ok(()) if valid, Err with message if invalid
	 */
	pub fn validate(&self) -> Result<(), &'static str> {
		if self.e_ident[0..4] != ELF_MAGIC {
			return Err("ELF magic doesn't match");
		}
		if self.e_ident[4] != 2 {
			return Err("Not 64-bit ELF");
		}
		if self.e_machine != Machine::X86_64 as u16 {
			return Err("Machine not x86_64");
		}
		Ok(())
	}
}
