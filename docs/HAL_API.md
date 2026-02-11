================================================================================
Serix Hardware Abstraction Layer (HAL) Documentation
================================================================================

:Author: Serix Kernel Team
:Version: v0.0.5
:Last Updated: 2025-01-XX
:Architecture: x86_64
:Module Path: hal/src/

.. Contents:

   1. Introduction
   2. Hardware Initialization
   3. Serial Console Driver
   4. CPU Control Interface  
   5. I/O Port Operations
   6. CPU Topology Detection
   7. Debugging and Tracing
   8. Future Work

   1. Introduction
================================================================================

The Serix HAL provides low-level interfaces to x86_64 hardware, isolating
platform-specific code from kernel subsystems. This layer exposes safe Rust
abstractions over CPU instructions, legacy I/O ports, and serial communication
devices while maintaining zero-cost performance characteristics.

1.1 Design Principles
-----------------------------------------------------------

- Zero-Cost Abstractions - Inline assembly with no runtime overhead
- Type-Safe Hardware Access - Rust's type system prevents hardware bugs  
- Minimal Unsafe Surface - Unsafe operations clearly marked and isolated
- Direct Hardware Control - No buffering or indirection layers

1.2 Current Features (v0.0.5)
-----------------------------------------------------------

Serial Console (WORKING)::

- COM1 UART 16550 driver fully operational
- Debug output via serial_println! macro
- 115200 baud, 8N1 configuration
- Thread-safe global singleton with spinlock protection

CPU Control (WORKING)::

- Interrupt enable/disable (CLI/STI instructions)
- Halt instruction (HLT) for idle loops
- Basic CPU feature detection via CPUID

I/O Port Access (WORKING)::

- Low-level inb/outb primitives for legacy devices
- 16-bit port address space (0x0000-0xFFFF)

CPU Topology (BASIC)::

- Single-CPU detection
- Placeholder for multi-core enumeration

1.3 Module Organization
-----------------------------------------------------------

::

  hal/
  ├── src/
  │   ├── lib.rs          # Public API exports
  │   ├── cpu.rs          # CPU control (halt, interrupts, CPUID)
  │   ├── io.rs           # Port I/O (inb, outb)
  │   ├── serial.rs       # UART 16550 serial driver
  │   └── topology.rs     # CPU topology detection (stub)
  ├── Cargo.toml
  └── README.md

Dependencies::

  [dependencies]
  x86_64 = "0.15"      # Architecture primitives
  spin = "0.10"        # Spinlock for serial port

1. Hardware Initialization
================================================================================

2.1 Boot Sequence
-----------------------------------------------------------

The HAL is initialized early during kernel boot, immediately after the Limine
bootloader transfers control to the kernel entry point (_start). The following
sequence must be strictly observed:

1. Initialize serial console (hal::init_serial)
2. Disable legacy PIC, enable APIC
3. Load IDT with exception/interrupt handlers  
4. Enable interrupts (STI instruction)
5. Other subsystems may now use serial_println!

Example initialization code::

  // kernel/src/main.rs
  pub extern "C" fn _start() -> ! {
      // Step 1: Serial console (CRITICAL - enables debug output)
      hal::init_serial();
      serial_println!("[HAL] Serial console initialized at COM1 (0x3F8)");

      // Step 2: Disable PIC, enable APIC
      apic::disable_pic();
      apic::enable();
      
      // Step 3: Load IDT  
      idt::init();
      
      // Step 4: Enable interrupts
      x86_64::instructions::interrupts::enable();
      serial_println!("[HAL] Interrupts enabled");
      
      // ... rest of kernel initialization ...
  }

.. warning::
   Never use Vec, Box, String, or call serial_println! before init_serial().
   Doing so will cause a page fault or triple fault.

2.2 Early Boot Constraints
-----------------------------------------------------------

During early boot (before heap initialization), the following restrictions apply:

- No dynamic allocations (Vec, Box, String, format!)
- No serial_println! before init_serial()
- Interrupts disabled until IDT loaded
- Stack is limited (typically 64 KB from bootloader)

The serial console is the ONLY output mechanism available during early boot.
Framebuffer initialization happens much later, after memory management is ready.

2.3 Asciinema Demo
-----------------------------------------------------------

.. asciinema:: recordings/hal-init-sequence.cast

   Shows HAL initialization sequence with serial port setup and first debug
   messages appearing on COM1. Demonstrates checkpoint logging during boot.
    → SerialPort::is_transmit_empty() (polling)
    → outb(COM1 + DATA_REG, byte)
    → I/O port write to 0x3F8
    → UART transmits byte over serial line

```

#### Interrupt Control Path

```

disable_interrupts()
    → cpu::disable_interrupts()
    → x86_64::instructions::interrupts::disable()
    → asm!("cli")
    → CPU clears IF flag in RFLAGS
    → Interrupts masked

```

---



3. Serial Console Driver
================================================================================

The serial console is the primary debug interface during kernel boot and runtime.
It provides reliable output before framebuffer initialization and persists even
if graphics fail. All kernel boot messages, panics, and debug output route
through COM1.

3.1 Hardware: UART 16550
-----------------------------------------------------------

The 16550 Universal Asynchronous Receiver/Transmitter (UART) is a legacy serial
controller present in all x86 systems (real hardware and QEMU/VirtualBox).

Base Address (COM1)::

  0x3F8  (I/O port space)

Register Map (DLAB=0)::

  Offset  | Name | Read             | Write            | Description
  --------|------|------------------|------------------|---------------------------
  +0      | RBR  | Receive Buffer   | Transmit Buffer  | Data register
  +1      | IER  | Int Enable Reg   | Int Enable Reg   | RX/TX interrupt control
  +2      | IIR  | Int ID Reg       | (write-only FCR) | FIFO control/status
  +3      | LCR  | Line Control Reg | Line Control Reg | Word length, parity, DLAB
  +4      | MCR  | Modem Control    | Modem Control    | DTR, RTS, loopback  
  +5      | LSR  | Line Status Reg  | (read-only)      | TX empty, RX ready
  +6      | MSR  | Modem Status Reg | (read-only)      | CTS, DSR, carrier detect
  +7      | SCR  | Scratch Reg      | Scratch Reg      | Test register

When DLAB=1 (Line Control Register bit 7 set)::

  Offset +0 = Divisor Latch Low Byte  (baud rate LSB)
  Offset +1 = Divisor Latch High Byte (baud rate MSB)

3.2 Initialization Sequence
-----------------------------------------------------------

The serial port must be configured before any output is possible. This is done
in hal::serial::SerialPort::new().

Initialization Steps::

  1. Disable all interrupts (IER = 0x00)
  2. Enable DLAB (set LCR bit 7)
  3. Set baud rate divisor:
       Divisor = 115200 / desired_baud
       For 115200 baud: divisor = 1
       Write 0x01 to DLL (offset +0)
       Write 0x00 to DLH (offset +1)
  4. Configure line: 8N1, disable DLAB (LCR = 0x03)
       Bits 0-1: 11 = 8 data bits
       Bit 2:    0  = 1 stop bit
       Bits 3-5: 000 = no parity
       Bit 7:    0  = DLAB off (normal mode)
  5. Enable FIFO with 14-byte threshold (FCR = 0xC7)
       Bit 0:   1 = Enable FIFO
       Bit 1:   1 = Clear RX FIFO
       Bit 2:   1 = Clear TX FIFO  
       Bits 6-7: 11 = 14-byte trigger level
  6. Enable IRQ and set RTS/DSR (MCR = 0x0B)
       Bit 0: 1 = DTR (Data Terminal Ready)
       Bit 1: 1 = RTS (Request To Send)
       Bit 3: 1 = OUT2 (enables IRQ line to APIC)

Code example from hal/src/serial.rs::

  impl SerialPort {
      pub fn new() -> Self {
          let base = COM1;  // 0x3F8
          unsafe {
              // Step 1: Disable interrupts
              outb(base + 1, 0x00);
              
              // Step 2: Enable DLAB
              outb(base + 3, 0x80);
              
              // Step 3: Set divisor = 1 (115200 baud)
              outb(base + 0, 0x01);  // DLL
              outb(base + 1, 0x00);  // DLH
              
              // Step 4: 8N1, disable DLAB
              outb(base + 3, 0x03);
              
              // Step 5: Enable FIFO, clear, 14-byte threshold
              outb(base + 2, 0xC7);
              
              // Step 6: Enable IRQ, RTS/DSR
              outb(base + 4, 0x0B);
          }
          SerialPort { base }
      }
  }

3.3 Transmitting Data
-----------------------------------------------------------

Transmission is polled (no interrupts used). Each byte is sent by:

1. Wait for transmitter to be ready (poll LSR bit 5)
2. Write byte to data register (offset +0)
3. UART serializes byte and sends over TX line

Line Status Register (LSR) Bit 5 - THRE::

  Transmitter Holding Register Empty
  1 = Ready to accept new byte
  0 = Busy, previous byte still transmitting

Typical transmission time::

  At 115200 baud with 8N1 (10 bits per byte):
  Time per byte = 10 / 115200 ≈ 87 microseconds

Code::

  pub fn write_byte(&self, byte: u8) {
      // Poll until ready
      while unsafe { inb(self.base + 5) } & 0x20 == 0 {
          core::hint::spin_loop();  // Yield CPU
      }
      
      // Write byte to TX buffer
      unsafe {
          outb(self.base + 0, byte);
      }
  }

For strings::

  pub fn write_str(&self, s: &str) {
      for byte in s.bytes() {
          self.write_byte(byte);
      }
  }

3.4 Thread-Safe Global Serial Port
-----------------------------------------------------------

The kernel provides a global serial port protected by a spinlock. This allows
safe concurrent access from interrupt handlers and kernel threads.

Global singleton::

  static SERIAL_PORT: Once<Mutex<SerialPort>> = Once::new();
  
  pub fn init_serial() {
      SERIAL_PORT.call_once(|| Mutex::new(SerialPort::new()));
  }

Thread-safe print function::

  pub fn _serial_print(args: fmt::Arguments) {
      use core::fmt::Write;
      
      if let Some(serial) = SERIAL_PORT.get() {
          let mut serial = serial.lock();
          serial.write_fmt(args).unwrap();
      }
  }

Convenience macros::

  #[macro_export]
  macro_rules! serial_print {
      ($($arg:tt)*) => {
          $crate::serial::_serial_print(format_args!($($arg)*));
      };
  }
  
  #[macro_export]
  macro_rules! serial_println {
      () => ($crate::serial_print!("\n"));
      ($($arg:tt)*) => {
          $crate::serial::_serial_print(
              format_args!("{}\n", format_args!($($arg)*))
          );
      };
  }

Usage::

  serial_print!("CPU initialized");
  serial_println!("Memory map has {} entries", count);

3.5 Reading Serial Input (Future)
-----------------------------------------------------------

Currently, the serial driver is transmit-only. Receive functionality is planned
for v0.1.0 and will use interrupts (IRQ 4 via APIC).

Planned RX interrupt handler::

  - Configure IER to enable RX interrupts (bit 0)
  - Register IRQ 4 handler in IDT
  - Handler reads RBR when data available (LSR bit 0 set)
  - Data pushed to ring buffer for kernel debugger

3.6 Debugging Serial Output
-----------------------------------------------------------

QEMU Configuration::

  make run uses: qemu-system-x86_64 ... -serial stdio
  
  This redirects COM1 to QEMU's standard output/input, allowing serial
  messages to appear in the terminal where QEMU was launched.

VirtualBox Configuration::

  VM Settings → Serial Ports → Port 1
  ☑ Enable Serial Port
  Port Number: COM1
  Port Mode: Raw File
  Path/Address: /path/to/serial.log

Real Hardware::

  Connect null modem cable to physical COM port
  Use terminal program: minicom, screen, or PuTTY
  Configuration: 115200 8N1, no flow control

.. asciinema:: recordings/serial-console-demo.cast

   Demonstration of serial output during Serix boot. Shows initialization
   messages, memory map printing, and task scheduling debug output in real-time
   as they are transmitted over COM1 at 115200 baud.

3.7 Port I/O Implementation Details
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The serial driver uses hal::io::inb and hal::io::outb for register access.
These are thin wrappers over x86 IN/OUT instructions.

Assembly for outb (write to I/O port)::

  pub unsafe fn outb(port: u16, value: u8) {
      asm!(
          "out dx, al",
          in("dx") port,
          in("al") value,
          options(nostack, nomem)
      );
  }

Assembly for inb (read from I/O port)::

  pub unsafe fn inb(port: u16) -> u8 {
      let ret: u8;
      asm!(
          "in al, dx",
          out("al") ret,
          in("dx") port,
          options(nostack, nomem)
      );
      ret
  }

Why unsafe::

  - Direct hardware access
  - Wrong port address can crash system
  - Reading from write-only register causes undefined behavior
  - Writing to read-only register may be ignored or cause fault

I/O port address space::

  x86 has separate 16-bit I/O address space (0x0000 - 0xFFFF)
  Distinct from memory address space
  Accessed via IN/OUT instructions, not MOV

Performance characteristics::

  - Latency: 100-1000 CPU cycles per I/O operation
  - I/O instructions serialize the CPU pipeline
  - No caching (always hits actual hardware)
  - Slower than MMIO on modern systems



4. CPU Control Interface
================================================================================

Module: hal::cpu

Provides safe wrappers over x86_64 CPU control instructions (HLT, CLI, STI) and
basic CPU feature detection via CPUID.

4.1 Halt Instruction
-----------------------------------------------------------

Function::

  #[inline(always)]
  pub fn halt()

Halts the CPU until the next interrupt arrives. The processor enters a low-power
state (C1) and will wake on any interrupt, including timer, keyboard, or NMI.

Assembly::

  hlt

Behavior::

  - CPU enters low-power idle state (~1-5% of active power)
  - Wakes on next interrupt (timer, keyboard, NMI, etc.)
  - Returns after interrupt handler completes
  - Wake latency: 1-10 microseconds (CPU-dependent)

Usage in idle loop::

  pub fn idle_loop() -> ! {
      loop {
          x86_64::instructions::interrupts::enable();
          hal::cpu::halt();
          // Interrupt occurred, process it, then loop again
      }
  }

.. warning::
   NEVER call halt() with interrupts disabled! The CPU will deadlock::

     // DEADLOCK - CPU sleeps forever
     x86_64::instructions::interrupts::disable();
     hal::cpu::halt();

   Non-maskable interrupts (NMI) can still wake the CPU, but this is not
   reliable for normal operation.

4.2 Interrupt Control
-----------------------------------------------------------

Enable Interrupts (STI)::

  #[inline(always)]  
  pub fn enable_interrupts()
  
  Sets RFLAGS.IF = 1
  Assembly: sti
  Effect: CPU processes pending and future hardware interrupts

Disable Interrupts (CLI)::

  #[inline(always)]
  pub fn disable_interrupts()
  
  Clears RFLAGS.IF = 0
  Assembly: cli  
  Effect: CPU ignores maskable interrupts (NMI still processed)

Critical Section Pattern::

  // RAII-style (recommended)
  use x86_64::instructions::interrupts;
  
  interrupts::without_interrupts(|| {
      // Interrupts disabled here
      let mut data = SHARED_DATA.lock();
      data.modify();
      // Interrupts re-enabled automatically on scope exit
  });

Manual control (less safe)::

  hal::cpu::disable_interrupts();
  // Critical section - keep short (<100 microseconds)
  let mut data = SHARED_DATA.lock();
  data.modify();
  drop(data);
  hal::cpu::enable_interrupts();

Interrupt State Query::

  use x86_64::instructions::interrupts;
  
  if interrupts::are_enabled() {
      serial_println!("Interrupts enabled");
  }

Maximum Interrupt-Disabled Duration::

  Keep interrupts disabled for <100 microseconds to avoid:
  - Timer drift (LAPIC timer ticks missed)
  - Input device buffer overflow (keyboard, mouse)
  - Network packet loss
  - Real-time deadline violations

4.3 CPU Identification (CPUID)
-----------------------------------------------------------

The CPUID instruction provides CPU vendor, model, and feature information.

Basic usage from x86_64 crate::

  use x86_64::registers::model_specific::Msr;
  use core::arch::x86_64::__cpuid;
  
  // Get vendor string
  let vendor = unsafe { __cpuid(0) };
  let vendor_str = [
      vendor.ebx.to_le_bytes(),
      vendor.edx.to_le_bytes(),
      vendor.ecx.to_le_bytes(),
  ].concat();  // e.g., "GenuineIntel" or "AuthenticAMD"
  
  // Get features (leaf 1)
  let features = unsafe { __cpuid(1) };
  let has_apic = features.edx & (1 << 9) != 0;
  let has_x2apic = features.ecx & (1 << 21) != 0;

Feature flags (CPUID leaf 1, EDX)::

  Bit 0:  FPU   - x87 FPU on-chip
  Bit 4:  TSC   - Time Stamp Counter (RDTSC instruction)
  Bit 5:  MSR   - Model-Specific Registers (RDMSR/WRMSR)
  Bit 6:  PAE   - Physical Address Extension
  Bit 9:  APIC  - On-chip APIC
  Bit 11: SEP   - SYSENTER/SYSEXIT instructions
  Bit 23: MMX   - MMX technology
  Bit 25: SSE   - SSE extensions
  Bit 26: SSE2  - SSE2 extensions

HAL CPU topology module (hal/src/topology.rs) uses CPUID to enumerate cores,
but this is currently a stub returning 1 CPU.


5. I/O Port Operations
================================================================================

Module: hal::io

Low-level x86 I/O port access via IN and OUT instructions. The x86 architecture
has a separate 16-bit I/O address space (65,536 ports) distinct from memory.

5.1 Port Address Space
-----------------------------------------------------------

Address Range::

  0x0000 - 0xFFFF  (65,536 ports)

Common Port Ranges::

  0x0000 - 0x001F   DMA controller
  0x0020 - 0x003F   Programmable Interrupt Controller (PIC)
  0x0040 - 0x005F   Programmable Interval Timer (PIT)
  0x0060 - 0x006F   PS/2 keyboard and mouse  
  0x0070 - 0x007F   CMOS/RTC
  0x00F0 - 0x00FF   Math coprocessor
  0x0170 - 0x017F   Secondary IDE controller
  0x01F0 - 0x01FF   Primary IDE controller
  0x0278 - 0x027F   Parallel port (LPT2)
  0x02F8 - 0x02FF   Serial port (COM2)
  0x0378 - 0x037F   Parallel port (LPT1)
  0x03B0 - 0x03DF   VGA controller
  0x03F0 - 0x03F7   Floppy disk controller
  0x03F8 - 0x03FF   Serial port (COM1)
  0x0CF8 - 0x0CFF   PCI configuration space

5.2 Output to Port (outb)
-----------------------------------------------------------

Function::

  #[inline]
  pub unsafe fn outb(port: u16, value: u8)

Writes an 8-bit value to the specified I/O port.

Parameters::

  port:  16-bit port address (0x0000 - 0xFFFF)
  value: 8-bit value to write

Assembly::

  out dx, al
  
  dx = port address (16-bit)
  al = value to write (8-bit)

Example - Write to serial port::

  use hal::io::outb;
  
  const COM1_DATA: u16 = 0x3F8;
  unsafe {
      outb(COM1_DATA, b'A');
  }

Example - Disable PIC::

  const PIC1_DATA: u16 = 0x21;
  const PIC2_DATA: u16 = 0xA1;
  
  unsafe {
      outb(PIC1_DATA, 0xFF);  // Mask all IRQs on master PIC
      outb(PIC2_DATA, 0xFF);  // Mask all IRQs on slave PIC
  }

Safety Requirements::

  - Port must correspond to writable hardware
  - Value must be valid for target device register
  - Caller must ensure device is in correct state for write
  - No concurrent access to stateful devices

5.3 Input from Port (inb)
-----------------------------------------------------------

Function::

  #[inline]
  pub unsafe fn inb(port: u16) -> u8

Reads an 8-bit value from the specified I/O port.

Parameters::

  port: 16-bit port address (0x0000 - 0xFFFF)

Returns::

  8-bit value read from port

Assembly::

  in al, dx
  
  al = value read (8-bit, output)
  dx = port address (16-bit, input)

Example - Read serial port status::

  use hal::io::inb;
  
  const COM1_LSR: u16 = 0x3FD;  // Line Status Register
  let status = unsafe { inb(COM1_LSR) };
  
  if status & 0x20 != 0 {
      serial_println!("Transmitter ready");
  }

Example - Poll keyboard controller::

  const PS2_STATUS: u16 = 0x64;
  
  loop {
      let status = unsafe { inb(PS2_STATUS) };
      if status & 0x01 != 0 {
          // Data available in output buffer
          break;
      }
  }

Side Effects::

  Reading some ports has side effects:
  - PIC IRR: Reading clears interrupt request
  - RTC registers: Reading advances index register
  - Device FIFOs: Reading consumes data

5.4 Performance Characteristics
-----------------------------------------------------------

I/O port operations are significantly slower than memory access::

  Memory load/store:    ~4 CPU cycles (L1 cache hit)
  I/O port operation:   100-1000 CPU cycles (depends on device)

Why so slow::

  - I/O operations traverse chipset (not cached)
  - LPC/ISA bus is slow (~8 MHz effective)
  - I/O instructions serialize CPU pipeline
  - Device may insert wait states

Serialization::

  IN and OUT instructions are serializing - they force all previous instructions
  to complete before executing, and all subsequent instructions wait until the
  I/O completes. This prevents speculative execution and reordering.

Use MMIO for performance-critical devices::

  Modern devices (PCIe, AHCI, NVMe, network cards) use memory-mapped I/O (MMIO)
  instead of port I/O. MMIO is cacheable and much faster. Legacy devices (serial,
  PIC, PIT, PS/2) still use port I/O.


6. CPU Topology Detection  
================================================================================

Module: hal::topology

Detects number of CPUs, cores, and threads in the system using CPUID and ACPI
tables. Currently this module is a stub returning 1 CPU.

6.1 Current Implementation (v0.0.5)
-----------------------------------------------------------

Function::

  pub fn cpu_count() -> usize

Returns::

  Always returns 1 (single-CPU assumed)

Planned for v0.1.0::

  - Parse ACPI MADT (Multiple APIC Description Table) for CPU list
  - Use CPUID leaf 0x0B (x2APIC topology) for core/thread enumeration
  - Detect hyperthreading vs. physical cores
  - Enumerate NUMA domains

6.2 Multi-Processor Detection (Future)
-----------------------------------------------------------

ACPI MADT provides list of Local APICs::

  Each CPU core has a Local APIC with unique APIC ID
  MADT contains APIC ID → Processor ID mapping
  Enabled cores have flags bit 0 set

CPUID Topology Enumeration::

  Leaf 0x0B provides hierarchical topology:
  - SMT level (hyperthreads sharing a core)
  - Core level (cores sharing a package)
  - Package level (physical CPUs)

Example code (planned)::

  pub struct CpuInfo {
      pub apic_id: u8,
      pub package_id: u8,
      pub core_id: u8,
      pub thread_id: u8,
  }
  
  pub fn enumerate_cpus() -> Vec<CpuInfo> {
      // Parse ACPI MADT
      // For each APIC entry, decode topology with CPUID
      // Return list of CPUs
  }


7. Debugging and Tracing
================================================================================

7.1 Serial Console Debugging
-----------------------------------------------------------

The serial console is the primary debugging interface. It works in all scenarios:
- Early boot (before heap, framebuffer)
- Kernel panics (when graphics may be corrupted)
- Interrupt handlers (when framebuffer is unsafe)
- Real hardware (via null modem cable)

Logging conventions::

  serial_println!("[SUBSYSTEM] message");
  
  Examples:
  serial_println!("[HAL] Serial console initialized");
  serial_println!("[MEMORY] Heap at {:?}, size {}", addr, size);
  serial_println!("[IDT] Loaded IDT with {} entries", count);

Checkpoint logging during boot::

  serial_println!("[CHECKPOINT] About to initialize heap");
  init_heap();
  serial_println!("[CHECKPOINT] Heap initialized successfully");

This helps isolate hangs and triple faults.

7.2 Debug Output Configuration
-----------------------------------------------------------

QEMU::

  make run includes -serial stdio
  Serial output appears in terminal
  Can redirect to file: -serial file:serial.log

VirtualBox::

  VM Settings → Serial Ports → Enable COM1
  Port Mode: Raw File or Host Pipe
  All serial_println! output saved to file

Real Hardware::

  Requires physical COM port (rare on modern systems)
  USB-to-serial adapters work
  Null modem cable to another PC running terminal emulator
  
  Terminal settings: 115200 8N1, no flow control

7.3 Common Debugging Scenarios
-----------------------------------------------------------

Kernel hangs during boot::

  Add serial_println! checkpoints to isolate where it hangs:
  
  serial_println!("[CHECKPOINT 1] Before heap init");
  init_heap();
  serial_println!("[CHECKPOINT 2] After heap init");
  
  If checkpoint 2 never appears, heap init is hanging/faulting.

Triple fault::

  CPU resets, QEMU shows "Triple fault, resetting"
  Usually caused by:
  - Page fault with no IDT loaded
  - Stack overflow (nested exceptions)
  - Invalid page table entry
  
  Use QEMU monitor: -monitor stdio -serial file:serial.log
  Or: -d int,cpu_reset -no-reboot (dumps CPU state on triple fault)

Interrupt handler debugging::

  serial_println! is safe to call from interrupt handlers:
  
  extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
      static mut TICKS: u64 = 0;
      unsafe { TICKS += 1; }
      if unsafe { TICKS % 100 == 0 } {
          serial_println!("[TIMER] {} ticks", unsafe { TICKS });
      }
      apic::send_eoi();
  }

7.4 Asciinema Recordings
-----------------------------------------------------------

This documentation references asciinema recordings demonstrating HAL operation:

recordings/serial-console-demo.cast::

  Shows serial output during full Serix boot sequence
  - Limine bootloader messages
  - HAL initialization (serial port setup)
  - Memory map enumeration
  - Graphics initialization
  - Task scheduler startup
  - Idle loop with periodic timer interrupts
  
  Demonstrates real-time serial output as kernel executes.

recordings/hal-init-sequence.cast::

  Focused view of HAL initialization:
  - Serial port configuration (0x3F8)
  - First debug messages
  - APIC enable sequence
  - IDT loading
  - Interrupt enable (STI)
  
  Shows checkpoint logging used to debug early boot issues.

To record your own::

  asciinema rec serial-output.cast
  make run
  # Boot kernel, serial output captured
  # Press Ctrl+C to stop QEMU
  exit  # Stop recording


8. Future Work
================================================================================

8.1 Planned for v0.1.0
-----------------------------------------------------------

Serial RX Support::

  - Enable UART RX interrupts (IER bit 0)
  - Register IRQ 4 handler
  - Ring buffer for incoming data
  - Integration with kernel debugger (GDB stub)

CPU Topology::

  - Parse ACPI MADT for CPU enumeration
  - CPUID topology leaves (0x0B, 0x1F)
  - Detect hyperthreading vs. physical cores
  - NUMA domain detection

MSR Access::

  - Safe wrappers for RDMSR/WRMSR
  - MSR definitions for common registers (APIC base, EFER, etc.)
  - MSR-based feature detection

8.2 Planned for v0.2.0
-----------------------------------------------------------

PCI Configuration Space::

  - PCI config space access (I/O ports 0xCF8/0xCFC)
  - PCIe MMIO configuration space (ECAM)
  - Device enumeration
  - BAR (Base Address Register) parsing
  - Capability list parsing

DMA Support::

  - ISA DMA controller programming (legacy)
  - Bus master DMA setup
  - Scatter-gather lists
  - IOMMU integration (Intel VT-d, AMD-Vi)

Performance Monitoring::

  - TSC (Time Stamp Counter) calibration
  - PMC (Performance Monitoring Counter) access
  - CPU frequency scaling detection
  - Cache hierarchy enumeration

8.3 Planned for v1.0.0
-----------------------------------------------------------

ACPI Integration::

  - Full ACPI table parsing (DSDT, SSDT)
  - ACPI event handling (_Lxx, _Exx)
  - Power state transitions (S1-S5)
  - Thermal zone monitoring

SMP Bootstrapping::

  - Application Processor (AP) startup via INIT-SIPI-SIPI
  - Per-CPU data structures
  - Inter-processor interrupts (IPI)
  - TLB shootdown protocol

Hardware Watchdog::

  - Watchdog timer configuration
  - Automatic reset on kernel hang
  - Integration with panic handler

================================================================================
End of HAL Documentation
================================================================================
