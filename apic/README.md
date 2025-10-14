# APIC (Advanced Programmable Interrupt Controller) Module

## Overview

The APIC module provides a modern interrupt handling infrastructure for x86_64 systems, replacing the legacy 8259 PIC (Programmable Interrupt Controller). This module manages three critical components: the Local APIC (LAPIC) for per-CPU interrupt handling, the I/O APIC for routing external hardware interrupts, and the LAPIC timer for precise timing and scheduling.

## Architecture

### Components

1. **Local APIC (LAPIC)**: Per-CPU interrupt controller
2. **I/O APIC**: Central hub for routing hardware IRQs to CPUs
3. **LAPIC Timer**: Programmable per-CPU timer

### Why APIC Over Legacy PIC?

The 8259 PIC has several limitations that make it unsuitable for modern operating systems:

| Feature | Legacy 8259 PIC | Modern APIC |
|---------|----------------|-------------|
| CPU Support | Single CPU only | Multiple CPUs (SMP ready) |
| Interrupt Lines | 15 IRQs (cascaded) | 24+ IRQs via I/O APIC |
| Priority Levels | Fixed priority | Dynamic priority |
| Interrupt Routing | Fixed routing | Flexible per-IRQ routing |
| Timer Resolution | Limited | High precision |
| Performance | Slow (ISA bus) | Fast (system bus) |
| EOI (End of Interrupt) | Per-PIC required | Single LAPIC EOI |

## Module Structure

```
apic/
├── src/
│   ├── lib.rs          # LAPIC management, PIC disable
│   ├── ioapic.rs       # I/O APIC IRQ routing
│   └── timer.rs        # LAPIC timer configuration
└── Cargo.toml
```

## Local APIC (lib.rs)

### Memory-Mapped I/O Base

```rust
const APIC_BASE: u64 = 0xFEE00000;
```

The Local APIC is accessed via memory-mapped I/O at a fixed physical address. All LAPIC registers are 32-bit aligned at 16-byte boundaries.

### Register Access

```rust
fn lapic_reg(offset: u32) -> *mut u32 {
    (APIC_BASE + offset as u64) as *mut u32
}
```

**Purpose**: Calculates the pointer to a specific LAPIC register.

**Safety**: All LAPIC register accesses must use `read_volatile()` and `write_volatile()` to prevent compiler optimizations that could break MMIO semantics.

### Key LAPIC Registers

| Offset | Register | Purpose |
|--------|----------|---------|
| 0xF0 | SVR (Spurious Interrupt Vector Register) | Enable/disable LAPIC |
| 0xB0 | EOI (End of Interrupt) | Signal interrupt completion |
| 0x320 | LVT Timer Register | Configure timer interrupt |
| 0x380 | Initial Count Register | Set timer period |
| 0x3E0 | Divide Configuration Register | Set timer divider |

### Disabling Legacy PIC

```rust
pub unsafe fn disable_pic()
```

**Purpose**: Properly disables the legacy 8259 PIC to prevent conflicts with APIC.

**Procedure**:
1. **Initialization Command Word 1 (ICW1)**: Start initialization sequence (0x11)
2. **Initialization Command Word 2 (ICW2)**: Remap IRQs to vectors 32-47
   - Master PIC (IRQ 0-7) → Vectors 32-39
   - Slave PIC (IRQ 8-15) → Vectors 40-47
3. **Initialization Command Word 3 (ICW3)**: Configure cascading
   - Master: IRQ2 is slave
   - Slave: Cascade identity = 2
4. **Initialization Command Word 4 (ICW4)**: Set 8086 mode (0x01)
5. **Mask All Interrupts**: Write 0xFF to both data ports

**Why Remap Before Disabling?**
The PIC defaults to vectors 0-15, which conflict with CPU exceptions. Even when disabling, we remap to safe vectors to prevent spurious interrupts from causing confusion.

**Port Addresses**:
- Master PIC Command: 0x20
- Master PIC Data: 0x21
- Slave PIC Command: 0xA0
- Slave PIC Data: 0xA1

### Enabling APIC

```rust
pub unsafe fn enable()
```

**Purpose**: Enables the Local APIC through MSR (Model Specific Register) and local enable bit.

**Procedure**:

#### Step 1: Enable APIC via IA32_APIC_BASE MSR (0x1B)

```rust
// Read current MSR value
let mut apic_base: u64;
core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, lateout("eax") apic_base, ...);

// Set bit 11 (APIC Global Enable)
if (apic_base & (1 << 11)) == 0 {
    apic_base |= 1 << 11;
    // Write back to MSR
    core::arch::asm!("wrmsr", in("ecx") 0x1Bu32, ...);
}
```

**MSR Layout (IA32_APIC_BASE)**:
- Bits 0-7: Reserved
- Bits 8-11: BSP flag (bit 8), Reserved, Reserved, Global Enable (bit 11)
- Bits 12-35: APIC Base address (4KB aligned)
- Bits 36-63: Reserved

#### Step 2: Enable LAPIC via Spurious Interrupt Vector Register

```rust
let svr = lapic_reg(0xF0);
let val = svr.read_volatile() | 0x100; // Set bit 8
svr.write_volatile(val);
```

**SVR Register (0xF0)**:
- Bits 0-7: Spurious vector number
- Bit 8: APIC Software Enable (must be 1)
- Bit 9: Focus Processor Checking (legacy, should be 0)
- Bits 10-11: Reserved
- Bit 12: EOI Broadcast Suppression

**Two-Level Enable**: Both MSR bit 11 and SVR bit 8 must be set for LAPIC to function.

### LAPIC Timer Configuration

```rust
pub unsafe fn set_timer(vector: u8, divide: u32, initial_count: u32)
```

**Purpose**: Configures the LAPIC timer for periodic interrupts.

**Parameters**:
- `vector`: Interrupt vector number (e.g., 0x31 = 49 decimal)
- `divide`: Divider configuration (0x3 = divide by 16)
- `initial_count`: Timer period in bus cycles

**Register Configuration**:

1. **Divide Configuration Register (0x3E0)**:
   ```rust
   lapic_reg(0x3E0).write_volatile(divide);
   ```
   - Determines timer frequency divider
   - Value 0x3 = divide bus clock by 16

2. **LVT Timer Register (0x320)**:
   ```rust
   lapic_reg(0x320).write_volatile((vector as u32) | 0x20000);
   ```
   - Bits 0-7: Vector number
   - Bit 17 (0x20000): Timer mode (0 = one-shot, 1 = periodic)
   - Bit 16: Mask (0 = not masked)

3. **Initial Count Register (0x380)**:
   ```rust
   lapic_reg(0x380).write_volatile(initial_count);
   ```
   - Timer countdown value
   - Counts down to zero, then reloads (in periodic mode)

**Timer Frequency Calculation**:
```
Interrupt Frequency = Bus Clock / (Divider × Initial Count)
Example: 1 GHz bus / (16 × 100,000) ≈ 625 Hz (1.6 ms period)
```

### End of Interrupt (EOI)

```rust
pub unsafe fn send_eoi()
```

**Purpose**: Signals to LAPIC that the current interrupt has been handled.

**Critical**: Must be called at the end of every interrupt handler, or the LAPIC will not deliver further interrupts at the same or lower priority.

**Implementation**:
```rust
let eoi = lapic_reg(0xB0);
eoi.write_volatile(0);
```

Writing any value (typically 0) to the EOI register signals completion. The written value is ignored.

## I/O APIC (ioapic.rs)

### Memory-Mapped I/O Base

```rust
const IOAPIC_BASE: u64 = 0xFEC00000;
```

The I/O APIC is accessed via MMIO at a fixed address. Unlike LAPIC, I/O APIC uses an indirect access model with two registers:
- **IOREGSEL (0x00)**: Register selector
- **IOWIN (0x10)**: Register data window

### Indirect Register Access

```rust
fn ioapic_reg(offset: u32) -> *mut u32 {
    (IOAPIC_BASE + offset as u64) as *mut u32
}

unsafe fn ioapic_read(reg: u32) -> u32 {
    ioapic_reg(0x00).write_volatile(reg);
    ioapic_reg(0x10).read_volatile()
}

unsafe fn ioapic_write(reg: u32, value: u32) {
    ioapic_reg(0x00).write_volatile(reg);
    ioapic_reg(0x10).write_volatile(value);
}
```

**Procedure**:
1. Write register index to IOREGSEL (offset 0x00)
2. Read/write data via IOWIN (offset 0x10)

### IRQ Routing

```rust
pub unsafe fn map_irq(irq: u8, vector: u8)
```

**Purpose**: Routes a hardware IRQ to a specific interrupt vector.

**Implementation**:
```rust
let reg = 0x10 + (irq as u32 * 2);
ioapic_write(reg, vector as u32);
ioapic_write(reg + 1, 0);
```

**Redirection Table Entry Format**:

Each IRQ has a 64-bit redirection entry split across two 32-bit registers:

**Lower 32 bits (reg)**: Configuration
- Bits 0-7: Vector number
- Bits 8-10: Delivery mode (000 = Fixed)
- Bit 11: Destination mode (0 = Physical)
- Bit 12: Delivery status (read-only)
- Bit 13: Polarity (0 = Active high)
- Bit 14: Remote IRR (read-only)
- Bit 15: Trigger mode (0 = Edge)
- Bit 16: Mask (0 = Not masked)

**Upper 32 bits (reg + 1)**: Destination
- Bits 24-31: Destination CPU (APIC ID)

### Initialization

```rust
pub unsafe fn init_ioapic()
```

**Purpose**: Initializes I/O APIC by routing essential IRQs:

```rust
map_irq(1, 33);  // Keyboard (IRQ1) → Vector 33
map_irq(0, 32);  // Timer (IRQ0) → Vector 32
```

**Standard IRQ Assignments**:
- IRQ 0: PIT Timer (usually disabled in favor of LAPIC timer)
- IRQ 1: Keyboard (PS/2)
- IRQ 2: Cascade from slave PIC (unused in APIC mode)
- IRQ 3-15: Various hardware devices

**Future Expansion**: As more drivers are added, additional IRQs will be routed here.

## LAPIC Timer (timer.rs)

### Configuration Constants

```rust
pub const TIMER_VECTOR: u8 = 0x31;              // Interrupt vector 49
pub const TIMER_DIVIDE_CONFIG: u32 = 0x3;       // Divide by 16
pub const TIMER_INITIAL_COUNT: u32 = 100_000;   // Countdown value
```

### Timer Interrupt Handler

```rust
extern "x86-interrupt" fn timer_interrupt(_stack_frame: InterruptStackFrame)
```

**Purpose**: Handles LAPIC timer interrupts for timekeeping and scheduling.

**Implementation**:
```rust
unsafe {
    TICKS += 1;
    send_eoi();  // Signal interrupt completion
}
```

**Tick Counter**:
```rust
static mut TICKS: u64 = 0;

pub fn ticks() -> u64 {
    unsafe { TICKS }
}
```

**Usage**: Provides a monotonically increasing counter for:
- Uptime measurement
- Timeout implementation
- Scheduling quantum tracking

### Two-Phase Initialization

The timer uses a two-phase initialization to avoid race conditions:

#### Phase 1: Register Handler (Before IDT Load)

```rust
pub unsafe fn register_handler()
```

**Purpose**: Registers the timer interrupt handler with the IDT module.

**Called**: Before `idt::init_idt()` in kernel initialization

**Implementation**:
```rust
idt::register_interrupt_handler(TIMER_VECTOR, timer_interrupt);
```

#### Phase 2: Initialize Hardware (After Interrupts Enabled)

```rust
pub unsafe fn init_hardware()
```

**Purpose**: Configures LAPIC timer hardware registers.

**Called**: After `x86_64::instructions::interrupts::enable()` in kernel initialization

**Implementation**:
```rust
// Divide Configuration Register
lapic_reg(0x3E0).write_volatile(TIMER_DIVIDE_CONFIG);

// LVT Timer Register - periodic mode (bit 17)
lapic_reg(0x320).write_volatile((TIMER_VECTOR as u32) | 0x20000);

// Initial Count Register
lapic_reg(0x380).write_volatile(TIMER_INITIAL_COUNT);

// Enable interrupts globally
hal::cpu::enable_interrupts();
```

**Why Two Phases?**
- IDT must have handler entry before timer starts firing
- Hardware can't be configured until LAPIC is fully enabled
- Interrupts must be enabled before timer generates interrupts

## Dependencies

### Internal Crates

- **hal**: Hardware abstraction for serial output, CPU control
- **idt**: Interrupt descriptor table for registering handlers
- **keyboard**: Integration for timer-based keyboard polling (future)

### External Crates

- **x86_64** (0.15.2): x86_64 abstractions and interrupt stack frame

## Usage Example

### In Kernel Initialization

```rust
unsafe {
    // Phase 1: Disable PIC and enable APIC
    apic::enable();
    
    // Phase 2: Route IRQs through I/O APIC
    apic::ioapic::init_ioapic();
    
    // Phase 3: Register timer handler
    apic::timer::register_handler();
}

// Load IDT
idt::init_idt();

// Enable interrupts
x86_64::instructions::interrupts::enable();

// Phase 4: Start timer hardware
unsafe {
    apic::timer::init_hardware();
}
```

### In Interrupt Handlers

```rust
extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    // Handle keyboard input
    let scancode = read_keyboard_port();
    process_scancode(scancode);
    
    // Signal interrupt completion
    unsafe {
        apic::send_eoi();
    }
}
```

## Memory Layout

### LAPIC MMIO Region

```
Physical: 0xFEE00000 - 0xFEE00FFF (4KB page)
Virtual:  Direct mapped (identity or offset)

Key Registers:
0xFEE00020: Local APIC ID
0xFEE00080: Task Priority Register
0xFEE000B0: EOI Register
0xFEE000F0: Spurious Interrupt Vector
0xFEE00320: LVT Timer Register
0xFEE00380: Timer Initial Count
0xFEE00390: Timer Current Count
0xFEE003E0: Timer Divide Configuration
```

### I/O APIC MMIO Region

```
Physical: 0xFEC00000 - 0xFEC000FF (256 bytes)
Virtual:  Direct mapped (identity or offset)

Registers:
0xFEC00000: IOREGSEL (Register Select)
0xFEC00010: IOWIN (Data Window)
```

## Interrupt Vector Allocation

```
0-31:    CPU Exceptions (reserved by x86_64)
32:      Timer (IRQ0) - currently remapped but not used
33:      Keyboard (IRQ1)
49 (0x31): LAPIC Timer
50-255:  Available for future use
```

## Timing Characteristics

### LAPIC Timer Resolution

With current configuration:
```
Divider: 16
Initial Count: 100,000
Typical Bus Clock: ~1 GHz

Interrupt Period = (16 × 100,000) / 1,000,000,000
                 = 1.6 ms
Interrupt Frequency ≈ 625 Hz
```

**Adjustable**: By changing `TIMER_INITIAL_COUNT`, interrupt frequency can be tuned for different scheduling needs.

### Calibration (Future Enhancement)

Proper timer calibration involves:
1. Read TSC (Time Stamp Counter) before timer start
2. Wait for known number of PIT ticks
3. Read TSC after
4. Calculate bus frequency
5. Adjust LAPIC timer initial count for desired frequency

## Multiprocessor Support (Future)

Current implementation targets single-core systems. For SMP support:

### Per-CPU LAPIC Initialization

```rust
pub unsafe fn init_ap(cpu_id: u8) {
    // Each CPU must initialize its own LAPIC
    enable();
    // Configure per-CPU timer
    set_timer(TIMER_VECTOR, TIMER_DIVIDE_CONFIG, TIMER_INITIAL_COUNT);
}
```

### Inter-Processor Interrupts (IPI)

```rust
pub unsafe fn send_ipi(dest_cpu: u8, vector: u8) {
    // Set destination
    lapic_reg(0x310).write_volatile((dest_cpu as u32) << 24);
    
    // Send IPI
    let icr_low = vector as u32 | (0 << 8) | (0 << 11) | (1 << 14);
    lapic_reg(0x300).write_volatile(icr_low);
}
```

**Use Cases**:
- TLB shootdown (invalidate TLB on other CPUs)
- Scheduler wakeup
- Panic synchronization

## Error Handling

### APIC Not Present

```rust
// Check CPUID for APIC support
let cpuid = CpuId::new();
if !cpuid.get_feature_info().unwrap().has_apic() {
    panic!("APIC not supported by CPU");
}
```

### APIC Base Relocation

```rust
// Some systems may relocate APIC base via MSR
let apic_base_msr = read_msr(0x1B);
let relocated_base = apic_base_msr & 0xFFFF_F000;
```

## Debugging

### Common Issues

#### Interrupts Not Firing

**Symptoms**: Timer handler never called, keyboard unresponsive

**Checks**:
1. APIC enabled in MSR and SVR?
2. IDT handler registered?
3. Global interrupts enabled (IF flag)?
4. Timer initial count non-zero?
5. LVT Timer not masked (bit 16 = 0)?

#### Interrupt Storm

**Symptoms**: System hangs, serial output stops

**Causes**:
- Missing `send_eoi()` in handler
- PIC not properly disabled (dual interrupts)
- IRQ triggered faster than handler completes

#### Spurious Interrupts

**Symptoms**: Unexpected vector 0xFF interrupts

**Cause**: LAPIC generates spurious interrupts in some edge cases

**Solution**: Register spurious interrupt handler (vector 0xFF) that just does `send_eoi()`

### Debugging Tools

#### LAPIC Register Dump

```rust
pub unsafe fn dump_lapic_regs() {
    serial_println!("LAPIC ID: {:08x}", lapic_reg(0x20).read_volatile());
    serial_println!("LAPIC Version: {:08x}", lapic_reg(0x30).read_volatile());
    serial_println!("TPR: {:08x}", lapic_reg(0x80).read_volatile());
    serial_println!("SVR: {:08x}", lapic_reg(0xF0).read_volatile());
    serial_println!("LVT Timer: {:08x}", lapic_reg(0x320).read_volatile());
    serial_println!("Timer Current: {:08x}", lapic_reg(0x390).read_volatile());
}
```

#### I/O APIC Redirection Table Dump

```rust
pub unsafe fn dump_ioapic_redirs() {
    for irq in 0..24 {
        let reg = 0x10 + (irq * 2);
        let low = ioapic_read(reg);
        let high = ioapic_read(reg + 1);
        serial_println!("IRQ {}: {:08x} {:08x}", irq, high, low);
    }
}
```

## Performance Considerations

### Timer Frequency Trade-offs

**Higher Frequency (shorter period)**:
- Pros: More responsive scheduling, better time resolution
- Cons: Higher interrupt overhead, reduced throughput

**Lower Frequency (longer period)**:
- Pros: Lower overhead, better throughput
- Cons: Coarser time resolution, less responsive scheduling

**Typical Values**:
- Linux: 100-1000 Hz (1-10 ms period)
- Windows: ~64 Hz (15.6 ms period)
- Real-time systems: 1000+ Hz

### EOI Optimization

Modern x86_64 supports "EOI Broadcast Suppression" (SVR bit 12), which prevents LAPIC from broadcasting EOI to I/O APIC. This improves performance in systems with many I/O APICs.

## References

- [Intel 64 and IA-32 Architectures Software Developer's Manual, Volume 3A: System Programming Guide, Chapter 10 (APIC)](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev APIC](https://wiki.osdev.org/APIC)
- [OSDev IOAPIC](https://wiki.osdev.org/IOAPIC)
- [MultiProcessor Specification](https://pdos.csail.mit.edu/6.828/2008/readings/ia32/MPspec.pdf)

## License

GPL-3.0 (see LICENSE file in repository root)
