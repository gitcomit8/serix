# Memory Layout Technical Specification

**Document Version:** 1.0  
**Last Updated:** 2025-10-13  
**Target Architecture:** x86_64  

## Table of Contents

1. [Overview](#overview)
2. [Virtual Memory Layout](#virtual-memory-layout)
3. [Physical Memory Layout](#physical-memory-layout)
4. [Memory Mapping Strategy](#memory-mapping-strategy)
5. [Page Table Structure](#page-table-structure)
6. [Memory Regions](#memory-regions)
7. [Address Translation](#address-translation)
8. [Memory Initialization](#memory-initialization)
9. [Frame Allocation](#frame-allocation)
10. [Heap Management](#heap-management)

---

## Overview

Serix uses a 4-level paging scheme mandated by x86_64 architecture with 48-bit virtual addresses. The kernel employs an offset page table strategy where all physical memory is mapped at a fixed virtual offset, enabling efficient address translation and direct physical memory access.

### Key Parameters

| Parameter | Value | Notes |
|-----------|-------|-------|
| Virtual Address Width | 48 bits | x86_64 canonical addresses |
| Physical Address Width | 52 bits (max) | Depends on CPU, typically 40-48 bits |
| Page Size | 4 KB (4096 bytes) | Standard page size |
| Huge Page Size | 2 MB (optional) | Not currently used |
| Giant Page Size | 1 GB (optional) | Not currently used |
| Physical Memory Offset | 0xFFFF_8000_0000_0000 | Virtual base for physical memory |

### Memory Model

- **Flat Memory Model**: No segmentation (except legacy requirements)
- **Paging**: 4-level page tables (PML4 → PDP → PD → PT)
- **Identity Mapping**: Not used (offset mapping instead)
- **Higher Half Kernel**: Kernel at high virtual addresses

---

## Virtual Memory Layout

### Canonical Address Space

x86_64 uses 48-bit virtual addresses, creating a "canonical address space" with a non-canonical hole in the middle:

```
Canonical Form:
0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF: User space    (Lower half, 128 TB)
0x0000_8000_0000_0000 - 0xFFFF_7FFF_FFFF_FFFF: Non-canonical (Invalid, 16,776,704 TB)
0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF: Kernel space  (Upper half, 128 TB)
```

**Canonical Addresses**: Bits 48-63 must be copies of bit 47.
- Lower half: Bits 48-63 are all 0
- Upper half: Bits 48-63 are all 1
- Non-canonical: Mixed bits 48-63 cause #GP fault

### Kernel Virtual Address Space Layout

```
0xFFFF_8000_0000_0000  ┌─────────────────────────────────────┐
                       │                                     │
                       │  Physical Memory Mapping            │
                       │  (All physical RAM mapped here)     │
                       │                                     │
                       │  Size: Varies by system RAM         │
                       │  (e.g., 0-16GB physical maps to     │
                       │   0xFFFF_8000_0000_0000 +           │
                       │   0xFFFF_8000_0000_0000 + 16GB)     │
                       │                                     │
0xFFFF_8000_0000_0000+ │                                     │
      [RAM_SIZE]       ├─────────────────────────────────────┤
                       │                                     │
                       │  Unused Virtual Space               │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │  Memory-Mapped I/O (Future)         │
                       │  (GPU framebuffers, PCIe BARs)      │
                       ├─────────────────────────────────────┤
                       │                                     │
                       │  Unused Virtual Space               │
                       │                                     │
0x4444_4454_0000       ├─────────────────────────────────────┤
                       │  Kernel Heap                        │
                       │  Size: 1 MB (configurable)          │
0x4444_4444_0000       ├─────────────────────────────────────┤
                       │                                     │
                       │  Unused Virtual Space               │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │  Kernel .text, .rodata, .data, .bss │
                       │  (Loaded by Limine bootloader)      │
                       │  Typical range: -2GB from top       │
                       ├─────────────────────────────────────┤
                       │                                     │
                       │  Kernel Stacks (per-task)           │
                       │  (Future: guard pages between)      │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │                                     │
                       │  Unused / Reserved                  │
                       │                                     │
0xFFFF_FFFF_FFFF_FFFF  └─────────────────────────────────────┘
```

### Detailed Region Breakdown

#### Physical Memory Mapping (0xFFFF_8000_0000_0000 + offset)

**Purpose**: Direct access to all physical memory from kernel.

**Mapping**: Linear, entire physical address space.

**Formula**:
```
Virtual Address = 0xFFFF_8000_0000_0000 + Physical Address
Physical Address = Virtual Address - 0xFFFF_8000_0000_0000
```

**Example**:
```
Physical 0x0000_0000_0010_0000 (1 MB)
→ Virtual 0xFFFF_8000_0010_0000

Physical 0x0000_0001_0000_0000 (4 GB)
→ Virtual 0xFFFF_8001_0000_0000
```

**Access**: All pages mapped with Present + Writable flags.

**Benefits**:
- Simple address translation (addition/subtraction)
- No temporary mappings needed
- Easy page table manipulation
- Efficient for kernel memory operations

**Limitations**:
- Consumes virtual address space equal to physical RAM
- Not suitable for >64 TB systems (exceeds 47-bit canonical limit)

#### Kernel Heap (0x4444_4444_0000 - 0x4444_4454_0000)

**Purpose**: Dynamic memory allocation for kernel data structures.

**Size**: 1 MB (1024 * 1024 bytes = 256 pages)

**Allocator**: Linked list allocator (first-fit)

**Mapped**: During boot, after frame allocator is initialized.

**Flags**: Present + Writable

**Usage**:
```rust
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;

let v = Vec::new();           // Allocates from heap
let b = Box::new(42);         // Allocates from heap
let s = String::from("hi");   // Allocates from heap
```

**Address Selection Rationale**:
- Not conflicting with user space (above 0x0000_7FFF_FFFF_FFFF)
- Not conflicting with physical mapping (below 0xFFFF_8000_0000_0000)
- Easy to recognize in debugging (repeated 0x44 pattern)
- Sufficient alignment for any data type

#### Kernel Image (Variable, loaded by Limine)

**Purpose**: Kernel executable code and data.

**Location**: Determined by Limine bootloader, typically in higher half.

**Typical Range**: 0xFFFF_FFFF_8000_0000 - 0xFFFF_FFFF_FFFF_FFFF (last 2 GB)

**Sections**:
- **.text**: Executable code (Read + Execute)
- **.rodata**: Read-only data (Read-only)
- **.data**: Initialized writable data (Read + Write)
- **.bss**: Zero-initialized data (Read + Write)

**Linker Script**: `kernel/linker.ld` defines section layout.

**Relocation**: Position-independent (PIE) or relocated by bootloader.

#### Kernel Stacks (Variable, per-task)

**Purpose**: Stack space for kernel execution and task contexts.

**Size**: 8 KB per task (default, configurable)

**Allocation**: Dynamic, from heap or dedicated region.

**Current Implementation**: Placeholder addresses (TODO: proper allocation).

**Future**:
- Guard pages between stacks (unmapped page to detect overflow)
- Per-CPU stacks for multiprocessor support
- Separate interrupt stacks (IST)

### User Space Layout (Future)

```
0x0000_0000_0000_0000  ┌─────────────────────────────────────┐
                       │  Null Page (Unmapped)               │
                       │  Size: 4 KB                         │
0x0000_0000_0000_1000  ├─────────────────────────────────────┤
                       │                                     │
                       │  User .text (Code)                  │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │  User .rodata (Read-only data)      │
                       ├─────────────────────────────────────┤
                       │  User .data (Initialized data)      │
                       ├─────────────────────────────────────┤
                       │  User .bss (Uninitialized data)     │
                       ├─────────────────────────────────────┤
                       │                                     │
                       │  User Heap (grows upward ↑)         │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │                                     │
                       │  Memory-mapped files, shared libs   │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │                                     │
                       │  Unused                             │
                       │                                     │
                       ├─────────────────────────────────────┤
                       │  User Stack (grows downward ↓)      │
                       │                                     │
0x0000_7FFF_FFFF_FFFF  └─────────────────────────────────────┘
```

**Note**: User space not yet implemented in current Serix version.

---

## Physical Memory Layout

### Memory Map from Bootloader

Limine provides a memory map with different region types:

```
Type                      Description
─────────────────────────────────────────────────────────────
USABLE                    Free RAM, available for allocation
RESERVED                  Reserved by hardware, do not use
ACPI_RECLAIMABLE          ACPI tables, can reclaim after parsing
ACPI_NVS                  ACPI Non-Volatile Storage, do not use
BAD_MEMORY                Defective RAM, do not use
BOOTLOADER_RECLAIMABLE    Bootloader code/data, can reclaim after boot
KERNEL_AND_MODULES        Kernel image and modules, in use
FRAMEBUFFER               Framebuffer, in use
```

### Typical Physical Memory Layout

```
Physical Address          Region                      Size
─────────────────────────────────────────────────────────────────
0x0000_0000_0000_0000     IVT / BDA (legacy)          4 KB
0x0000_0000_0000_1000     Low memory                  639 KB
0x0000_0000_0009_FC00     EBDA (Extended BIOS Data)   1 KB
0x0000_0000_000A_0000     VGA memory                  128 KB
0x0000_0000_000C_0000     Video BIOS                  32 KB
0x0000_0000_000C_8000     Expansion ROMs              160 KB
0x0000_0000_000F_0000     System BIOS                 64 KB
0x0000_0000_0010_0000     Extended memory start       (1 MB boundary)
    ...                   Usable RAM                  (varies)
0x0000_0000_[KERNEL]      Kernel image                (e.g., 2 MB)
    ...                   More usable RAM             (varies)
0x0000_00[FB_START]       Framebuffer                 (e.g., 8 MB)
    ...                   More usable RAM             (varies)
[TOTAL_RAM - RESERVED]    Reserved regions            (varies)
[TOTAL_RAM]               End of RAM                  (e.g., 16 GB)
0x0000_00FE_C0_0000       I/O APIC MMIO               4 KB
0x0000_00FE_E0_0000       Local APIC MMIO             4 KB
0x0000_00[PCI_BARs]       PCI device BARs             (varies)
```

### Low Memory Regions (0x0000 - 0xFFFF)

**Historical x86 Compatibility Regions**:

| Address Range | Size | Region | Usage |
|---------------|------|--------|-------|
| 0x0000-0x03FF | 1 KB | Interrupt Vector Table (IVT) | Legacy real mode interrupts |
| 0x0400-0x04FF | 256 B | BIOS Data Area (BDA) | BIOS variables |
| 0x0500-0x9FBFF | ~638 KB | Conventional Memory | Available for DOS programs (unused by Serix) |
| 0x9FC00-0x9FFFF | 1 KB | Extended BIOS Data Area (EBDA) | BIOS extensions |
| 0xA0000-0xBFFFF | 128 KB | VGA Framebuffer | Legacy VGA text/graphics memory |
| 0xC0000-0xC7FFF | 32 KB | Video BIOS ROM | VGA BIOS code |
| 0xC8000-0xEFFFF | 160 KB | Expansion ROM Space | Add-in card ROMs |
| 0xF0000-0xFFFFF | 64 KB | System BIOS ROM | Main BIOS code |

**Serix Policy**: 
- Does not use low memory (< 1 MB) for kernel allocations
- Leaves it reserved for potential legacy hardware needs
- All kernel allocations from extended memory (>= 1 MB)

### Extended Memory (0x100000+)

**Usable RAM Regions**:
- Identified by bootloader memory map
- Filtered for `USABLE` type
- Excluded if overlapping with kernel, framebuffer, or reserved regions

**Memory Holes**:
- MMIO regions (PCI device BARs, APIC registers)
- Reserved by firmware (ACPI tables, SMM)
- Bad memory (detected by BIOS/UEFI memory test)

---

## Memory Mapping Strategy

### Offset Page Table Approach

**Concept**: Map all physical memory at a fixed virtual offset.

**Advantages**:
1. **Simple Translation**: Virtual ↔ Physical conversion is addition/subtraction
2. **No Temporary Mappings**: Can access any physical address directly
3. **Efficient Page Table Ops**: Easy to manipulate page tables
4. **DMA Buffer Access**: Can provide physical addresses for DMA

**Disadvantages**:
1. **Virtual Space Consumption**: Uses virtual address space equal to physical RAM
2. **Scalability**: Limited to ~64 TB on 48-bit addresses
3. **Security**: All physical memory accessible (mitigated by kernel-only access)

### Alternative Strategies (Not Used)

#### Identity Mapping
```
Virtual Address = Physical Address
```
**Problem**: Conflicts with user space (0x0 region), wastes low addresses.

#### On-Demand Mapping
```
Map physical pages only when needed, unmapping when done
```
**Problem**: Complex, requires TLB shootdowns, expensive.

#### Multiple Offset Mappings
```
Different physical regions at different offsets
```
**Problem**: More complex translation, multiple offset values to track.

### Current Mapping Formula

```
Physical → Virtual (Kernel Access):
  virtual_addr = 0xFFFF_8000_0000_0000 + physical_addr

Virtual → Physical (Reverse Translation):
  physical_addr = virtual_addr - 0xFFFF_8000_0000_0000

Range Check:
  is_physical_mapping = (virtual_addr >= 0xFFFF_8000_0000_0000) &&
                        (virtual_addr < 0xFFFF_8000_0000_0000 + total_ram)
```

---

## Page Table Structure

### 4-Level Paging Hierarchy

```
Virtual Address (48-bit canonical):
┌────────┬──────────┬──────────┬──────────┬──────────┬──────────────┐
│  Sign  │  PML4    │   PDP    │    PD    │    PT    │    Offset    │
│ Extend │  Index   │  Index   │  Index   │  Index   │              │
├────────┼──────────┼──────────┼──────────┼──────────┼──────────────┤
│ 63-48  │  47-39   │  38-30   │  29-21   │  20-12   │    11-0      │
│ 16 bit │   9 bit  │   9 bit  │   9 bit  │   9 bit  │    12 bit    │
└────────┴──────────┴──────────┴──────────┴──────────┴──────────────┘
  Copies    512       512        512        512         4096 bytes
  of bit   entries   entries    entries    entries
    47
```

### Page Table Entry Format (64-bit)

```
┌──┬──────────┬───────────────────────────────────────┬──────────────┐
│63│ 62-52    │         51-12                         │    11-0      │
├──┼──────────┼───────────────────────────────────────┼──────────────┤
│NX│ Reserved │     Physical Page Frame Number        │    Flags     │
│  │          │         (40 bits, 4KB aligned)         │              │
└──┴──────────┴───────────────────────────────────────┴──────────────┘
```

**Flag Bits (0-11)**:

| Bit | Name | Symbol | Description |
|-----|------|--------|-------------|
| 0 | Present | P | Page is present in memory |
| 1 | Writable | R/W | 0=Read-only, 1=Read-write |
| 2 | User | U/S | 0=Supervisor (kernel), 1=User accessible |
| 3 | Write-Through | PWT | Page-level write-through caching |
| 4 | Cache Disable | PCD | Page-level cache disable |
| 5 | Accessed | A | Page has been accessed (set by CPU) |
| 6 | Dirty | D | Page has been written to (set by CPU, PT only) |
| 7 | Page Size | PS | 1=Large page (PD/PDP), 0=Standard page |
| 8 | Global | G | Page is global (not flushed on CR3 reload) |
| 9-11 | Available | AVL | Available for OS use |
| 63 | No Execute | NX | Page is not executable (requires IA32_EFER.NXE) |

### CR3 Register (Page Table Base)

```
┌────────────────────────────────────────┬──────────┬──┬──┬──┬──┬────┐
│    PML4 Physical Address [51-12]       │   Res    │ 0│0 │CD│WT│ Res│
├────────────────────────────────────────┼──────────┼──┼──┼──┼──┼────┤
│                63-12                   │   11-5   │ 4│ 3│ 2│ 1│  0 │
└────────────────────────────────────────┴──────────┴──┴──┴──┴──┴────┘
```

**Reading CR3**:
```rust
use x86_64::registers::control::Cr3;
let (pml4_frame, flags) = Cr3::read();
let pml4_phys_addr = pml4_frame.start_address().as_u64();
```

**Writing CR3** (switches page table):
```rust
Cr3::write(new_pml4_frame, Cr3Flags::empty());
```

### Address Translation Process

Given virtual address `0xFFFF_8000_1234_5678`:

```
Step 1: Extract Indices
  PML4 index = bits[47:39] = 0x100 (256)
  PDP  index = bits[38:30] = 0x000 (0)
  PD   index = bits[29:21] = 0x012 (18)
  PT   index = bits[20:12] = 0x034 (52)
  Offset     = bits[11:0]  = 0x678 (1656)

Step 2: Read PML4 Entry
  PML4_base = CR3 & ~0xFFF
  PML4_entry_addr = PML4_base + (256 * 8)
  PML4_entry = read_u64(PML4_entry_addr)
  PDP_base = PML4_entry & ~0xFFF

Step 3: Read PDP Entry
  PDP_entry_addr = PDP_base + (0 * 8)
  PDP_entry = read_u64(PDP_entry_addr)
  PD_base = PDP_entry & ~0xFFF

Step 4: Read PD Entry
  PD_entry_addr = PD_base + (18 * 8)
  PD_entry = read_u64(PD_entry_addr)
  PT_base = PD_entry & ~0xFFF

Step 5: Read PT Entry
  PT_entry_addr = PT_base + (52 * 8)
  PT_entry = read_u64(PT_entry_addr)
  Page_frame = PT_entry & ~0xFFF

Step 6: Calculate Physical Address
  Physical_addr = Page_frame + Offset
                = Page_frame + 0x678
```

**Hardware TLB**: CPU caches translations to avoid walking page tables on every access.

### TLB (Translation Lookaside Buffer)

**Purpose**: Hardware cache of recent virtual→physical translations.

**Flushing**:
```rust
// Flush single page
use x86_64::instructions::tlb;
tlb::flush(VirtAddr::new(0xFFFF_8000_1234_5000));

// Flush all (by reloading CR3)
use x86_64::registers::control::Cr3;
let (frame, flags) = Cr3::read();
Cr3::write(frame, flags);  // Reload same CR3

// Invlpg instruction (for single page)
unsafe {
    core::arch::asm!("invlpg [{0}]", in(reg) 0xFFFF_8000_1234_5000_u64);
}
```

**Global Pages**: Not flushed by CR3 reload (G flag in PTE).

---

## Memory Regions

### Kernel Code and Data

**Sections**:

```
.text       Code (executable, read-only)
.rodata     Read-only data (constants, strings)
.data       Initialized writable data (global variables)
.bss        Uninitialized data (zero-initialized globals)
```

**Typical Permissions**:
- `.text`: Present, Execute, No-Write
- `.rodata`: Present, No-Execute, No-Write
- `.data`: Present, No-Execute, Writable
- `.bss`: Present, No-Execute, Writable

**Linker Script** (`kernel/linker.ld`):
```ld
SECTIONS {
    . = 0xFFFFFFFF80000000;  /* Typical kernel load address */
    
    .text : {
        *(.text .text.*)
    }
    
    .rodata : {
        *(.rodata .rodata.*)
    }
    
    .data : {
        *(.data .data.*)
    }
    
    .bss : {
        *(.bss .bss.*)
    }
}
```

### Heap Region

**Virtual Range**: `0x4444_4444_0000` - `0x4444_4454_0000` (1 MB)

**Physical Backing**: Allocated on demand from frame allocator during `init_heap()`.

**Page Table Flags**: Present + Writable

**Allocator**: Linked list allocator with first-fit strategy.

**Overhead**: ~16 bytes per free block (linked list node).

**Fragmentation**: Can occur with mixed allocation/deallocation patterns.

### Stack Regions

**Current**: Placeholder addresses, not properly allocated.

**Per-Task Stacks**:
- Size: 8 KB default (configurable)
- Growth: Downward (high address → low address)
- Overflow Detection: None (future: guard pages)

**Future Implementation**:
```
Stack Layout (per task):
┌─────────────────┐ ← High address (stack base)
│                 │
│   Stack Data    │  ↓ Grows downward
│                 │
├─────────────────┤ ← Current RSP
│   Unused        │
├─────────────────┤ ← Stack limit
│  Guard Page     │  (Unmapped, causes page fault on overflow)
└─────────────────┘ ← Low address
```

### Framebuffer Region

**Type**: Device memory (not RAM).

**Location**: Provided by bootloader (varies by GPU).

**Typical Range**: 0xE000_0000 - 0xE080_0000 (8 MB for 1920×1080×32bpp).

**Mapping**: Linear framebuffer, mapped by bootloader.

**Caching**: Write-combining (WC) or uncached (UC) for performance.

### MMIO Regions

**Local APIC**: `0xFEE0_0000` - `0xFEE0_0FFF` (4 KB)

**I/O APIC**: `0xFEC0_0000` - `0xFEC0_00FF` (256 bytes)

**PCI Configuration Space**: Varies by chipset.

**Device BARs**: Allocated by BIOS/UEFI, read from PCI config space.

**Mapping Strategy**: 
- Map on-demand when device driver loads
- Use uncached (UC) page attribute
- Size from BAR register

---

## Address Translation

### Physical to Virtual (Kernel)

```rust
pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    const PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000;
    VirtAddr::new(PHYS_OFFSET + phys.as_u64())
}
```

**Usage**:
```rust
let phys_addr = PhysAddr::new(0x1000);
let virt_addr = phys_to_virt(phys_addr);  // 0xFFFF_8000_0000_1000
```

### Virtual to Physical (Kernel)

```rust
pub fn virt_to_phys(virt: VirtAddr) -> Option<PhysAddr> {
    const PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000;
    
    if virt.as_u64() >= PHYS_OFFSET {
        Some(PhysAddr::new(virt.as_u64() - PHYS_OFFSET))
    } else {
        None  // Not in physical mapping region
    }
}
```

**Usage**:
```rust
let virt_addr = VirtAddr::new(0xFFFF_8000_0000_1000);
if let Some(phys_addr) = virt_to_phys(virt_addr) {
    // phys_addr = 0x1000
}
```

### Using Page Tables (General Translation)

```rust
use x86_64::structures::paging::{Mapper, OffsetPageTable};

pub fn translate_addr(mapper: &OffsetPageTable, addr: VirtAddr) -> Option<PhysAddr> {
    mapper.translate_addr(addr)
}
```

**Implementation**: Walks page tables starting from CR3.

---

## Memory Initialization

### Boot Sequence

```
1. Bootloader (Limine)
   ├─ Sets up identity mapping for kernel
   ├─ Maps framebuffer
   ├─ Provides memory map
   └─ Jumps to kernel _start

2. Kernel _start
   ├─ HAL init (serial)
   ├─ Read memory map from bootloader
   ├─ Initialize offset page table
   │  └─ Get CR3 (bootloader's page table)
   ├─ Preallocate frames to static array
   │  └─ Scan memory map for USABLE regions
   ├─ Create StaticBootFrameAllocator
   │  └─ Wraps preallocated frame array
   ├─ Initialize heap
   │  ├─ Allocate frames for heap pages
   │  ├─ Map virtual heap region to physical frames
   │  └─ Initialize linked list allocator
   └─ Continue kernel init...
```

### Detailed Initialization Steps

#### Step 1: Read Memory Map

```rust
let mmap_response = MMAP_REQ.get_response().expect("No memory map");
let entries = mmap_response.entries();

for entry in entries.iter() {
    serial_println!("Memory region:");
    serial_println!("  Base:   {:#x}", entry.base);
    serial_println!("  Length: {:#x}", entry.length);
    serial_println!("  Type:   {:?}", entry.entry_type);
}
```

#### Step 2: Initialize Page Table

```rust
let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };
```

**Effect**: Creates `OffsetPageTable` wrapping current page table (from CR3).

#### Step 3: Preallocate Frames

```rust
const MAX_BOOT_FRAMES: usize = 65536;
static mut BOOT_FRAMES: [Option<PhysFrame>; MAX_BOOT_FRAMES] = [None; MAX_BOOT_FRAMES];

let mut frame_count = 0;
for region in entries.iter().filter(|r| r.entry_type == EntryType::USABLE) {
    let start_frame = PhysFrame::containing_address(PhysAddr::new(region.base));
    let end_frame = PhysFrame::containing_address(PhysAddr::new(region.base + region.length - 1));
    
    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        if frame_count < MAX_BOOT_FRAMES {
            unsafe {
                BOOT_FRAMES[frame_count] = Some(frame);
            }
            frame_count += 1;
        }
    }
}
```

**Capacity**: 65536 frames × 4 KB = 256 MB of trackable RAM.

#### Step 4: Create Frame Allocator

```rust
let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);
```

#### Step 5: Initialize Heap

```rust
const HEAP_START: usize = 0x4444_4444_0000;
const HEAP_SIZE: usize = 1024 * 1024;  // 1 MB

init_heap(&mut mapper, &mut frame_alloc);
```

**Process**:
1. Calculate page range for heap (256 pages for 1 MB)
2. For each page:
   - Allocate physical frame
   - Map virtual page to physical frame
   - Set flags: Present + Writable
   - Flush TLB
3. Initialize linked list allocator with heap region

---

## Frame Allocation

### Frame Allocator Trait

```rust
pub trait FrameAllocator<S: PageSize> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>>;
}
```

### StaticBootFrameAllocator

**Purpose**: Pre-heap frame allocator using static array.

**Implementation**:
```rust
pub struct StaticBootFrameAllocator {
    next: usize,
    limit: usize,
}

unsafe impl FrameAllocator<Size4KiB> for StaticBootFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while self.next < self.limit {
            unsafe {
                if let Some(frame) = BOOT_FRAMES[self.next].take() {
                    self.next += 1;
                    return Some(frame);
                }
            }
            self.next += 1;
        }
        None
    }
}
```

**Algorithm**: Bump allocator (no deallocation).

### BootFrameAllocator

**Purpose**: Post-heap frame allocator using Vec.

**Implementation**:
```rust
pub struct BootFrameAllocator {
    frames: &'static [PhysFrame],
    next: usize,
}

impl BootFrameAllocator {
    pub fn new(memory_map: &[&Entry]) -> Self {
        let mut frames = Vec::new();
        
        for region in memory_map.iter().filter(|r| r.entry_type == EntryType::USABLE) {
            // ... add frames to vector
        }
        
        let static_frames = Box::leak(frames.into_boxed_slice());
        BootFrameAllocator { frames: static_frames, next: 0 }
    }
}
```

**Note**: Currently unused (StaticBootFrameAllocator sufficient for current needs).

---

## Heap Management

### Heap Allocator: linked_list_allocator

**Strategy**: First-fit with coalescing.

**Structure**:
```
Free Block Node:
┌────────────────┬────────────────┐
│ Size           │ Next Pointer   │
├────────────────┴────────────────┤
│                                 │
│   Free Space                    │
│                                 │
└─────────────────────────────────┘
```

**Allocation Algorithm**:
1. Walk free list
2. Find first block with `size >= requested_size + overhead`
3. Split block if much larger than needed
4. Remove block from free list
5. Return pointer to usable space

**Deallocation Algorithm**:
1. Add block back to free list
2. Merge with adjacent free blocks (coalescing)

**Coalescing**:
```
Before:
[Free A] [Used] [Free B]

After deallocation:
[Free A+Used+B (merged)]
```

### Heap Statistics (Future)

```rust
pub struct HeapStats {
    pub total_size: usize,
    pub used: usize,
    pub free: usize,
    pub num_allocations: usize,
    pub num_deallocations: usize,
    pub largest_free_block: usize,
}

pub fn get_heap_stats() -> HeapStats {
    HEAP_ALLOCATOR.lock().stats()
}
```

---

## Appendix

### Configuration Constants

```rust
// memory/src/heap.rs
pub const HEAP_START: usize = 0x4444_4444_0000;
pub const HEAP_SIZE: usize = 1024 * 1024;  // 1 MB
pub const MAX_BOOT_FRAMES: usize = 65536;

// kernel/src/main.rs
const PHYS_MEM_OFFSET: u64 = 0xFFFF_8000_0000_0000;
```

### Page Size Definitions

```rust
pub trait PageSize {
    const SIZE: u64;
}

pub struct Size4KiB;
impl PageSize for Size4KiB {
    const SIZE: u64 = 4096;  // 4 KB
}

pub struct Size2MiB;
impl PageSize for Size2MiB {
    const SIZE: u64 = 2 * 1024 * 1024;  // 2 MB
}

pub struct Size1GiB;
impl PageSize for Size1GiB {
    const SIZE: u64 = 1024 * 1024 * 1024;  // 1 GB
}
```

### Memory Type Definitions

```rust
// From Limine
pub enum EntryType {
    USABLE,
    RESERVED,
    ACPI_RECLAIMABLE,
    ACPI_NVS,
    BAD_MEMORY,
    BOOTLOADER_RECLAIMABLE,
    KERNEL_AND_MODULES,
    FRAMEBUFFER,
}
```

---

**End of Document**
