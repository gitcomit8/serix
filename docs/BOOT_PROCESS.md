===================================

# Serix Kernel Boot Process

.. contents

```
:depth: 3

```

## Overview

Serix uses a multi-stage boot process that transitions from firmware (BIOS/UEFI)
through the Limine bootloader to the kernel proper. This document specifies each
stage, the handoff between stages, and the complete kernel initialization sequence.

## Boot Flow Summary

The boot process follows this sequence

```

Firmware (BIOS/UEFI)
Limine Stage 1
Limine Stage 2
Kernel Entry (_start)
Early Initialization
Memory Initialization
Device Initialization
Userspace Initialization
Kernel Idle Loop

```

## Time Estimates

============== =================== ============================================
Stage          Duration            Notes
============== =================== ============================================
Firmware POST  0.5-3 seconds       Varies by hardware
Limine Stage 1 < 100 ms            Minimal work
Limine Stage 2 100-500 ms          Loads kernel, sets up environment
Kernel Init    10-50 ms            Serial, APIC, IDT, memory, heap, graphics
Total Boot     1-4 seconds         Typical on modern hardware in QEMU
============== =================== ============================================

## Boot Stages

## Stage 0: Firmware (BIOS/UEFI)

BIOS Boot

```

The BIOS boot process follows these steps:

1. Power on - CPU starts at reset vector (0xFFFFFFF0)
2. Jump to BIOS code in ROM
3. POST (Power-On Self-Test)

   - Memory test
   - Hardware detection
   - Initialize chipset

4. Enumerate boot devices (HDD, SSD, USB, CD-ROM, Network)
5. Read MBR (Master Boot Record) from first sector of boot device
6. Load bootloader code from MBR to 0x7C00
7. Jump to 0x7C00 (bootloader entry)

Environment at handoff

```

CPU Mode:   Real mode (16-bit)
Memory:     Low memory available (<1 MB)
A20 Line:   May be disabled (limits addressing to 1 MB)

```
UEFI Boot
```

The UEFI boot process:

1. Power on - CPU starts at reset vector
2. SEC (Security) Phase: Minimal setup
3. PEI (Pre-EFI Initialization): Memory and platform init
4. DXE (Driver Execution Environment): Load drivers
5. BDS (Boot Device Selection): Enumerate boot options
6. Read EFI System Partition (ESP, FAT32)
7. Load EFI bootloader application (.efi file)
8. Execute bootloader in UEFI context

Environment at handoff

```

CPU Mode:   Long mode (64-bit) or protected mode (32-bit)
Memory:     Full memory available
Services:   UEFI runtime services available

```


## Stage 1: Limine Stage 1

**Location**: MBR/VBR (Volume Boot Record)

**Size**: 446 bytes (MBR) or 512 bytes (VBR)

**Purpose**: Load Stage 2 from disk

Operation

```

1. Read Stage 2 sectors from known disk location
2. Load Stage 2 to memory
3. Jump to Stage 2 entry point

```

**Limitations**:

- Minimal code space
- Real mode only (BIOS) or limited environment (UEFI)
- No filesystem support


## Stage 2: Limine Stage 2

**Location**: Dedicated partition or filesystem

**Size**: ~200 KB

**Purpose**: Full bootloader functionality

Capabilities:

- Filesystem support (ext2, FAT32, etc.)
- ELF loading
- Memory map creation
- Framebuffer initialization
- Configuration parsing

Operation

```

1. Parse configuration file (limine.conf)
2. Setup memory map
3. Setup framebuffer (if available)
4. Load kernel ELF from filesystem
5. Setup initial page tables
6. Enter long mode (if not already)
7. Create Limine boot info structures
8. Jump to kernel entry point

```


## Limine Bootloader


## Limine Protocol

**Version**: 10.x

**Documentation**: https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md

**Request/Response Model**:

- Kernel defines "requests" in special ELF sections
- Bootloader populates "responses" before kernel entry
- Allows kernel to request specific features/information


## Configuration File

**Path**: ``/limine.conf`` (root of boot partition)

Serix configuration

```

TIMEOUT=3

:Serix OS v0.0.5

```

**Directives**:

TIMEOUT
    Seconds to wait before auto-boot

:Entry Name
    Boot menu entry

PROTOCOL=limine
    Use Limine protocol

KERNEL_PATH
    Path to kernel ELF (boot:/// = boot partition root)

MODULE_PATH
    Path to init binary module


## Boot Protocol Requests

Serix defines several requests in kernel code.

Base Revision Request

```


```

static BASE_REVISION: BaseRevision = BaseRevision::new();

```
**Purpose**: Declares bootloader protocol version

**Section**: ``.limine_reqs``

**Required**: Yes (protocol version check)

Framebuffer Request
```


```

static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();

```
**Purpose**: Requests graphics framebuffer

**Response Includes**:

- Framebuffer physical address
- Width, height (pixels)
- Pitch (bytes per line)
- Bits per pixel (BPP)
- Pixel format (BGRA, RGB, etc.)

**Multiple Framebuffers**: Response may include multiple displays

Memory Map Request
```


```

static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

```
**Purpose**: Requests system memory map

**Response Includes**:

- Array of memory regions
- Each region: base address, length, type

**Memory Types**:

USABLE
    Free RAM

RESERVED
    Hardware reserved

ACPI_RECLAIMABLE
    ACPI tables (can be reclaimed after parsing)

ACPI_NVS
    ACPI Non-Volatile Storage

BAD_MEMORY
    Defective RAM

BOOTLOADER_RECLAIMABLE
    Bootloader code/data (can be reclaimed)

KERNEL_AND_MODULES
    Kernel image and loaded modules

FRAMEBUFFER
    Framebuffer memory

HHDM Request
```


```

static HHDM_REQ: HhdmRequest = HhdmRequest::new();

```
**Purpose**: Requests Higher Half Direct Mapping offset

**Response**: Virtual address offset for physical memory mapping
(typically 0xFFFF_8000_0000_0000)

Module Request
```


```

static MODULE_REQ: ModuleRequest = ModuleRequest::new();

```
**Purpose**: Requests loaded modules (init binary)

**Response**: Array of loaded modules with addresses and sizes


## Limine Handoff State

CPU state at kernel entry

```

RIP:        Kernel entry point (_start)
CS:         Kernel code segment (typically 0x08)
DS/ES/SS:   Kernel data segment (typically 0x10)
RSP:        Valid stack pointer
RFLAGS:     IF=0 (interrupts disabled), DF=0
CR0:        PE=1 (protected mode), PG=1 (paging enabled)
CR3:        Page table base (identity + higher half mapped)
CR4:        PAE=1, PGE=1, OSFXSR=1, OSXMMEXCPT=1

```
Memory layout

```

Identity mapping:   0x0 - [physical RAM size]
Higher half:        0xFFFF_FFFF_8000_0000 - 0xFFFF_FFFF_FFFF_FFFF
Physical mapping:   0xFFFF_8000_0000_0000 - 0xFFFF_8000_FFFF_FFFF

```
GDT (Global Descriptor Table)

```

Entry 0: Null descriptor
Entry 1: Kernel code segment (0x08)
Entry 2: Kernel data segment (0x10)

```
**Interrupts**: Disabled (IF=0), IDT not loaded



## Kernel Entry Point


## _start Function

**Location**: ``kernel/src/main.rs``

**Signature**

```

[unsafe(no_mangle)]
pub extern "C" fn_start() -> !

```

**Attributes**:

- ``#[unsafe(no_mangle)]``: Preserve function name for linker
- ``extern "C"``: C calling convention
- ``-> !``: Never returns (diverging function)

**Entry Conditions**:

- Interrupts disabled
- Paging enabled (bootloader's page tables)
- Stack allocated by bootloader
- Limine responses populated


## Initial Code

The kernel entry point structure

```

[unsafe(no_mangle)]
pub extern "C" fn_start() -> ! {

}

```

## Initialization Sequence


## Phase 1: Serial Console (0-1 ms)

**Purpose**: Establish debug output channel

**Operations**:

1. Initialize COM1 serial port (0x3F8)
2. Configure: 115200 baud, 8N1
3. Enable FIFO
4. Test with initial message

**Code Path**: ``hal::init_serial()`` in ``hal/src/serial.rs``

**Critical**: Must be first step for debug output

Serial output

```

Serix Kernel v0.0.5 Starting...
Serial console initialized

```

## Phase 2: APIC Setup (1-2 ms)

**Purpose**: Setup interrupt controller

Disable Legacy PIC
```


```

unsafe { apic::enable(); }  // Also calls disable_pic()

```
**Steps**:

1. Initialize PIC (ICW1-ICW4)
2. Remap IRQs to vectors 32-47
3. Mask all interrupts (0xFF to both data ports)

**Why**: Prevent conflicts between PIC and APIC

Enable Local APIC
```


```

unsafe { apic::enable(); }

```
**Steps**:

1. Read IA32_APIC_BASE MSR (0x1B)
2. Set bit 11 (APIC Global Enable)
3. Write back MSR
4. Write 0x1FF to SVR register (offset 0xF0) - sets vector 0xFF and enable bit

**Verification**: Read SVR, check bit 8 is set

Initialize I/O APIC
```


```

unsafe { apic::ioapic::init_ioapic(); }

```
**Steps**:

1. Map IRQ0 (timer) to vector 32
2. Map IRQ1 (keyboard) to vector 33

**Registers**: I/O APIC redirection table entries

Register Timer Handler
```

```

unsafe { apic::timer::register_handler(); }

```

**Purpose**: Register handler before IDT is loaded

**Handler**: ``timer_interrupt`` (vector 49)

Serial output

```

Legacy PIC disabled
APIC enabled

```


## Phase 3: IDT Setup (2-3 ms)

**Purpose**: Setup interrupt and exception handlers

Load IDT

```


```

idt::init_idt();

```
**Steps**:

1. Initialize IDT structure (lazy_static)
2. Set exception handlers (divide by zero, page fault, double fault)
3. Set hardware interrupt handlers (keyboard, timer)
4. Load IDT into IDTR register (LIDT instruction)

**Vectors Configured**:

====== ========================
Vector Handler
====== ========================
0      Divide by zero
8      Double fault
14     Page fault
33     Keyboard (PS/2)
49     Timer (LAPIC)
====== ========================

Enable Interrupts
```

```

x86_64::instructions::interrupts::enable();

```

**Operation**: Execute STI instruction (set IF flag)

**Effect**: CPU now responds to hardware interrupts


## Phase 4: Timer Hardware (3-4 ms)

**Purpose**: Start LAPIC timer for timekeeping

**Operations**

```

unsafe { apic::timer::init_hardware(); }

```

**Steps**:

1. Write divide config register (0x3E0) = 0x3 (divide by 16)
 2. Write LVT timer register (0x320) = vector 49 | 0x20000 (periodic) 
3. Write initial count register (0x380) = 100,000

**Result**: Timer fires every ~1.6 ms (625 Hz)

Serial output

```

Keyboard ready for input!

```


## Phase 5: Framebuffer Access (4-5 ms)

**Purpose**: Get graphics output capability

**Operations**

```

let fb_response = FRAMEBUFFER_REQ.get_response()

```

**Response Fields**:

- Framebuffer count (typically 1)
- For each framebuffer:

  - Physical address
  - Width, height (pixels)
  - Pitch (bytes per line)
  - BPP (bits per pixel, typically 32)
  - Memory model (typically RGB)

**Usage**: Passed to graphics subsystem for rendering


## Phase 6: Memory Map Processing (5-10 ms)

**Purpose**: Discover available RAM

**Operations**

```

let mmap_response = MMAP_REQ.get_response()
let entries = mmap_response.entries();

```

Processing loop

```

for entry in entries.iter() {
}

```

Example serial output

```

Memory region:
Memory region:
Memory region:

```


## Phase 7: Page Table Initialization (10-15 ms)

**Purpose**: Setup virtual memory management

Initialize Offset Page Table

```


```

let phys_mem_offset = hhdm_response.offset();
let mut mapper = unsafe {
};

```
**Steps**:

1. Read CR3 (get current PML4 physical address)
2. Convert to virtual address using HHDM offset
3. Create OffsetPageTable wrapper

**Result**: ``mapper`` can be used to manipulate page tables

Preallocate Frames
```

Iterate through usable memory regions and store frames

```

let mut frame_count = 0;
for region in entries.iter()
{

}

```
**Purpose**: Store available frames in static array (pre-heap allocation)

**Capacity**: 65,536 frames (256 MB maximum)

Create Frame Allocator
```


```

let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);

```
**Purpose**: Wrap frame array in FrameAllocator trait


## Phase 8: Heap Initialization (15-25 ms)

**Purpose**: Enable dynamic memory allocation

**Operations**

```

init_heap(&mut mapper, &mut frame_alloc);

```
**Steps**:

1. Calculate heap page range (256 pages for 1 MB)
2. For each page:

   - Allocate physical frame
   - Map virtual page (0xFFFF_8000_4444_0000 + offset) to frame
   - Set flags: Present + Writable
   - Flush TLB

3. Initialize linked list allocator

**Result**: Rust ``alloc`` crate now functional (Vec, Box, String, etc.)

**Duration**: ~10 ms (depends on page count and TLB flush overhead)

**Heap Configuration**

```

HEAP_START: 0xFFFF_8000_4444_0000
HEAP_SIZE:  1 MB (1,048,576 bytes)

```

## Phase 9: Graphics Initialization (25-35 ms)

**Purpose**: Setup visual output

Paint Screen Blue
```


```

if let Some(fb) = fb_response.framebuffers().next() {
}

```
**Steps**:

1. Get first framebuffer
2. Fill entire screen with blue pixels (0xFF0000FF in BGRA format)
3. Draw memory map visualization (colored bars at bottom)

**Duration**: ~5-10 ms (depends on resolution)

**Visual Result**: Solid blue screen with memory map at bottom

Initialize Text Console
```


```

let fb = fb_response.framebuffers().next().expect("No framebuffer");
init_console(
);

```
**Steps**:

1. Create FramebufferConsole instance
2. Set cursor to (0, 0)
3. Store in global static

**Result**: ``fb_println!`` macro now functional

Test output

```

graphics::fb_println!("Welcome to Serix OS v0.0.5!");
graphics::fb_println!("Framebuffer: {}x{}", fb.width(), fb.height());
graphics::fb_println!("Memory: {} MB usable", total_mb);

```

## Phase 10: VFS Initialization (35-40 ms)

**Purpose**: Setup virtual filesystem with ramdisk

**Operations**

```

vfs::init_ramdisk(&module_response);

```
**Steps**:

1. Get module response from Limine
2. Locate init binary module
3. Create ramdisk filesystem
4. Mount at root (/)

**Result**: Files can be accessed through VFS


## Phase 11: Userspace Initialization (40-50 ms)

**Purpose**: Load and execute init binary

Load Init Binary
```


```

let init_binary = vfs::read_file("/init")

```
**Steps**:

1. Read init binary from ramdisk
2. Parse ELF headers
3. Allocate memory for segments
4. Map segments into memory
5. Setup userspace stack

Execute Init
```


```

loader::exec_elf(&init_binary)

```
**Result**: First userspace process running

Serial output

```

Init process started (PID 1)
Userspace initialized

```

## Subsystem Initialization


## Summary Table

===== ========== ======== ================ ================================
Phase Subsystem  Duration Dependencies     Purpose
===== ========== ======== ================ ================================
1     HAL        0-1 ms   None             Debug output (serial)
2     APIC       1-2 ms   HAL              Interrupt controller
3     IDT        2-3 ms   APIC             Exception/interrupt handling
4     Timer      3-4 ms   APIC, IDT        Timekeeping
5     FB Access  4-5 ms   Limine           Graphics access
6     Mem Map    5-10 ms  Limine           RAM discovery
7     Page Tbl   10-15 ms Memory Map       Virtual memory
8     Heap       15-25 ms Page Tables      Dynamic allocation
9     Graphics   25-35 ms Framebuffer,Heap Visual output
10    VFS        35-40 ms Heap             Filesystem
11    Userspace  40-50 ms VFS, Loader      Init process
===== ========== ======== ================ ================================


## Dependency Graph


```

Firmware
Limine
 HAL (Serial)    Page Tables |
APIC            Heap <-----------------+
 IDT ||
 Timer ||

```

## Critical Path

The critical path (longest dependency chain) is

```

Firmware -> Limine -> HAL -> APIC -> IDT -> Timer

```
Total critical path duration: ~4 ms

Parallel paths (Memory/Graphics/VFS) take longer but don't block interrupt setup.



## Boot Completion


## Main Loop Entry

After initialization completes, kernel enters idle loop

```

loop {
}

```
**Purpose**: Kernel idle loop

**Behavior**:

- Execute HLT instruction
- CPU enters low-power state
- Wakes on interrupt (timer, keyboard, etc.)
- Services interrupt
- Returns to HLT

**CPU Usage**: <1% (only active during interrupts)


## Boot Success Indicators

Serial console output

```

Serix Kernel v0.0.5 Starting...
Serial console initialized
Legacy PIC disabled
APIC enabled
Keyboard ready for input!
Init process started (PID 1)
Entering idle loop

```
Framebuffer display:

- Blue screen background
- Memory map bars at bottom (colored)
- Text: "Welcome to Serix OS v0.0.5!"
- Text: System information (memory, framebuffer resolution)

Interrupt functionality:

- Timer ticking (~625 Hz, visible in serial output if enabled)
- Keyboard responding to input (characters appear on screen)


## Boot Diagnostics


## Early Boot Debugging

**Serial Output**: Primary diagnostic tool

**Strategy**: Add serial_println! at each major step

Example checkpoint pattern

```

serial_println!("[CHECKPOINT] HAL init");
hal::init_serial();
serial_println!("[CHECKPOINT] APIC init");
apic::enable();
serial_println!("[CHECKPOINT] IDT init");
idt::init_idt();

```

## Boot Stages Logging

Enumeration for tracking boot progress

```

enum BootStage {
}

fn set_boot_stage(stage: BootStage) {
}

```

## Checkpoint Macro

Simple macro for progress tracking

```

macro_rules! checkpoint {
}

// Usage
checkpoint!("Serial initialized");
checkpoint!("APIC enabled");
checkpoint!("IDT loaded");

```

## Memory Map Dump

Detailed memory map logging

```

fn dump_memory_map(entries: &[&Entry]) {
}

```

## Troubleshooting


## No Serial Output

**Symptoms**: Nothing on serial console

**Causes**:

1. QEMU not redirecting serial (missing ``-serial stdio``)
2. Wrong COM port configured
3. Kernel not starting (bootloader failure)

**Debug Steps**

```

## QEMU with serial

qemu-system-x86_64 -serial stdio -cdrom serix.iso

## Check bootloader messages

qemu-system-x86_64 -serial file:boot.log -cdrom serix.iso

```

## Triple Fault / Reboot Loop

**Symptoms**: System reboots immediately after kernel entry

**Causes**:

1. Stack overflow
2. Page fault during early init
3. Invalid instruction
4. Exception before IDT loaded

**Debug Strategy**:

1. Add checkpoint at very start of _start
2. Binary search: comment out later code until boot succeeds
3. Check linker script (stack size, section alignment)

**QEMU Debug Mode**

```

qemu-system-x86_64 -d int,cpu_reset -no-reboot -cdrom serix.iso

```
This will dump interrupt/exception information and stop on triple fault instead
of rebooting.


## Hang During Initialization

**Symptoms**: Boot stops at specific point, no further output

**Causes**:

1. Infinite loop in initialization code
2. Waiting for hardware that doesn't respond
3. Deadlock on spinlock

**Debug Steps**:

1. Identify last checkpoint reached
2. Add more granular checkpoints
3. Check for infinite loops without timeout
4. Verify hardware is available in QEMU

Example timeout pattern

```

// Bad: Can hang forever
while !serial_port_ready() {}

// Good: Timeout
let mut timeout = 10000;
while !serial_port_ready() && timeout > 0 {
}
if timeout == 0 {
}

```

## No Framebuffer / Black Screen

**Symptoms**: No visual output, but serial works

**Causes**:

1. Framebuffer request not satisfied by bootloader
2. Invalid framebuffer address
3. Wrong pixel format
4. QEMU graphics backend issue

**Debug Code**

```

let fb_response = FRAMEBUFFER_REQ.get_response();
if fb_response.is_none() {
}

let fb = fb_response.unwrap().framebuffers().next();
if fb.is_none() {
}

let fb = fb.unwrap();
serial_println!("Framebuffer: {:#x}", fb.addr());
serial_println!("  Size: {}x{}", fb.width(), fb.height());
serial_println!("  Pitch: {}", fb.pitch());
serial_println!("  BPP: {}", fb.bpp());

```

## Interrupt Issues

**Symptoms**: Timer not ticking, keyboard not responding

**Causes**:

1. Interrupts not enabled (IF flag)
2. APIC not enabled
3. I/O APIC not configured
4. IDT not loaded
5. Missing EOI in handler

**Debug Code**

```

// Check IF flag
let rflags: u64;
unsafe {
}
serial_println!("RFLAGS: {:#x} (IF={})", rflags, (rflags >> 9) & 1);

// Check APIC enabled
let svr = unsafe { lapic_reg(0xF0).read_volatile() };
serial_println!("APIC SVR: {:#x} (enabled={})", svr, (svr >> 8) & 1);

```

## Page Fault During Boot

**Symptoms**: Page fault exception logged, system halts

**Causes**:

1. Accessing unmapped memory
2. Heap not initialized before allocation
3. Stack overflow
4. Invalid pointer dereference

**Debug Information**:

- Check CR2 (faulting address)
- Check RIP (instruction pointer where fault occurred)
- Check error code (present, write, user flags)

**Prevention**

```

// Ensure heap initialized before use
init_heap(&mut mapper, &mut frame_alloc);

// Now safe to allocate
let v = Vec::new();

```

## Out of Memory

**Symptoms**: Allocation returns null, system panics

**Causes**:

1. Heap too small (1 MB default)
2. Memory leak
3. Large allocation attempt

**Solutions**:

Increase heap size in ``memory/src/heap.rs``

```

pub const HEAP_SIZE: usize = 4 *1024* 1024;  // 4 MB

```
Add heap statistics for debugging

```

serial_println!("Heap: {} / {} bytes used", heap_used(), HEAP_SIZE);

```

## Build and Run Commands


## Building

Build kernel binary

```

cargo build --release --manifest-path kernel/Cargo.toml \

```
Build init userspace binary

```

make init

```
Create bootable ISO

```

make iso

```

## Running

Run in QEMU with default settings

```

make run

```
Run with serial output to file

```

qemu-system-x86_64 -serial file:serial.log -cdrom build/serix.iso

```
Run with debugging

```

qemu-system-x86_64 -serial stdio -d int,cpu_reset \

```
Run with GDB support

```

qemu-system-x86_64 -serial stdio -s -S -cdrom build/serix.iso

## In another terminal:

gdb target/x86_64-unknown-none/release/kernel
(gdb) target remote :1234
(gdb) break_start
(gdb) continue

```

## Cleaning

Clean build artifacts

```

make clean
cargo clean

```

## Appendix


## Boot Sequence Timing (QEMU)

Typical timing on modern hardware

```

Time (ms)   Event
─────────────────────────────────────────────────────────
0           Firmware POST begins
500-2000    Firmware POST completes, Limine Stage 1 loads
2000        Limine Stage 2 loads
2100        Kernel ELF loaded into memory
2200        Kernel entry (_start)
2201        Serial init
2202        APIC init
2203        IDT init
2204        Timer init
2205        Framebuffer access
2215        Memory map processed
2225        Page table init
2235        Heap init
2240        Graphics init
2245        VFS init
2250        Init binary loaded and executed
2252        Main loop entered
─────────────────────────────────────────────────────────
Total: ~2.2 seconds (typical)

```

## ISO Structure

The bootable ISO has this structure

```

serix.iso/
├── boot/
│   ├── serix-kernel          # Kernel ELF binary
│   └── init                  # Init userspace binary
├── limine/
│   ├── limine-bios.sys       # BIOS bootloader
│   ├── limine-bios-cd.bin    # BIOS CD boot
│   └── limine-uefi-cd.bin    # UEFI CD boot
├── EFI/
│   └── BOOT/
│       └── BOOTX64.EFI       # UEFI bootloader
└── limine.conf               # Boot configuration

```

## Limine Files

Bootloader components

```

limine/
├── limine-bios.sys       # BIOS bootloader (loaded by Stage 1)
├── limine-bios-cd.bin    # BIOS CD boot sector
├── limine-uefi-cd.bin    # UEFI CD boot sector
└── BOOTX64.EFI           # UEFI bootloader application

```

## QEMU Command Reference

Common QEMU options for Serix

```

-m 4G                     # 4GB RAM
-serial stdio             # Serial output to terminal
-d int,cpu_reset          # Debug interrupts and CPU resets
-no-reboot                # Stop on triple fault instead of reboot
-s                        # GDB server on port 1234
-S                        # Start paused (wait for GDB)
-drive file=disk.img,...  # Attach disk image
-device virtio-blk-pci    # VirtIO block device

```

## See Also

- **[Memory Layout](MEMORY_LAYOUT.md)** - Virtual and physical memory organization
- **[Interrupt Handling](INTERRUPT_HANDLING.md)** - IDT and APIC configuration details
- **[Architecture Overview](ARCHITECTURE.md)** - High-level system design
- **[HAL Module](../hal/README.md)** - Serial console and hardware abstraction implementation
- **[APIC Module](../apic/README.md)** - APIC interrupt controller implementation
- **[Kernel Module](../kernel/README.md)** - Kernel entry point and initialization code
- **[Graphics Module](../graphics/README.md)** - Framebuffer console implementation
