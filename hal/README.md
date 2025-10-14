# HAL (Hardware Abstraction Layer) Module

## Overview

The Hardware Abstraction Layer (HAL) provides a clean, safe interface to low-level hardware operations on x86_64 systems. It abstracts CPU instructions, I/O port operations, and serial communication, allowing higher-level kernel code to interact with hardware without directly using assembly or unsafe operations. The HAL serves as the foundation for all hardware interactions in Serix.

## Architecture

### Components

1. **I/O Port Operations** (`io.rs`): Port-mapped I/O for legacy devices
2. **Serial Communication** (`serial.rs`): COM port driver for debugging
3. **CPU Control** (`cpu.rs`): CPU instruction wrappers (halt, interrupt control)

### Design Philosophy

- **Safety Abstraction**: Provides safe wrappers around inherently unsafe hardware operations
- **Minimal Overhead**: Inline functions with zero-cost abstractions
- **Debug-First**: Serial output is prioritized for early boot debugging
- **Stateless Operations**: No global state except serial port initialization

## Module Structure

```
hal/
├── src/
│   ├── lib.rs      # Module exports and re-exports
│   ├── io.rs       # Port I/O operations
│   ├── serial.rs   # Serial port driver
│   └── cpu.rs      # CPU control functions
└── Cargo.toml
```

## I/O Port Operations (io.rs)

### Overview

x86_64 systems use port-mapped I/O to communicate with legacy devices. Ports are addressed by 16-bit port numbers (0x0000-0xFFFF) and accessed via special `IN` and `OUT` instructions.

### Port Output

```rust
#[inline]
pub unsafe fn outb(port: u16, value: u8)
```

**Purpose**: Writes a byte to an I/O port.

**Assembly Implementation**:
```rust
core::arch::asm!("out dx, al", in("dx") port, in("al") value);
```

**Instruction Breakdown**:
- `out dx, al`: x86 OUT instruction
- `dx` register: Port number (16-bit)
- `al` register: Value to write (8-bit)

**Usage Examples**:
```rust
// Write to serial port data register
unsafe { outb(0x3F8, b'A'); }

// Mask PIC interrupt
unsafe { outb(0x21, 0xFF); }

// Write to CMOS register
unsafe { outb(0x70, 0x00); }
```

**Safety**: Caller must ensure the port number is valid and writing won't cause undefined behavior or hardware damage.

### Port Input

```rust
#[inline]
pub unsafe fn inb(port: u16) -> u8
```

**Purpose**: Reads a byte from an I/O port.

**Assembly Implementation**:
```rust
let value: u8;
core::arch::asm!("in al, dx", out("al") value, in("dx") port);
value
```

**Instruction Breakdown**:
- `in al, dx`: x86 IN instruction
- `al` register: Receives read value (8-bit)
- `dx` register: Port number (16-bit)

**Usage Examples**:
```rust
// Read from serial port line status register
let status = unsafe { inb(0x3FD) };

// Read keyboard scancode
let scancode = unsafe { inb(0x60) };

// Read from CMOS data register
let value = unsafe { inb(0x71) };
```

**Safety**: Caller must ensure the port number is valid and reading won't have side effects.

### Port Access Characteristics

| Port Range | Typical Usage |
|------------|---------------|
| 0x000-0x01F | DMA controller |
| 0x020-0x021 | Master PIC (8259A) |
| 0x040-0x043 | PIT (Programmable Interval Timer) |
| 0x060-0x064 | Keyboard controller |
| 0x070-0x071 | CMOS/RTC |
| 0x0A0-0xA1 | Slave PIC (8259A) |
| 0x0F0-0x0FF | Math coprocessor |
| 0x170-0x177 | Secondary IDE controller |
| 0x1F0-0x1F7 | Primary IDE controller |
| 0x278-0x27F | Parallel port 2 |
| 0x2E8-0x2EF | Serial port 4 (COM4) |
| 0x2F8-0x2FF | Serial port 2 (COM2) |
| 0x378-0x37F | Parallel port 1 |
| 0x3B0-0x3BB | VGA (monochrome) |
| 0x3C0-0x3CF | VGA (color) |
| 0x3D0-0x3DF | VGA (CRT controller) |
| 0x3E8-0x3EF | Serial port 3 (COM3) |
| 0x3F0-0x3F7 | Floppy controller |
| 0x3F8-0x3FF | Serial port 1 (COM1) |

### Performance

Port I/O operations are significantly slower than memory access:
- Memory read/write: ~1-4 cycles
- Port I/O: ~100-1000+ cycles (varies by device)

**Reason**: Port I/O goes through the chipset to reach legacy devices, incurring high latency.

## Serial Communication (serial.rs)

### Overview

The serial port (RS-232 UART) provides a simple, reliable communication channel for kernel debugging. It works immediately after boot, requires no complex initialization, and is universally supported by emulators and hardware.

### Serial Port Architecture

#### COM Port Addresses

```rust
const COM1: u16 = 0x3F8;  // Primary serial port
// COM2: 0x2F8
// COM3: 0x3E8
// COM4: 0x2E8
```

#### Register Offsets

```rust
const DATA_REG: u16 = 0;        // Data register (read/write)
const INT_EN_REG: u16 = 1;      // Interrupt enable register
const FIFO_REG: u16 = 2;        // FIFO control register
const LINE_CTRL_REG: u16 = 3;   // Line control register
const MODEM_CTRL_REG: u16 = 4;  // Modem control register
const LINE_STATUS_REG: u16 = 5; // Line status register
```

**Absolute Addresses for COM1**:
- Data: 0x3F8
- Interrupt Enable: 0x3F9
- FIFO Control: 0x3FA
- Line Control: 0x3FB
- Modem Control: 0x3FC
- Line Status: 0x3FD

### Serial Port Structure

```rust
pub struct SerialPort {
    base: u16,  // Base port address (e.g., 0x3F8 for COM1)
}
```

### Initialization

```rust
pub fn new() -> Self
```

**Purpose**: Creates and initializes COM1 serial port.

**Configuration**: 115200 baud, 8 data bits, no parity, 1 stop bit (8N1)

**Initialization Sequence**:

```rust
unsafe fn init(&self) {
    // 1. Disable all interrupts
    outb(self.base + INT_EN_REG, 0x00);
    
    // 2. Enable DLAB (Divisor Latch Access Bit)
    outb(self.base + LINE_CTRL_REG, 0x80);
    
    // 3. Set divisor to 1 (115200 baud)
    outb(self.base + DATA_REG, 0x01);      // Low byte
    outb(self.base + INT_EN_REG, 0x00);    // High byte
    
    // 4. 8 bits, no parity, one stop bit (8N1)
    outb(self.base + LINE_CTRL_REG, 0x03);
    
    // 5. Enable FIFO, clear, 14-byte threshold
    outb(self.base + FIFO_REG, 0xC7);
    
    // 6. Enable IRQ, RTS/DSR set
    outb(self.base + MODEM_CTRL_REG, 0x0B);
}
```

#### Detailed Initialization Steps

**Step 1: Disable Interrupts**
```rust
outb(base + 1, 0x00);
```
Prevents serial port from generating interrupts during configuration.

**Step 2: Enable DLAB**
```rust
outb(base + 3, 0x80);
```
Line Control Register bit 7 enables access to divisor registers instead of data registers.

**Step 3: Set Baud Rate Divisor**
```rust
outb(base + 0, 0x01);  // Divisor low byte
outb(base + 1, 0x00);  // Divisor high byte
```

**Baud Rate Formula**:
```
Baud Rate = 115200 / Divisor

Examples:
Divisor 1  → 115200 baud
Divisor 2  → 57600 baud
Divisor 3  → 38400 baud
Divisor 6  → 19200 baud
Divisor 12 → 9600 baud
```

**Step 4: Configure Line Parameters**
```rust
outb(base + 3, 0x03);
```

**Line Control Register (0x03 = 0b00000011)**:
- Bits 0-1: 11 = 8 data bits
- Bit 2: 0 = 1 stop bit
- Bits 3-5: 000 = No parity
- Bit 6: 0 = Break control disabled
- Bit 7: 0 = DLAB disabled

**Step 5: Enable and Configure FIFO**
```rust
outb(base + 2, 0xC7);
```

**FIFO Control Register (0xC7 = 0b11000111)**:
- Bit 0: 1 = Enable FIFO
- Bit 1: 1 = Clear receive FIFO
- Bit 2: 1 = Clear transmit FIFO
- Bit 3: 0 = DMA mode disabled
- Bits 6-7: 11 = 14-byte interrupt threshold

**FIFO Benefits**:
- Buffers up to 16 bytes
- Reduces interrupt frequency
- Improves throughput

**Step 6: Configure Modem Control**
```rust
outb(base + 4, 0x0B);
```

**Modem Control Register (0x0B = 0b00001011)**:
- Bit 0: 1 = Data Terminal Ready (DTR)
- Bit 1: 1 = Request To Send (RTS)
- Bit 2: 0 = Auxiliary output 1
- Bit 3: 1 = Auxiliary output 2 (enables IRQ)
- Bit 4: 0 = Loopback mode disabled

### Transmission

```rust
pub fn write_byte(&self, byte: u8)
```

**Purpose**: Writes a single byte to the serial port.

**Implementation**:
```rust
// Wait for transmit buffer to be empty
while !self.is_transmit_empty() {
    core::hint::spin_loop();
}

// Write byte to data register
unsafe {
    outb(self.base + DATA_REG, byte);
}
```

#### Transmit Ready Check

```rust
fn is_transmit_empty(&self) -> bool
```

**Purpose**: Checks if the transmit buffer has space.

**Implementation**:
```rust
unsafe {
    inb(self.base + LINE_STATUS_REG) & 0x20 != 0
}
```

**Line Status Register Bit 5 (0x20)**:
- 0 = Transmit buffer full (busy)
- 1 = Transmit buffer empty (ready)

**Why Busy-Wait?**
The serial port is slow (~100 microseconds per byte at 115200 baud), but the wait is predictable and short. For early boot debugging, simplicity trumps complexity.

### String Output

```rust
pub fn write_str(&self, s: &str)
```

**Purpose**: Writes a string to the serial port.

**Implementation**:
```rust
for byte in s.bytes() {
    self.write_byte(byte);
}
```

**Character Encoding**: Assumes ASCII/UTF-8. Non-ASCII characters may render incorrectly on terminal.

### Global Serial Port

#### Lazy Initialization

```rust
use spin::Once;
static SERIAL_PORT: Once<Mutex<SerialPort>> = Once::new();
```

**Once Initialization**: Ensures serial port is initialized exactly once, even in multithreaded environments.

#### Initialization Function

```rust
pub fn init_serial()
```

**Purpose**: Initializes the global serial port instance.

**Implementation**:
```rust
SERIAL_PORT.call_once(|| Mutex::new(SerialPort::new()));
```

**Thread Safety**: `Once::call_once` guarantees:
- Initialization runs exactly once
- Subsequent calls return immediately
- Concurrent calls block until initialization completes

#### Global Print Function

```rust
pub fn serial_print(s: &str)
```

**Purpose**: Prints a string to the global serial port (thread-safe).

**Implementation**:
```rust
if let Some(serial) = SERIAL_PORT.get() {
    let port = serial.lock();
    port.write_str(s);
}
```

**Lock Behavior**: Spinlock blocks until serial port is available. In single-threaded boot code, this is instantaneous.

### Macros

#### `serial_print!`

```rust
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_serial_print(format_args!($($arg)*))
    };
}
```

**Purpose**: Formatted printing to serial port (no newline).

**Usage**:
```rust
serial_print!("Value: ");
serial_print!("{:#x}", 0xDEADBEEF);
```

#### `serial_println!`

```rust
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => {
        $crate::serial_print!("{}\n", format_args!($($arg)*))
    };
}
```

**Purpose**: Formatted printing to serial port with newline.

**Usage**:
```rust
serial_println!("Kernel starting...");
serial_println!("Memory: {} bytes", mem_size);
serial_println!();  // Blank line
```

#### Formatting Helper

```rust
pub fn _serial_print(args: core::fmt::Arguments)
```

**Purpose**: Internal function that handles `core::fmt::Arguments` formatting.

**Implementation**:
```rust
use core::fmt::Write;

struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        serial_print(s);
        Ok(())
    }
}

SerialWriter.write_fmt(args).ok();
```

**Design**: Implements `Write` trait to leverage Rust's formatting machinery.

## CPU Control (cpu.rs)

### CPU Halt

```rust
#[inline(always)]
pub fn halt()
```

**Purpose**: Halts the CPU until the next interrupt.

**Implementation**:
```rust
use x86_64::instructions::hlt;
hlt();
```

**Assembly**: Executes `HLT` instruction.

**Behavior**:
- CPU enters low-power state
- Wakes on interrupt (timer, keyboard, etc.)
- Returns to next instruction after interrupt handler completes

**Use Case**: Kernel main loop to save power instead of busy-waiting.

### Interrupt Control

#### Enable Interrupts

```rust
#[inline(always)]
pub fn enable_interrupts()
```

**Purpose**: Sets the interrupt flag (IF) in RFLAGS register.

**Implementation**:
```rust
use x86_64::instructions::interrupts;
interrupts::enable();
```

**Assembly**: Executes `STI` instruction.

**Effect**: CPU will respond to maskable hardware interrupts.

**Safety**: Only safe after IDT is loaded with proper handlers.

#### Disable Interrupts

```rust
#[inline(always)]
pub fn disable_interrupts()
```

**Purpose**: Clears the interrupt flag (IF) in RFLAGS register.

**Implementation**:
```rust
use x86_64::instructions::interrupts;
interrupts::disable();
```

**Assembly**: Executes `CLI` instruction.

**Effect**: CPU ignores maskable hardware interrupts.

**Use Cases**:
- Critical sections requiring atomicity
- Preventing interrupt handlers from running during sensitive operations
- Synchronization primitives

**Warning**: Interrupts must be re-enabled promptly or the system will become unresponsive to hardware events.

## Usage Examples

### Early Boot Debugging

```rust
// Initialize serial port first thing
hal::init_serial();
hal::serial_println!("Kernel entry point reached");

// Continue with boot process...
hal::serial_println!("Initializing APIC...");
apic::enable();

hal::serial_println!("Loading IDT...");
idt::init_idt();
```

### Port I/O Operations

```rust
use hal::{inb, outb};

// Read keyboard scancode
let scancode = unsafe { inb(0x60) };

// Acknowledge interrupt to PIC
unsafe {
    outb(0x20, 0x20);  // Send EOI to master PIC
}

// Reset PS/2 keyboard
unsafe {
    outb(0x60, 0xFF);
}
```

### CPU State Management

```rust
use hal::cpu;

// Critical section with interrupts disabled
cpu::disable_interrupts();
// ... critical code ...
cpu::enable_interrupts();

// Main kernel loop
loop {
    cpu::halt();  // Sleep until interrupt
}
```

## Performance Considerations

### Inline Functions

All HAL functions are marked `#[inline]` or `#[inline(always)]`:

**Benefits**:
- Zero function call overhead
- Enables compiler optimizations
- Critical for hot paths (interrupt handlers)

**Trade-off**: Increased code size at call sites (usually negligible for small functions).

### Serial Output Performance

At 115200 baud:
- Bit time: ~8.68 μs
- Byte time (10 bits): ~86.8 μs
- String (20 chars): ~1.74 ms

**Recommendation**: Use serial output sparingly in performance-critical code paths or interrupt handlers.

## Thread Safety

### Serial Port

The global serial port uses a spinlock mutex:

```rust
static SERIAL_PORT: Once<Mutex<SerialPort>>;
```

**Thread Safety**:
- ✅ Multiple threads can safely print
- ⚠️ Deadlock possible if interrupt fires while holding lock

**Solution**: Disable interrupts around serial operations in interrupt-sensitive contexts:

```rust
x86_64::instructions::interrupts::without_interrupts(|| {
    serial_println!("Critical message");
});
```

### I/O Port Operations

Port I/O operations are inherently atomic at the hardware level, but:
- Not protected by locks
- Caller must ensure correct sequencing
- Multiple threads accessing same port must coordinate

## Safety Considerations

### `unsafe` Functions

Most HAL functions are marked `unsafe`:

**Rationale**:
- Direct hardware access has no safety guarantees
- Incorrect port access can cause undefined behavior
- Some operations can damage hardware (rare, but possible)

**Caller Responsibilities**:
- Ensure port numbers are correct
- Understand hardware side effects
- Maintain proper initialization order
- Don't cause race conditions on shared hardware

### Common Pitfalls

1. **Reading from Write-Only Ports**: May return garbage or hang
2. **Writing to Read-Only Ports**: May be ignored or cause errors
3. **Port Access Order**: Some devices require specific sequencing
4. **Interrupt State**: Disabling interrupts for too long causes missed events

## Debugging

### Serial Output Not Working

**Checks**:
1. `init_serial()` called?
2. QEMU/hardware has serial port connected?
3. Baud rate correct on receiving end?
4. Correct COM port configured?

**QEMU Serial Redirection**:
```bash
# To stdout
qemu-system-x86_64 -serial stdio ...

# To file
qemu-system-x86_64 -serial file:serial.log ...

# To TCP
qemu-system-x86_64 -serial tcp::4444,server,nowait ...
```

### Port I/O Issues

**Debugging Technique**: Log port operations:
```rust
unsafe fn debug_outb(port: u16, value: u8) {
    serial_println!("OUT 0x{:03X} <- 0x{:02X}", port, value);
    outb(port, value);
}
```

## Future Enhancements

### Extended Port Operations

```rust
pub unsafe fn outw(port: u16, value: u16);  // 16-bit output
pub unsafe fn outl(port: u16, value: u32);  // 32-bit output
pub unsafe fn inw(port: u16) -> u16;        // 16-bit input
pub unsafe fn inl(port: u16) -> u32;        // 32-bit input
```

### Advanced Serial Features

- Multiple COM port support
- Hardware flow control (RTS/CTS)
- Break signal handling
- Loopback testing
- DMA mode
- Interrupt-driven transmission

### CPU Feature Detection

```rust
pub fn has_sse() -> bool;
pub fn has_avx() -> bool;
pub fn has_popcnt() -> bool;
```

### Performance Counters

```rust
pub fn rdtsc() -> u64;  // Read time-stamp counter
pub fn rdpmc(counter: u32) -> u64;  // Read performance counter
```

## Dependencies

### Internal Crates

None (HAL is the lowest-level module)

### External Crates

- **x86_64** (0.15.2): Architecture abstractions for CPU control
- **spin** (0.10.0): Spinlock mutex for serial port

## Configuration

### Cargo.toml

```toml
[package]
name = "hal"
version = "0.1.0"
edition = "2024"

[dependencies]
x86_64 = "0.15.2"
spin = "0.10.0"
```

## References

- [OSDev - Serial Ports](https://wiki.osdev.org/Serial_Ports)
- [OSDev - I/O Ports](https://wiki.osdev.org/I/O_Ports)
- [Intel 64 and IA-32 Architectures Software Developer's Manual, Volume 1: Basic Architecture, Chapter 16 (I/O)](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [16550 UART Specification](http://www.ti.com/lit/ds/symlink/pc16550d.pdf)

## License

GPL-3.0 (see LICENSE file in repository root)
