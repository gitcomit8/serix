# Boot Process Technical Specification

**Document Version:** 1.0  
**Last Updated:** 2025-10-13  
**Target Architecture:** x86_64  
**Bootloader:** Limine v8.x  

## Table of Contents

1. [Overview](#overview)
2. [Boot Stages](#boot-stages)
3. [Limine Bootloader](#limine-bootloader)
4. [Kernel Entry Point](#kernel-entry-point)
5. [Initialization Sequence](#initialization-sequence)
6. [Subsystem Initialization](#subsystem-initialization)
7. [Boot Completion](#boot-completion)
8. [Boot Diagnostics](#boot-diagnostics)
9. [Troubleshooting](#troubleshooting)

---

## Overview

Serix uses a multi-stage boot process that transitions from firmware (BIOS/UEFI) through the Limine bootloader to the kernel proper. This document specifies each stage, the handoff between stages, and the complete kernel initialization sequence.

### Boot Flow Summary

```
┌─────────────────┐
│ Firmware        │  BIOS/UEFI
│ (BIOS/UEFI)     │  - POST (Power-On Self-Test)
└────────┬────────┘  - Initialize hardware
         │           - Load bootloader from disk
         ▼
┌─────────────────┐
│ Stage 1         │  Limine Stage 1
│ Bootloader      │  - Load Stage 2 from disk
└────────┬────────┘  - Minimal environment
         │
         ▼
┌─────────────────┐
│ Stage 2         │  Limine Stage 2
│ Bootloader      │  - Setup memory map
│ (Limine)        │  - Load kernel ELF
└────────┬────────┘  - Setup framebuffer
         │           - Enter long mode (64-bit)
         │           - Setup initial page tables
         │           - Parse config (limine.conf)
         ▼
┌─────────────────┐
│ Kernel Entry    │  _start() function
│ (_start)        │  - Serial init (debug output)
└────────┬────────┘  - Read bootloader info
         │           - System detection
         ▼
┌─────────────────┐
│ Early Init      │  Critical subsystems
│                 │  - APIC setup
└────────┬────────┘  - IDT setup
         │           - Interrupts enable
         ▼
┌─────────────────┐
│ Memory Init     │  Memory management
│                 │  - Page tables
└────────┬────────┘  - Frame allocator
         │           - Heap initialization
         ▼
┌─────────────────┐
│ Device Init     │  Device drivers
│                 │  - Framebuffer
└────────┬────────┘  - Console
         │           - Timer
         ▼
┌─────────────────┐
│ Scheduler Init  │  Task management
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Main Loop       │  Kernel idle loop
│                 │  - HLT instruction
└─────────────────┘  - Wait for interrupts
```

### Time Estimates

| Stage | Approximate Duration | Notes |
|-------|---------------------|--------|
| Firmware POST | 0.5-3 seconds | Varies by hardware |
| Limine Stage 1 | < 100 ms | Minimal work |
| Limine Stage 2 | 100-500 ms | Loads kernel, sets up environment |
| Kernel Early Init | 1-10 ms | Serial, APIC, IDT |
| Memory Init | 5-50 ms | Depends on RAM size |
| Device Init | 1-5 ms | Framebuffer, console |
| Total Boot Time | 1-4 seconds | Typical on modern hardware |

---

## Boot Stages

### Stage 0: Firmware (BIOS/UEFI)

#### BIOS Boot

**Process**:
1. Power on → CPU starts at reset vector (0xFFFFFFF0)
2. Jump to BIOS code in ROM
3. POST (Power-On Self-Test)
   - Memory test
   - Hardware detection
   - Initialize chipset
4. Enumerate boot devices (HDD, SSD, USB, CD-ROM, Network)
5. Read MBR (Master Boot Record) from first sector of boot device
6. Load bootloader code from MBR to 0x7C00
7. Jump to 0x7C00 (bootloader entry)

**Environment at Handoff**:
- **CPU Mode**: Real mode (16-bit)
- **Memory**: Low memory available (<1 MB)
- **A20 Line**: May be disabled (limits addressing to 1 MB)

#### UEFI Boot

**Process**:
1. Power on → CPU starts at reset vector
2. SEC (Security) Phase: Minimal setup
3. PEI (Pre-EFI Initialization): Memory and platform init
4. DXE (Driver Execution Environment): Load drivers
5. BDS (Boot Device Selection): Enumerate boot options
6. Read EFI System Partition (ESP, FAT32)
7. Load EFI bootloader application (.efi file)
8. Execute bootloader in UEFI context

**Environment at Handoff**:
- **CPU Mode**: Long mode (64-bit) or protected mode (32-bit)
- **Memory**: Full memory available
- **Services**: UEFI runtime services available

### Stage 1: Limine Stage 1

**Location**: MBR/VBR (Volume Boot Record)

**Size**: 446 bytes (MBR) or 512 bytes (VBR)

**Purpose**: Load Stage 2 from disk

**Limitations**:
- Minimal code space
- Real mode only (BIOS) or limited environment (UEFI)
- No filesystem support

**Operation**:
1. Read Stage 2 sectors from known disk location
2. Load Stage 2 to memory
3. Jump to Stage 2 entry point

### Stage 2: Limine Stage 2

**Location**: Dedicated partition or filesystem

**Size**: ~200 KB

**Purpose**: Full bootloader functionality

**Capabilities**:
- Filesystem support (ext2, FAT32, etc.)
- ELF loading
- Memory map creation
- Framebuffer initialization
- Configuration parsing

**Operation**:
1. Parse configuration file (`limine.conf`)
2. Setup memory map
3. Setup framebuffer (if available)
4. Load kernel ELF from filesystem
5. Setup initial page tables
6. Enter long mode (if not already)
7. Create Limine boot info structures
8. Jump to kernel entry point

---

## Limine Bootloader

### Limine Protocol

**Version**: 8.x

**Documentation**: https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md

**Request/Response Model**:
- Kernel defines "requests" in special ELF sections
- Bootloader populates "responses" before kernel entry
- Allows kernel to request specific features/information

### Configuration File

**Path**: `/limine.conf` (root of boot partition)

**Serix Configuration**:
```
TIMEOUT=3

:Serix OS
    PROTOCOL=limine
    KERNEL_PATH=boot:///serix-kernel
```

**Directives**:
- `TIMEOUT`: Seconds to wait before auto-boot
- `:Entry Name`: Boot menu entry
- `PROTOCOL=limine`: Use Limine protocol
- `KERNEL_PATH`: Path to kernel ELF (boot:/// = boot partition root)

### Boot Protocol Requests

Serix defines several requests in kernel code:

#### Base Revision Request

```rust
static BASE_REVISION: BaseRevision = BaseRevision::new();
```

**Purpose**: Declares bootloader protocol version.

**Section**: `.limine_reqs`

**Required**: Yes (protocol version check)

#### Framebuffer Request

```rust
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
```

**Purpose**: Requests graphics framebuffer.

**Response Includes**:
- Framebuffer physical address
- Width, height (pixels)
- Pitch (bytes per line)
- Bits per pixel (BPP)
- Pixel format (BGRA, RGB, etc.)

**Multiple Framebuffers**: Response may include multiple displays.

#### Memory Map Request

```rust
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
```

**Purpose**: Requests system memory map.

**Response Includes**:
- Array of memory regions
- Each region: base address, length, type

**Memory Types**:
- `USABLE`: Free RAM
- `RESERVED`: Hardware reserved
- `ACPI_RECLAIMABLE`: ACPI tables
- `ACPI_NVS`: ACPI Non-Volatile Storage
- `BAD_MEMORY`: Defective RAM
- `BOOTLOADER_RECLAIMABLE`: Bootloader code/data
- `KERNEL_AND_MODULES`: Kernel image
- `FRAMEBUFFER`: Framebuffer memory

### Limine Handoff State

**CPU State at Kernel Entry**:
```
RIP:       Kernel entry point (_start)
CS:        Kernel code segment (typically 0x08)
DS/ES/SS:  Kernel data segment (typically 0x10)
RSP:       Valid stack pointer
RFLAGS:    IF=0 (interrupts disabled), DF=0
CR0:       PE=1 (protected mode), PG=1 (paging enabled)
CR3:       Page table base (identity + higher half mapped)
CR4:       PAE=1, PGE=1, OSFXSR=1, OSXMMEXCPT=1
```

**Memory Layout**:
```
Identity mapping:   0x0 - [physical RAM size]
Higher half:        0xFFFF_FFFF_8000_0000 - 0xFFFF_FFFF_FFFF_FFFF
                    (Kernel loaded here)
Physical mapping:   (Not setup by bootloader, kernel does this)
```

**GDT (Global Descriptor Table)**:
- Entry 0: Null descriptor
- Entry 1: Kernel code segment (0x08)
- Entry 2: Kernel data segment (0x10)

**Interrupts**: Disabled (IF=0), IDT not loaded

---

## Kernel Entry Point

### _start Function

**Location**: `kernel/src/main.rs`

**Signature**:
```rust
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> !
```

**Attributes**:
- `#[unsafe(no_mangle)]`: Preserve function name for linker
- `extern "C"`: C calling convention
- `-> !`: Never returns (diverging function)

**Entry Conditions**:
- Interrupts disabled
- Paging enabled (bootloader's page tables)
- Stack allocated by bootloader
- Limine responses populated

### Initial Code

```rust
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // PHASE 1: Minimal Setup
    hal::init_serial();
    serial_println!("Serix Kernel Starting.....");
    serial_println!("Serial console initialized");
    
    // PHASE 2: Interrupt Setup
    unsafe {
        apic::enable();
        apic::ioapic::init_ioapic();
        apic::timer::register_handler();
    }
    idt::init_idt();
    x86_64::instructions::interrupts::enable();
    
    unsafe {
        apic::timer::init_hardware();
    }
    
    // PHASE 3: Memory Discovery
    let fb_response = FRAMEBUFFER_REQ.get_response()
        .expect("No framebuffer reply");
    let mmap_response = MMAP_REQ.get_response()
        .expect("No memory map response");
    
    // ... continued below
}
```

---

## Initialization Sequence

### Phase 1: Serial Console (0-1 ms)

**Purpose**: Establish debug output channel.

**Operations**:
1. Initialize COM1 serial port (0x3F8)
2. Configure: 115200 baud, 8N1
3. Enable FIFO
4. Test with initial message

**Code Path**: `hal::init_serial()` → `hal/src/serial.rs`

**Critical**: Must be first step for debug output.

**Output**:
```
Serix Kernel Starting.....
Serial console initialized
```

### Phase 2: APIC Setup (1-2 ms)

**Purpose**: Setup interrupt controller.

**Operations**:

#### 2a. Disable Legacy PIC
```rust
unsafe { apic::enable(); }  // Also calls disable_pic()
```

**Steps**:
1. Initialize PIC (ICW1-ICW4)
2. Remap IRQs to vectors 32-47
3. Mask all interrupts (0xFF to both data ports)

**Why**: Prevent conflicts between PIC and APIC.

#### 2b. Enable Local APIC
```rust
unsafe { apic::enable(); }
```

**Steps**:
1. Read IA32_APIC_BASE MSR (0x1B)
2. Set bit 11 (APIC Global Enable)
3. Write back MSR
4. Write 0x100 to SVR register (offset 0xF0)

**Verification**: Read SVR, check bit 8 is set.

#### 2c. Initialize I/O APIC
```rust
unsafe { apic::ioapic::init_ioapic(); }
```

**Steps**:
1. Map IRQ0 (timer) to vector 32
2. Map IRQ1 (keyboard) to vector 33

**Registers**: I/O APIC redirection table entries.

#### 2d. Register Timer Handler
```rust
unsafe { apic::timer::register_handler(); }
```

**Purpose**: Register handler before IDT is loaded.

**Handler**: `timer_interrupt` (vector 49)

**Output**:
```
Legacy PIC disabled
APIC enabled
```

### Phase 3: IDT Setup (2-3 ms)

**Purpose**: Setup interrupt and exception handlers.

**Operations**:

#### 3a. Load IDT
```rust
idt::init_idt();
```

**Steps**:
1. Initialize IDT structure (lazy_static)
2. Set exception handlers (divide by zero, page fault, double fault)
3. Set hardware interrupt handlers (keyboard)
4. Load IDT into IDTR register (LIDT instruction)

**Vectors Configured**:
- 0: Divide by zero
- 8: Double fault
- 14: Page fault
- 33: Keyboard
- 49: Timer (registered earlier)

#### 3b. Enable Interrupts
```rust
x86_64::instructions::interrupts::enable();
```

**Operation**: Execute STI instruction (set IF flag).

**Effect**: CPU now responds to hardware interrupts.

**Output**: (None, silent operation)

### Phase 4: Timer Hardware (3-4 ms)

**Purpose**: Start LAPIC timer for timekeeping.

**Operations**:
```rust
unsafe { apic::timer::init_hardware(); }
```

**Steps**:
1. Write divide config register (0x3E0) = 0x3 (divide by 16)
2. Write LVT timer register (0x320) = vector 49 | 0x20000 (periodic)
3. Write initial count register (0x380) = 100,000

**Result**: Timer fires every ~1.6 ms (625 Hz).

**Output**:
```
Keyboard ready for input!
```

### Phase 5: Framebuffer Access (4-5 ms)

**Purpose**: Get graphics output capability.

**Operations**:
```rust
let fb_response = FRAMEBUFFER_REQ.get_response()
    .expect("No framebuffer reply");
```

**Response Fields**:
- Framebuffer count (typically 1)
- For each framebuffer:
  - Physical address
  - Width, height (pixels)
  - Pitch (bytes per line)
  - BPP (bits per pixel, typically 32)
  - Memory model (typically RGB)

**Usage**: Passed to graphics subsystem for rendering.

### Phase 6: Memory Map Processing (5-10 ms)

**Purpose**: Discover available RAM.

**Operations**:
```rust
let mmap_response = MMAP_REQ.get_response()
    .expect("No memory map response");
let entries = mmap_response.entries();
```

**Processing**:
```rust
for entry in entries.iter() {
    serial_println!("Memory region:");
    serial_println!("  Base:   {:#x}", entry.base);
    serial_println!("  Length: {:#x}", entry.length);
    serial_println!("  Type:   {:?}", entry.entry_type);
}
```

**Output Example**:
```
Memory region:
  Base:   0x0
  Length: 0x9fc00
  Type:   USABLE
Memory region:
  Base:   0x100000
  Length: 0x3fef0000
  Type:   USABLE
Memory region:
  Base:   0x40000000
  Length: 0x200000
  Type:   RESERVED
...
```

### Phase 7: Page Table Initialization (10-15 ms)

**Purpose**: Setup virtual memory management.

**Operations**:

#### 7a. Initialize Offset Page Table
```rust
let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };
```

**Steps**:
1. Read CR3 (get current PML4 physical address)
2. Convert to virtual address using offset
3. Create OffsetPageTable wrapper

**Result**: `mapper` can be used to manipulate page tables.

#### 7b. Preallocate Frames
```rust
let mut frame_count = 0;
for region in entries.iter()
    .filter(|r| r.entry_type == EntryType::USABLE)
{
    let start_frame = PhysFrame::containing_address(PhysAddr::new(region.base));
    let end_frame = PhysFrame::containing_address(PhysAddr::new(region.base + region.length - 1));
    
    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        if frame_count < memory::heap::MAX_BOOT_FRAMES {
            unsafe {
                memory::heap::BOOT_FRAMES[frame_count] = Some(frame);
            }
            frame_count += 1;
        }
    }
}
```

**Purpose**: Store available frames in static array (pre-heap).

**Capacity**: 65,536 frames (256 MB).

**Output**: `frame_count` = number of available frames.

#### 7c. Create Frame Allocator
```rust
let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);
```

**Purpose**: Wrap frame array in FrameAllocator trait.

### Phase 8: Heap Initialization (15-25 ms)

**Purpose**: Enable dynamic memory allocation.

**Operations**:
```rust
hal::cpu::enable_interrupts();
init_heap(&mut mapper, &mut frame_alloc);
```

**Steps**:
1. Calculate heap page range (256 pages for 1 MB)
2. For each page:
   - Allocate physical frame
   - Map virtual page (0x4444_4444_0000 + offset) to frame
   - Set flags: Present + Writable
   - Flush TLB
3. Initialize linked list allocator

**Result**: Rust `alloc` crate now functional (Vec, Box, String, etc.).

**Duration**: ~10 ms (depends on page count and TLB flush overhead).

**Output**: (None, silent operation)

### Phase 9: Graphics Initialization (25-35 ms)

**Purpose**: Setup visual output.

**Operations**:

#### 9a. Paint Screen Blue
```rust
if let Some(fb) = fb_response.framebuffers().next() {
    fill_screen_blue(&fb);
    draw_memory_map(&fb, mmap_response.entries());
}
```

**Steps**:
1. Get first framebuffer
2. Fill entire screen with blue pixels (0xFF 0x00 0x00 0x00)
3. Draw memory map visualization (colored bars at bottom)

**Duration**: ~5-10 ms (depends on resolution).

**Visual Result**: Solid blue screen with memory map at bottom.

#### 9b. Initialize Text Console
```rust
let fb = fb_response.framebuffers().next().expect("No framebuffer");
init_console(fb.addr(), fb.width() as usize, fb.height() as usize, fb.pitch() as usize);
```

**Steps**:
1. Create FramebufferConsole instance
2. Set cursor to (0, 0)
3. Store in global static

**Result**: `fb_println!` macro now functional.

**Test Output**:
```rust
graphics::fb_println!("Welcome to Serix OS!");
graphics::fb_println!("Memory map entries: {}", mmap_response.entries().len());
```

**Visual Result**: Text appears on framebuffer.

### Phase 10: Scheduler Initialization (35-36 ms)

**Purpose**: Setup task management (currently minimal).

**Operations**:
```rust
Scheduler::init_global();
```

**Steps**:
1. Call Once::call_once to initialize global scheduler
2. Create empty task list

**Result**: Scheduler ready for task registration.

**Note**: No tasks created yet, scheduler not started.

---

## Subsystem Initialization

### Summary Table

| Phase | Subsystem | Duration | Dependencies | Purpose |
|-------|-----------|----------|--------------|---------|
| 1 | HAL (Serial) | 0-1 ms | None | Debug output |
| 2 | APIC | 1-2 ms | HAL | Interrupt controller |
| 3 | IDT | 2-3 ms | APIC | Exception/interrupt handling |
| 4 | Timer | 3-4 ms | APIC, IDT | Timekeeping |
| 5 | Framebuffer | 4-5 ms | Limine | Graphics access |
| 6 | Memory Map | 5-10 ms | Limine | RAM discovery |
| 7 | Page Tables | 10-15 ms | Memory Map | Virtual memory |
| 8 | Heap | 15-25 ms | Page Tables | Dynamic allocation |
| 9 | Graphics | 25-35 ms | Framebuffer, Heap | Visual output |
| 10 | Scheduler | 35-36 ms | Heap | Task management |

### Dependency Graph

```
Firmware
    ↓
Limine
    ↓
┌───────────────┐
│     HAL       │
│   (Serial)    │
└───────┬───────┘
        ↓
    ┌───────┐
    │ APIC  │
    └───┬───┘
        ↓
    ┌───────┐
    │  IDT  │
    └───┬───┘
        ↓
    ┌───────┐
    │ Timer │
    └───────┘
        
    Limine
        ↓
    ┌──────────┐
    │Framebuffer│
    └─────┬────┘
          │
    ┌─────┴────────┐
    │  Memory Map  │
    └─────┬────────┘
          ↓
    ┌─────────────┐
    │ Page Tables │
    └──────┬──────┘
           ↓
    ┌──────────┐
    │   Heap   │
    └─────┬────┘
          │
    ┌─────┴────────┐
    │   Graphics   │
    │   Console    │
    └──────────────┘
    
    ┌──────────┐
    │Scheduler │
    └──────────┘
```

### Critical Path

The critical path (longest dependency chain) is:

```
Firmware → Limine → HAL → APIC → IDT → Timer
```

Total critical path: ~4 ms

Parallel paths (Memory/Graphics) take longer but don't block interrupt setup.

---

## Boot Completion

### Main Loop Entry

**Code**:
```rust
loop {
    hlt()
}
```

**Purpose**: Kernel idle loop.

**Behavior**:
- Execute HLT instruction
- CPU enters low-power state
- Wakes on interrupt (timer, keyboard, etc.)
- Services interrupt
- Returns to HLT

**CPU Usage**: <1% (only active during interrupts)

### Boot Success Indicators

**Serial Console**:
```
Serix Kernel Starting.....
Serial console initialized
Legacy PIC disabled
APIC enabled
Keyboard ready for input!
```

**Framebuffer**:
- Blue screen
- Memory map bars at bottom
- Text: "Welcome to Serix OS!"
- Text: "Memory map entries: X"

**Interrupts**:
- Timer ticking (~625 Hz)
- Keyboard responding to input

### Boot Time Measurement

**Add to kernel**:
```rust
// At start
let start_tsc = read_tsc();

// At end
let end_tsc = read_tsc();
let cycles = end_tsc - start_tsc;
let ms = cycles / (cpu_freq_mhz * 1000);
serial_println!("Boot time: {} ms ({} cycles)", ms, cycles);
```

**Typical Results**:
- QEMU: 10-50 ms (depends on host speed)
- Real Hardware: 20-100 ms (depends on initialization overhead)

---

## Boot Diagnostics

### Early Boot Debugging

**Serial Output**: Primary diagnostic tool.

**Strategy**: Add serial_println! at each major step.

**Example**:
```rust
serial_println!("[1/10] HAL init");
hal::init_serial();
serial_println!("[2/10] APIC init");
apic::enable();
serial_println!("[3/10] IDT init");
idt::init_idt();
// ... etc
```

### Boot Stages Logging

```rust
enum BootStage {
    Firmware,
    Bootloader,
    KernelEntry,
    HalInit,
    ApicInit,
    IdtInit,
    TimerInit,
    MemoryInit,
    HeapInit,
    GraphicsInit,
    SchedulerInit,
    MainLoop,
}

fn set_boot_stage(stage: BootStage) {
    unsafe { BOOT_STAGE = stage; }
    serial_println!("Boot stage: {:?}", stage);
}
```

### Checkpoint System

```rust
macro_rules! checkpoint {
    ($msg:expr) => {
        serial_println!("[CHECKPOINT] {}", $msg);
    };
}

// Usage
checkpoint!("Serial initialized");
checkpoint!("APIC enabled");
checkpoint!("IDT loaded");
```

### Memory Map Dump

```rust
fn dump_memory_map(entries: &[&Entry]) {
    serial_println!("=== Memory Map ===");
    for (i, entry) in entries.iter().enumerate() {
        serial_println!("{}: {:#018x} - {:#018x} ({:#x} bytes) {:?}",
            i,
            entry.base,
            entry.base + entry.length,
            entry.length,
            entry.entry_type
        );
    }
}
```

---

## Troubleshooting

### No Serial Output

**Symptoms**: Nothing on serial console.

**Causes**:
1. QEMU not redirecting serial (missing `-serial stdio`)
2. Wrong COM port
3. Kernel not starting (bootloader failure)

**Debug**:
```bash
# QEMU with serial
qemu-system-x86_64 -serial stdio -cdrom serix.iso

# Check bootloader messages
qemu-system-x86_64 -serial file:boot.log -cdrom serix.iso
```

### Triple Fault / Reboot Loop

**Symptoms**: System reboots immediately after kernel entry.

**Causes**:
1. Stack overflow
2. Page fault during early init
3. Invalid instruction
4. Exception before IDT loaded

**Debug Strategy**:
1. Add checkpoint at very start of _start
2. Binary search: comment out later code until boot succeeds
3. Check linker script (stack size, section alignment)

**QEMU Debug**:
```bash
qemu-system-x86_64 -d int,cpu_reset -no-reboot -cdrom serix.iso
```

### Hang During Initialization

**Symptoms**: Boot stops at specific point, no further output.

**Causes**:
1. Infinite loop in initialization code
2. Waiting for hardware that doesn't respond
3. Deadlock on spinlock

**Debug**:
1. Identify last checkpoint reached
2. Add more granular checkpoints
3. Check for infinite loops (while without timeout)

**Example**:
```rust
// Bad: Can hang forever
while !serial_port_ready() {}

// Good: Timeout
let mut timeout = 10000;
while !serial_port_ready() && timeout > 0 {
    timeout -= 1;
}
if timeout == 0 {
    serial_println!("ERROR: Serial port timeout");
}
```

### No Framebuffer / Black Screen

**Symptoms**: No visual output, but serial works.

**Causes**:
1. Framebuffer request not satisfied by bootloader
2. Invalid framebuffer address
3. Wrong pixel format
4. QEMU graphics backend issue

**Debug**:
```rust
let fb_response = FRAMEBUFFER_REQ.get_response();
if fb_response.is_none() {
    serial_println!("ERROR: No framebuffer response");
    halt_loop();
}

let fb = fb_response.unwrap().framebuffers().next();
if fb.is_none() {
    serial_println!("ERROR: No framebuffer in response");
    halt_loop();
}

let fb = fb.unwrap();
serial_println!("Framebuffer: {:#x}", fb.addr());
serial_println!("  Size: {}x{}", fb.width(), fb.height());
serial_println!("  Pitch: {}", fb.pitch());
serial_println!("  BPP: {}", fb.bpp());
```

### Interrupt Issues

**Symptoms**: Timer not ticking, keyboard not responding.

**Causes**:
1. Interrupts not enabled (IF flag)
2. APIC not enabled
3. I/O APIC not configured
4. IDT not loaded
5. Missing EOI in handler

**Debug**:
```rust
// Check IF flag
let rflags: u64;
unsafe {
    core::arch::asm!("pushfq; pop {}", out(reg) rflags);
}
serial_println!("RFLAGS: {:#x} (IF={})", rflags, (rflags >> 9) & 1);

// Check APIC enabled
let svr = unsafe { lapic_reg(0xF0).read_volatile() };
serial_println!("APIC SVR: {:#x} (enabled={})", svr, (svr >> 8) & 1);

// Dump I/O APIC redirects
unsafe { dump_ioapic_redirects(); }
```

### Page Fault During Boot

**Symptoms**: Page fault exception logged, system halts.

**Causes**:
1. Accessing unmapped memory
2. Heap not initialized before allocation
3. Stack overflow
4. Invalid pointer

**Debug**:
- Check CR2 (faulting address)
- Check RIP (where fault occurred)
- Check error code (present, write, user flags)

**Prevention**:
```rust
// Ensure heap initialized before use
init_heap(&mut mapper, &mut frame_alloc);

// Now safe to allocate
let v = Vec::new();
```

### Out of Memory

**Symptoms**: Allocation returns null, system panics.

**Causes**:
1. Heap too small (1 MB default)
2. Memory leak
3. Large allocation

**Solutions**:
1. Increase heap size:
   ```rust
   const HEAP_SIZE: usize = 4 * 1024 * 1024;  // 4 MB
   ```
2. Add heap statistics:
   ```rust
   serial_println!("Heap used: {} bytes", heap_used());
   ```

---

## Appendix

### Boot Command Line (QEMU)

```bash
# Basic boot
qemu-system-x86_64 -cdrom serix.iso

# With serial output
qemu-system-x86_64 -serial stdio -cdrom serix.iso

# Debug mode
qemu-system-x86_64 \
    -serial stdio \
    -d int,cpu_reset \
    -no-reboot \
    -cdrom serix.iso

# GDB debugging
qemu-system-x86_64 \
    -serial stdio \
    -s -S \
    -cdrom serix.iso

# In another terminal
gdb target/x86_64-unknown-none/debug/kernel
(gdb) target remote :1234
(gdb) break _start
(gdb) continue
```

### Boot Sequence Timing

```
Time (ms)   Event
────────────────────────────────────────────
0           Firmware POST begins
500-2000    Firmware POST completes
            Bootloader Stage 1 loads
2000        Bootloader Stage 2 loads
2100        Kernel ELF loaded
2200        Kernel entry (_start)
2201        Serial init
2202        APIC init
2203        IDT init
2204        Timer init
2205        Framebuffer access
2215        Memory map process
2225        Page table init
2235        Heap init
2240        Graphics init
2241        Scheduler init
2242        Main loop entered
────────────────────────────────────────────
Total: ~2.2 seconds (typical)
```

### Build and Run Commands

```bash
# Build kernel
cargo build --release

# Create ISO
make iso

# Run in QEMU
make run

# Run with serial output
make run-serial

# Clean build
make clean
```

### Limine Files

```
limine/
├── limine-bios.sys       # BIOS bootloader
├── limine-bios-cd.bin    # BIOS CD boot
├── limine-uefi-cd.bin    # UEFI CD boot
└── BOOTX64.EFI           # UEFI bootloader
```

### ISO Structure

```
serix.iso
├── boot/
│   └── serix-kernel      # Kernel ELF
├── limine/
│   ├── limine-bios.sys
│   ├── limine-bios-cd.bin
│   └── limine-uefi-cd.bin
├── EFI/
│   └── BOOT/
│       └── BOOTX64.EFI
└── limine.conf           # Boot configuration
```

---

**End of Document**
