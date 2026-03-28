===================================

# Serix Kernel Memory Layout

:Page Size: 4 KiB

.. contents

```
:depth: 3

```

## Overview

This document specifies the complete virtual and physical memory layout of the
Serix kernel. Serix uses a higher-half kernel design with offset page tables and
a direct physical memory mapping (HHDM - Higher Half Direct Map).

## Memory Layout Philosophy

Serix follows these memory design principles:

1. **Higher-Half Kernel**: Kernel code and data reside in upper half of virtual
   address space (above 0xFFFF_8000_0000_0000)

2. **Direct Physical Mapping**: All physical RAM is mapped at a fixed virtual
   offset (HHDM) for efficient access

3. **Separate Address Spaces**: Each userspace process has its own address space;
   kernel space is shared across all processes

4. **No-Execute Protection**: Code pages are marked executable, data pages are not

5. **Write Protection**: Read-only data is enforced via page table flags

## Virtual Address Space Layout

## x86_64 Canonical Addresses

x86_64 uses 48-bit virtual addresses with canonical address form:

- **Lower canonical addresses**: ``0x0000_0000_0000_0000`` to ``0x0000_7FFF_FFFF_FFFF``
  (userspace)

- **Non-canonical gap**: ``0x0000_8000_0000_0000`` to ``0xFFFF_7FFF_FFFF_FFFF``
  (causes #GP fault if accessed)

- **Higher canonical addresses**: ``0xFFFF_8000_0000_0000`` to ``0xFFFF_FFFF_FFFF_FFFF``
  (kernel space)

## Complete Virtual Memory Map

```

Virtual Address Range                    Size        Purpose
================================================================================

USERSPACE (Lower Half)
--------------------------------------------------------------------------------
0x0000_0000_0000_0000 - 0x0000_0000_0FFF_FFFF   16 MB      Null guard (unmapped)
0x0000_0000_1000_0000 - 0x0000_0000_3FFF_FFFF  752 MB      User code (.text)
0x0000_0000_4000_0000 - 0x0000_0000_7FFF_FFFF    1 GB      User data (.data, .bss)
0x0000_0000_8000_0000 - 0x0000_7FFF_BFFF_FFFF  ~128 TB     User heap (grows up)
0x0000_7FFF_C000_0000 - 0x0000_7FFF_FFFF_FFFF    1 GB      User stack (grows down)

NON-CANONICAL GAP
--------------------------------------------------------------------------------
0x0000_8000_0000_0000 - 0xFFFF_7FFF_FFFF_FFFF  ~128 TB     Invalid (causes #GP)

KERNEL SPACE (Higher Half)
--------------------------------------------------------------------------------
0xFFFF_8000_0000_0000 - 0xFFFF_8000_FFFF_FFFF    4 GB      HHDM (Physical mem)
0xFFFF_8001_0000_0000 - 0xFFFF_8001_FFFF_FFFF    4 GB      Reserved
0xFFFF_8002_0000_0000 - 0xFFFF_8003_FFFF_FFFF    8 GB      Reserved for MMIO
0xFFFF_8000_4444_0000 - 0xFFFF_8000_4454_0000    1 MB      Kernel heap
0xFFFF_8000_4454_0000 - 0xFFFF_BFFF_FFFF_FFFF  ~64 TB      Reserved
0xFFFF_C000_0000_0000 - 0xFFFF_CFFF_FFFF_FFFF   16 TB      Page tables
0xFFFF_D000_0000_0000 - 0xFFFF_DFFF_FFFF_FFFF   16 TB      Reserved
0xFFFF_E000_0000_0000 - 0xFFFF_EFFF_FFFF_FFFF   16 TB      VFS caches
0xFFFF_F000_0000_0000 - 0xFFFF_FEFF_FFFF_FFFF   15 TB      Reserved
0xFFFF_FF00_0000_0000 - 0xFFFF_FF7F_FFFF_FFFF  512 GB      Per-CPU data
0xFFFF_FF80_0000_0000 - 0xFFFF_FFFF_7FFF_FFFF  512 GB      Kernel modules
0xFFFF_FFFF_8000_0000 - 0xFFFF_FFFF_FFFF_FFFF    2 GB      Kernel image

```

## Userspace Memory Layout

Each userspace process has this address space layout

```

0x0000_0000_0000_0000    Null guard page (16 MB, unmapped)

0x0000_0000_1000_0000    Program code (.text segment)

0x0000_0000_4000_0000    Program data (.data, .bss, .rodata)

0x0000_0000_8000_0000    Heap (grows upward ->)

0x0000_7FFF_C000_0000    Stack (grows downward <-)

```

**Stack Layout** (grows downward)

```

0x0000_7FFF_FFFF_FFF8    Top of stack (initial RSP)
...                      Local variables, function calls
0x0000_7FFF_C000_0000    Bottom of stack
0x0000_7FFF_BFFF_F000    Stack guard page (unmapped)

```

## Kernel Memory Layout

Higher Half Direct Map (HHDM)

```

Physical memory is mapped at fixed offset

```

Virtual: 0xFFFF_8000_0000_0000
Size:    Entire physical RAM (typically 4 GB in QEMU)
Flags:   Present, Writable, NX (No-Execute)

Translation:

```
**Purpose**: Direct access to any physical address without TLB misses

**Usage**: Frame allocator, DMA buffers, device MMIO

Kernel Heap
```

Dynamic kernel memory allocation

```

Virtual: 0xFFFF_8000_4444_0000
Size:    1 MB (1,048,576 bytes)
Flags:   Present, Writable, NX

Allocator: linked_list_allocator (buddy allocator planned)

```
**Current Status (v0.0.6)**: 1 MB heap is sufficient for current kernel needs.
Future expansion planned for:

- Larger VFS caches
- More concurrent tasks
- Network buffers

Kernel Image
```

Kernel ELF sections loaded by bootloader

```

Virtual: 0xFFFF_FFFF_8000_0000 (higher half)
Size:    ~4 MB (kernel binary + modules)
Flags:   .text (R-X), .rodata (R--), .data (RW-), .bss (RW-)

```
**Sections**:

.text
    Kernel code (executable, read-only)

.rodata
    Read-only data (strings, constants)

.data
    Initialized writable data (globals)

.bss
    Uninitialized data (zeroed by bootloader)

.limine_reqs
    Limine protocol requests (read-only)



## Physical Memory Layout


## Physical Address Map

Physical memory as seen by the MMU

```

Physical Address Range       Size        Purpose
= = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = = =

0x0000_0000 - 0x0009_FFFF    640 KB      Low memory (legacy)
0x000A_0000 - 0x000F_FFFF    384 KB      VGA/BIOS (reserved)
0x0010_0000 - [varies]       [varies]    Usable RAM (main memory)
0xFEC0_0000 - 0xFEC0_0FFF      4 KB      I/O APIC MMIO
0xFEE0_0000 - 0xFEE0_0FFF      4 KB      Local APIC MMIO
0xFED0_0000 - 0xFED0_3FFF     16 KB      HPET (High Precision Timer)
0xFFFC_0000 - 0xFFFF_FFFF    256 KB      BIOS ROM

```
**Note**: Actual layout varies by system. Memory map is discovered via Limine
memory map request during boot.


## Memory Types

Memory regions are classified by type:

USABLE
    Free RAM available for kernel/userspace use

RESERVED
    Hardware reserved regions (chipset, MMIO)

ACPI_RECLAIMABLE
    ACPI tables (can be reclaimed after parsing)

ACPI_NVS
    ACPI Non-Volatile Storage (must be preserved)

BAD_MEMORY
    Defective RAM (hardware failure detected)

BOOTLOADER_RECLAIMABLE
    Limine bootloader code/data (can be reclaimed after boot)

KERNEL_AND_MODULES
    Kernel ELF image and init module

FRAMEBUFFER
    Graphics framebuffer memory


## Bootloader Memory Regions

Limine provides detailed memory map during boot. Example from 4GB QEMU VM

```

Region 0: 0x0000_0000 - 0x0009_FC00  (640 KB)     USABLE
Region 1: 0x0009_FC00 - 0x000A_0000  (1 KB)       RESERVED
Region 2: 0x000F_0000 - 0x0010_0000  (64 KB)      RESERVED (BIOS)
Region 3: 0x0010_0000 - 0x7FEF_0000  (~2 GB)      USABLE
Region 4: 0x7FEF_0000 - 0x8000_0000  (1.1 MB)     BOOTLOADER_RECLAIMABLE
Region 5: 0x8000_0000 - 0xC000_0000  (1 GB)       RESERVED (PCI hole)
Region 6: 0x0010_0000 - 0x0050_0000  (4 MB)       KERNEL_AND_MODULES
Region 7: 0xFD00_0000 - 0xFE00_0000  (16 MB)      FRAMEBUFFER

```

## Page Table Structure


## x86_64 4-Level Paging

Serix uses 4-level paging (PML4)

```

Virtual Address (48-bit):
┌────────┬────────┬────────┬────────┬────────┬────────────┐
│  Sign  │  PML4  │  PDPT  │   PD   │   PT   │   Offset   │
│ Extend │  Index │  Index │  Index │  Index │            │
├────────┼────────┼────────┼────────┼────────┼────────────┤
│ 63-48  │ 47-39  │ 38-30  │ 29-21  │ 20-12  │   11-0     │
│ 16 bit │  9 bit │  9 bit │  9 bit │  9 bit │   12 bit   │
└────────┴────────┴────────┴────────┴────────┴────────────┘

Translation Process:

1. CR3 register points to PML4 table (base physical address)
2. Bits 47-39 index into PML4 table -> get PDPT address
3. Bits 38-30 index into PDPT table -> get PD address
4. Bits 29-21 index into PD table   -> get PT address
5. Bits 20-12 index into PT table   -> get physical page frame
6. Bits 11-0 are offset within page (4 KB = 12 bits)

```

## Page Table Entry Format

Each page table entry is 64 bits

```

Bit   Name            Description
===== =============== ====================================================
0     Present (P)     Page is present in memory
1     Writable (W)    Page is writable (otherwise read-only)
2     User (U)        Accessible from userspace (otherwise kernel-only)
3     PWT             Page-level write-through
4     PCD             Page-level cache disable
5     Accessed (A)    Page has been accessed
6     Dirty (D)       Page has been written to (PT entries only)
7     PAT/PS          Page Attribute Table / Page Size (1 GB or 2 MB page)
8     Global (G)      TLB entry is global (not flushed on CR3 reload)
9-11  Available       Available for OS use
12-51 Address         Physical address bits [51:12] (40-bit address)
52-62 Available       Available for OS use
63    NX              No-Execute (page is not executable)

```

## Page Table Flags Used by Serix


```

Flag Combination         Virtual Address Range              Purpose
======================== ================================== ===================
P                        Unmapped regions                   Not present
 P | W | NX               Kernel data, heap                  Kernel RW data
 P | NX                   Kernel rodata                      Kernel RO data
 P | W | NX               HHDM region                        Physical memory
 P | W | U | NX           Userspace data, stack, heap        User RW data
 P | U | NX               Userspace rodata                   User RO data
 P | U                    Userspace code (.text)             User executable
P                        Kernel code (.text)                Kernel executable

```
**Legend**:

- P = Present
- W = Writable
- U = User (accessible from ring 3)
- NX = No-Execute


## TLB Management

The Translation Lookaside Buffer (TLB) caches page translations. Serix manages
the TLB as follows:

**Global Pages**: Kernel pages in HHDM and kernel image use Global flag to
prevent TLB flush on context switch

**TLB Flush Operations**:

Single-page flush
    ``invlpg`` instruction (after modifying page table entry)

Full TLB flush
    Reload CR3 register (on context switch to different address space)

Global TLB flush
    Clear and reload CR3 (rare, used for kernel page table updates)

**TLB Shootdown** (SMP): Not yet implemented. Planned for multi-core support.



## Memory Management


## Frame Allocation

Physical memory is managed in 4 KiB frames. Two allocators are used:

Boot Frame Allocator (Pre-Heap)
```

Used during early boot before heap is initialized

```

Storage:    Static array BOOT_FRAMES[65536]
Capacity:   65,536 frames (256 MB)
Algorithm:  Simple bump allocator (no deallocation)

Usage:

```

**Limitation**: No deallocation. Frames cannot be freed.

Heap Frame Allocator (Post-Heap)

```

Used after heap initialization (planned for v0.1.0)

```

Storage:    Heap-allocated bitmap or buddy allocator
Capacity:   All physical RAM
Algorithm:  Bitmap or buddy system

Features:

```

## Heap Allocation

Kernel heap provides dynamic memory allocation:

Current Allocator: linked_list_allocator
```

```

Algorithm:  First-fit with coalescing
Size:       1 MB (256 pages)
Location:   0xFFFF_8000_4444_0000

Usage:

```

**Performance**:

- Allocation: O(n) where n = number of free blocks
- Deallocation: O(1) (constant time)
- Fragmentation: Mitigated by coalescing adjacent free blocks

Planned Allocator: SLAB (v0.1.0)

```


```

Algorithm:  Object caching with per-CPU freelists
Size:       4 MB (1024 pages)
Caches:     TaskCB (512 B), INode (256 B), Buffer (4 KB)

Benefits:

```

## Memory Allocation APIs

Kernel APIs for memory management

```

// Frame allocation
fn allocate_frame() -> Option<PhysFrame<Size4KiB>>
fn deallocate_frame(frame: PhysFrame<Size4KiB>)

// Page mapping
fn map_page(page: Page, frame: PhysFrame, flags: PageTableFlags)
fn unmap_page(page: Page) -> Option<PhysFrame>
fn translate_page(page: Page) -> Option<PhysFrame>

// Heap allocation (via Rust alloc crate)
Vec::new()
Box::new(value)
String::from(str)

// Address translation
fn phys_to_virt(phys: PhysAddr) -> VirtAddr
fn virt_to_phys(virt: VirtAddr) -> Option<PhysAddr>

```

## Stack Allocation

Kernel Stack
```

Each kernel task has its own kernel stack

```

Size:       16 KiB (4 pages)
Location:   Allocated from heap
Guard:      Unmapped guard page below stack
Flags:      Present, Writable, NX

Layout (grows downward):

```
**Stack Overflow Detection**: Accessing guard page triggers page fault, kernel
detects stack overflow and terminates task.

Userspace Stack
```

Each userspace thread has its own stack

```

Size:       8 MiB (2048 pages)
Location:   0x0000_7FFF_C000_0000 - 0x0000_7FFF_FFFF_FFFF
Guard:      Unmapped guard page below stack
Flags:      Present, Writable, User, NX

Growth:     Stack grows downward, page faults trigger expansion

```

## Memory Protection


## No-Execute (NX) Protection

All data pages are marked non-executable (NX bit set). This prevents code
execution from data regions:

- Kernel heap: NX
- Kernel stack: NX
- Userspace heap: NX
- Userspace stack: NX
- Userspace data: NX

Only code sections (.text) are executable.


## Write Protection

Read-only sections are enforced

```

.rodata     Kernel read-only data        P, NX       (not W)
.text       Kernel code                  P           (not W, not NX)
User .text  Userspace code               P, U        (not W)

```
**Violation**: Writing to read-only page triggers page fault (#PF with W=1 in
error code).


## User/Supervisor Protection

Kernel pages are marked supervisor-only (User bit clear). Userspace cannot
access kernel memory:

- HHDM: Supervisor-only
- Kernel heap: Supervisor-only
- Kernel stack: Supervisor-only
- Kernel image: Supervisor-only

**Violation**: Userspace access to kernel page triggers page fault (#PF with
U=1 in error code).


## SMAP/SMEP (Future)

Planned for v0.1.0:

SMAP (Supervisor Mode Access Prevention)
    Prevents kernel from accidentally accessing userspace memory (must use
    explicit copy functions)

SMEP (Supervisor Mode Execution Prevention)
    Prevents kernel from executing userspace code

**Benefit**: Mitigates privilege escalation exploits.



## Memory Statistics (v0.0.6)


## Kernel Memory Usage

Current kernel memory footprint

```

Component           Size        Location
=================== =========== ======================================
Kernel image        ~2 MB       0xFFFF_FFFF_8000_0000
Kernel heap         1 MB        0xFFFF_8000_4444_0000
Boot frame array    256 KB      .bss section (65536 * 4 bytes)
IDT                 4 KB        .data section
GDT                 64 bytes    .data section
Page tables         ~16 KB      Managed by bootloader
Total               ~3.3 MB

At runtime (with init):
Heap usage          ~10 KB      (VFS caches, task structs)
Stack usage         16 KB       (kernel main stack)

```

## Physical Memory Usage (4 GB QEMU)

Typical memory allocation on 4 GB VM

```

Region              Size        Percentage
=================== =========== ===========
Usable RAM          ~3.8 GB     95%
Reserved            ~128 MB     3%
Bootloader          ~1 MB       <1%
Kernel + modules    ~4 MB       <1%
Framebuffer         ~16 MB      <1%
MMIO regions        ~64 MB      1.5%

```

## Future Improvements


## Planned for v0.1.0

**Memory Management**:

- SLAB allocator for kernel objects
- Proper frame deallocation
- Memory pressure handling
- OOM killer

**Virtual Memory**:

- Demand paging for userspace
- Copy-on-write (COW) for fork()
- mmap() support
- Shared memory regions

**Security**:

- SMAP/SMEP support
- KPTI (Kernel Page Table Isolation) for Meltdown mitigation
- Address space layout randomization (ASLR)


## Planned for v0.2.0

**Huge Pages**:

- 2 MiB pages for large allocations
- 1 GiB pages for HHDM (reduce TLB pressure)

**NUMA**:

- NUMA-aware frame allocation
- Per-node memory statistics
- Page migration

**Memory Compression**:

- Swap compression (zswap-like)
- Transparent huge pages (THP)



## Debugging Memory Issues


## Page Fault Debugging

When a page fault occurs, the CPU provides

```

CR2 register:   Faulting virtual address
Error code:     Reason for fault

Error Code Bits:

```
Example page fault handler

```

extern "x86-interrupt" fn page_fault_handler(
) {

}

```

## Memory Leak Detection

Heap usage tracking (planned)

```

fn heap_stats() -> HeapStats {
}

// Periodic logging
serial_println!("Heap: {} / {} bytes ({} allocations)",

```

## Virtual Memory Dump

Dump page table entries for debugging

```

fn dump_page_tables(addr: VirtAddr) {

}

```

## Appendix


## Memory Constants

All memory-related constants used in Serix

```rust

// Virtual addresses
const KERNEL_HEAP_START:  u64 = 0xFFFF_8000_4444_0000;
const KERNEL_HEAP_SIZE:   u64 = 1024 * 1024;  // 1 MB
const HHDM_OFFSET:        u64 = 0xFFFF_8000_0000_0000;
const KERNEL_BASE:        u64 = 0xFFFF_FFFF_8000_0000;
const USER_STACK_TOP:     u64 = 0x0000_7FFF_FFFF_FFFF;
const USER_STACK_SIZE:    u64 = 8* 1024 * 1024;  // 8 MB

// Page sizes
const PAGE_SIZE:          u64 = 4096;         // 4 KiB
const HUGE_PAGE_SIZE:     u64 = 2 *1024* 1024;  // 2 MiB (future)
const GIANT_PAGE_SIZE:    u64 = 1024 *1024* 1024;  // 1 GiB (future)

// Frame allocation
const MAX_BOOT_FRAMES:    usize = 65536;      // 256 MB
const KERNEL_STACK_SIZE:  usize = 16 * 1024;  // 16 KiB

```

## Useful GDB Commands

Debugging memory with GDB

```

## Examine page table registers

(gdb) info registers cr3 cr4

## Dump memory

(gdb) x/16gx 0xFFFF_8000_0000_0000  # HHDM start
(gdb) x/16gx 0xFFFF_8000_4444_0000  # Kernel heap

## Examine page table entry

(gdb) p/x *(uint64_t*)0x... # PML4 entry address

## Set watchpoint on memory

(gdb) watch *(uint64_t*)0xFFFF_8000_4444_0000

## View virtual to physical translation

(gdb) monitor gva2gpa 0xFFFF_8000_4444_0000

```

## Memory Layout Diagram (ASCII)

Complete memory map

```

User Space (0x0000...)              Kernel Space (0xFFFF...)

0x0000_7FFF_FFFF_FFFF  ┐            0xFFFF_FFFF_FFFF_FFFF  ┐
0x0000_7FFF_C000_0000  ┘            0xFFFF_FFFF_8000_0000  ┘
0x0000_0000_8000_0000  │            0xFFFF_FF80_0000_0000  │
0x0000_0000_4000_0000  │            0xFFFF_FF00_0000_0000  │
0x0000_0000_1000_0000  │            0xFFFF_F000_0000_0000  │
0x0000_0000_0000_0000  ┘            0xFFFF_E000_0000_0000  │

```

## See Also

- **[Boot Process](BOOT_PROCESS.md)** - How memory is initialized during boot
- **[Architecture Overview](ARCHITECTURE.md)** - System design and memory philosophy
- **[Memory Module](../memory/README.md)** - Page table and heap implementation
- **[Kernel API](KERNEL_API.md)** - Memory-related syscalls
