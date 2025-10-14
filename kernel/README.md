# Kernel Module

## Overview

The kernel module is the central coordinator and entry point for the Serix operating system. It orchestrates all subsystems, manages system initialization, and provides the main execution loop. This module is responsible for bootstrapping the system from the Limine bootloader handoff to a fully operational kernel environment.

## Architecture

### Core Responsibilities

1. **System Bootstrap**: Initial entry point from bootloader
2. **Subsystem Coordination**: Initializes and coordinates all kernel subsystems
3. **Hardware Discovery**: Interfaces with Limine boot protocol for system information
4. **Main Loop**: Provides the kernel's main execution loop

### Design Philosophy

The kernel follows a modular, subsystem-based architecture where each major component (APIC, IDT, memory, graphics, etc.) is separated into independent crates. This design promotes:

- **Modularity**: Each subsystem can be developed and tested independently
- **Separation of Concerns**: Clear boundaries between different system components
- **Maintainability**: Easier to understand and modify individual components
- **Reusability**: Subsystems can potentially be reused in other projects

## Entry Point: `_start()`

The `_start()` function is the kernel's entry point, called by the Limine bootloader. It performs a carefully orchestrated initialization sequence:

### Initialization Sequence

```
1. Serial Console Initialization
   ↓
2. APIC & Interrupt Controller Setup
   ↓
3. IDT (Interrupt Descriptor Table) Setup
   ↓
4. Interrupt Enable
   ↓
5. Timer Hardware Initialization
   ↓
6. Framebuffer Acquisition
   ↓
7. Memory Map Processing
   ↓
8. Page Table Initialization
   ↓
9. Frame Allocator Setup
   ↓
10. Heap Initialization
    ↓
11. Graphics Console Initialization
    ↓
12. Scheduler Initialization
    ↓
13. Main Kernel Loop
```

### Detailed Phase Breakdown

#### Phase 1: Serial Console (Early Debug Output)

```rust
hal::init_serial();
serial_println!("Serix Kernel Starting.....");
```

**Purpose**: Establishes early debugging capability before any complex subsystems are initialized. This is critical for diagnosing boot failures.

**Dependencies**: None (operates directly on COM1 port 0x3F8)

#### Phase 2: APIC Initialization

```rust
unsafe {
    apic::enable();              // Enable Local APIC and disable PIC
    apic::ioapic::init_ioapic(); // Route IRQs through I/O APIC
    apic::timer::register_handler(); // Register timer handler
}
```

**Purpose**: 
- Disables legacy 8259 PIC (Programmable Interrupt Controller)
- Enables modern APIC (Advanced Programmable Interrupt Controller)
- Routes hardware interrupts (keyboard, timer) through I/O APIC
- Registers timer interrupt handler before IDT is loaded

**Why APIC over PIC?**
- Better multiprocessor support (future-ready)
- More flexible interrupt routing
- Higher performance
- Required for modern x86_64 systems

#### Phase 3: IDT Initialization

```rust
idt::init_idt(); // Setup CPU exception handlers and load IDT
```

**Purpose**: Loads the Interrupt Descriptor Table with handlers for:
- CPU exceptions (divide by zero, page faults, double faults)
- Hardware interrupts (keyboard IRQ1, timer IRQ0)

**Critical Note**: Must be done before enabling interrupts globally

#### Phase 4: Global Interrupt Enable

```rust
x86_64::instructions::interrupts::enable();
```

**Purpose**: Enables CPU interrupt flag (IF), allowing hardware interrupts to be received.

**Safety**: Only safe after IDT is loaded with proper handlers

#### Phase 5: Timer Hardware Initialization

```rust
unsafe {
    apic::timer::init_hardware();
}
```

**Purpose**: Configures LAPIC timer to generate periodic interrupts for:
- Task scheduling (preemption)
- Time measurement
- Timeout handling

#### Phase 6: Framebuffer Acquisition

```rust
let fb_response = FRAMEBUFFER_REQ
    .get_response()
    .expect("No framebuffer reply");
```

**Purpose**: Retrieves framebuffer information from Limine bootloader:
- Physical address of framebuffer
- Width, height, pitch (bytes per line)
- Bits per pixel (BPP)
- Pixel format (typically BGRA)

**Limine Protocol**: Uses Limine's request/response mechanism for bootloader communication

#### Phase 7: Memory Map Processing

```rust
let mmap_response = MMAP_REQ.get_response().expect("No memory map response");
let entries = mmap_response.entries();
```

**Purpose**: Retrieves system memory map from bootloader, categorizing memory regions:
- **Usable**: Available for kernel allocation
- **Reserved**: BIOS, ACPI, MMIO regions
- **Bootloader Reclaimable**: Bootloader code/data (can be reclaimed after boot)
- **Kernel/Modules**: Kernel and loaded module regions

#### Phase 8: Page Table Initialization

```rust
let phys_mem_offset = VirtAddr::new(0xffff_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };
```

**Purpose**: Initializes virtual memory management with offset page table approach:
- Maps all physical memory at a fixed virtual offset
- Allows easy physical to virtual address translation
- Required for heap allocation and memory safety

**Memory Layout**:
```
0xFFFF_8000_0000_0000: Physical memory mapping base
0x4444_4444_0000:      Heap region start
```

#### Phase 9: Frame Allocator Setup

```rust
let mut frame_count = 0;
for region in entries.iter().filter(/* USABLE */) {
    // Preallocate all usable frames before heap mapping
    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        if frame_count < memory::heap::MAX_BOOT_FRAMES {
            unsafe {
                memory::heap::BOOT_FRAMES[frame_count] = Some(frame);
            }
            frame_count += 1;
        }
    }
}
let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);
```

**Purpose**: Creates a physical frame allocator from usable memory regions:
- Identifies all usable 4KB pages
- Stores them in static array (pre-heap allocation)
- Provides frame allocation interface for page table mapping

**Bootstrap Problem**: Can't use heap allocator until we've mapped heap memory, which requires frame allocation. Solution: use static array for boot-time frame allocation.

#### Phase 10: Heap Initialization

```rust
init_heap(&mut mapper, &mut frame_alloc);
```

**Purpose**: Maps and initializes kernel heap:
1. Allocates page table entries for heap region
2. Maps virtual pages to physical frames
3. Initializes linked list allocator
4. Enables Rust's `alloc` crate functionality (Vec, Box, String, etc.)

**Heap Specifications**:
- **Virtual Address**: `0x4444_4444_0000`
- **Size**: 1 MB (configurable)
- **Allocator**: `linked_list_allocator` crate

#### Phase 11: Graphics Output

```rust
if let Some(fb) = fb_response.framebuffers().next() {
    fill_screen_blue(&fb);
    draw_memory_map(&fb, mmap_response.entries());
}
```

**Purpose**: Visual boot confirmation and memory visualization:
- Fills screen with blue (classic boot success indicator)
- Draws memory map as colored bars at bottom:
  - **Green**: Usable memory
  - **Yellow**: Bootloader reclaimable
  - **Gray**: Reserved/other

#### Phase 12: Console Initialization

```rust
let fb = fb_response.framebuffers().next().expect("No framebuffer");
init_console(fb.addr(), fb.width() as usize, fb.height() as usize, fb.pitch() as usize);
```

**Purpose**: Initializes text console on framebuffer:
- 8x16 pixel font rendering
- Scrolling support
- Global `fb_println!` macro support

#### Phase 13: Scheduler Initialization

```rust
Scheduler::init_global();
```

**Purpose**: Initializes the global task scheduler (currently minimal):
- Creates global scheduler instance
- Prepares task management infrastructure
- Future: will support preemptive multitasking

#### Phase 14: Main Loop

```rust
loop {
    hlt()
}
```

**Purpose**: Kernel main loop:
- Uses `hlt` instruction to halt CPU until next interrupt
- Saves power compared to busy-waiting
- Wakes on timer ticks, keyboard input, etc.

**Future**: Will be replaced by scheduler that switches between tasks

## Static Variables

### Limine Protocol Requests

```rust
static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
```

**Purpose**: Communicate with Limine bootloader via request/response protocol
- Requests are placed in specific ELF sections
- Bootloader populates responses before jumping to kernel
- Provides framebuffer, memory map, RSDP, modules, etc.

### Global Scheduler

```rust
static SCHEDULER: TaskManager = TaskManager::new();
```

**Purpose**: Global task management instance (currently unused in main loop)

## Panic Handling

```rust
#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    serial_println!("[KERNEL PANIC]");
    if let Some(loc) = info.location() {
        serial_println!("Location: {}:{}", loc.file(), loc.line());
    }
    halt_loop();
}
```

**Purpose**: Custom panic handler for `no_std` environment:
- Outputs panic information to serial console
- Shows file and line number of panic
- Enters infinite halt loop (alternative: triple fault reboot)

## Dependencies

### Internal Crates
- **apic**: APIC/I/O APIC/timer management
- **graphics**: Framebuffer rendering and console
- **hal**: Hardware abstraction (serial, I/O ports, CPU control)
- **idt**: Interrupt descriptor table and exception handlers
- **memory**: Page tables, heap, frame allocation
- **util**: Panic handling and utility functions
- **task**: Task management and scheduling (proto)

### External Crates
- **limine** (0.5.0): Bootloader protocol
- **x86_64** (0.15.2): x86_64 architecture abstractions
- **spin** (0.10.0): Spinlock synchronization primitives
- **alloc**: Rust standard allocation library (no_std compatible)

## Configuration

### Cargo.toml

```toml
[package]
name = "kernel"
version = "0.1.0"
edition = "2024"

[profile.release]
panic = "abort"

[profile.dev]
panic = "abort"
```

**Key Settings**:
- `edition = "2024"`: Uses latest Rust edition
- `panic = "abort"`: Panics immediately halt (no unwinding in kernel)

### Linker Script

See `linker.ld` for memory layout and section definitions:
- Loads at high virtual addresses
- Defines `.text`, `.rodata`, `.data`, `.bss` sections
- Limine protocol sections (`.limine_reqs`)
- Stack guard pages

## Build Process

The kernel is built as a freestanding binary:

```bash
cargo build --target x86_64-unknown-none
```

### Target Specification

Uses custom target: `x86_64-unknown-none`
- No operating system
- No standard library
- Bare metal execution
- Static linking only

### Output Binary

Generated at: `target/x86_64-unknown-none/debug/kernel` or `/release/kernel`

## Boot Process Flow

```
BIOS/UEFI
    ↓
Limine Bootloader
    ↓ (loads kernel ELF)
Limine Protocol Setup
    ↓ (populates requests)
Jump to kernel _start()
    ↓
Serial Init
    ↓
APIC Setup
    ↓
IDT Setup
    ↓
Enable Interrupts
    ↓
Timer Init
    ↓
Acquire Framebuffer
    ↓
Acquire Memory Map
    ↓
Initialize Paging
    ↓
Setup Frame Allocator
    ↓
Map & Initialize Heap
    ↓
Blue Screen + Memory Viz
    ↓
Console Init
    ↓
Scheduler Init
    ↓
Main Loop (HLT)
```

## Debugging

### Serial Output

All kernel messages are output to COM1 (0x3F8):

```bash
# QEMU serial output to console
qemu-system-x86_64 -serial stdio ...

# QEMU serial output to file
qemu-system-x86_64 -serial file:serial.log ...
```

### GDB Debugging

```bash
# Start QEMU with GDB server
qemu-system-x86_64 -s -S ...

# In another terminal
gdb target/x86_64-unknown-none/debug/kernel
(gdb) target remote :1234
(gdb) break _start
(gdb) continue
```

### Common Issues

#### Blue Screen Not Appearing
- Check serial output for panic messages
- Verify framebuffer request is satisfied
- Check QEMU graphics backend

#### Immediate Reboot
- IDT not properly initialized
- Page fault during early boot
- Check serial output for last message

#### Hang at Specific Point
- Interrupt storm (IDT issue)
- Infinite loop without `hlt`
- Deadlock in spinlock
- Check serial output for last message

## Future Development

### Planned Features

1. **Full Task Scheduler**: Preemptive multitasking with round-robin or CFS
2. **SMP Support**: Multi-processor initialization and coordination
3. **User Mode**: Ring 3 execution with syscall interface
4. **Virtual File System**: Abstract filesystem layer
5. **Device Drivers**: PCI, USB, SATA, NVMe
6. **Network Stack**: TCP/IP implementation
7. **Shell**: Interactive command-line interface

### Architectural Improvements

1. **Proper Error Handling**: Replace panics with Result types
2. **Logging Framework**: Structured logging with levels
3. **Configuration System**: Compile-time and runtime configuration
4. **Module Loading**: Dynamic kernel module support
5. **Security**: ASLR, stack canaries, DEP/NX, SMEP/SMAP

## Testing

Currently, testing is manual via QEMU:

```bash
make run
```

**Future**: Automated testing with:
- Unit tests (where possible in no_std)
- Integration tests
- CI/CD pipeline
- Fuzzing for parser/input handlers

## Contributing

When modifying the kernel module:

1. **Maintain Initialization Order**: Dependencies must be initialized in correct sequence
2. **Serial Debug Output**: Add `serial_println!` at key points for debugging
3. **Error Handling**: Check all bootloader responses for None
4. **Documentation**: Update this README for significant changes
5. **Testing**: Test on both QEMU and real hardware where possible

## License

GPL-3.0 (see LICENSE file in repository root)

## References

- [Limine Boot Protocol](https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md)
- [Intel 64 and IA-32 Architectures Software Developer's Manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev Wiki](https://wiki.osdev.org/)
- [x86_64 crate documentation](https://docs.rs/x86_64/)
