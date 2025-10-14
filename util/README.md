# Util (Utilities) Module

## Overview

The util module provides essential utility functions and infrastructure for the Serix kernel, focusing on error handling, panic management, and system halt operations. It serves as a collection of common utilities that don't fit into more specialized modules but are needed across the kernel.

## Architecture

### Components

1. **Panic Handler** (`panic.rs`): Kernel panic and oops (non-fatal error) handling
2. **Dummy Allocator** (`lib.rs`): Placeholder global allocator (unused after memory module loads)
3. **Alloc Error Handler** (`lib.rs`): Handler for allocation failures

### Design Philosophy

- **Fail Safely**: When errors occur, provide maximum debugging information before halting
- **No Recovery**: Early-stage kernel errors are unrecoverable; fail fast for easier debugging
- **Minimal Dependencies**: Utilities should have minimal dependencies to avoid circular dependencies

## Module Structure

```
util/
├── src/
│   ├── lib.rs      # Module definition, dummy allocator, alloc error handler
│   └── panic.rs    # Panic handling and system halt
└── Cargo.toml
```

## Panic Handling (panic.rs)

### Oops Function

```rust
pub fn oops(msg: &str)
```

**Purpose**: Handles non-fatal kernel errors (similar to Linux "Oops").

**Etymology**: "Oops" is Unix/Linux terminology for a kernel error that's serious but doesn't necessarily require a full panic. The system prints an error message and halts.

**Implementation**:
```rust
serial_println!("[KERNEL OOPS] {}", msg);
halt_loop();
```

**Behavior**:
1. Prints error message to serial console with `[KERNEL OOPS]` prefix
2. Enters infinite halt loop

**Usage Example**:
```rust
if !is_valid_address(addr) {
    oops("Invalid memory address");
}
```

**When to Use**:
- CPU exceptions (divide by zero, page fault, etc.)
- Hardware errors detected
- Assertion failures
- Unrecoverable errors that don't involve Rust panics

**Difference from Panic**:
- `panic!()`: Rust-level panic with unwinding (we use abort mode)
- `oops()`: Kernel-level error reporting for hardware/CPU exceptions

### Halt Loop

```rust
pub fn halt_loop() -> !
```

**Purpose**: Enters an infinite loop that halts the CPU, never returning.

**Implementation**:
```rust
loop {
    unsafe {
        core::arch::asm!("hlt");
    }
}
```

**HLT Instruction**: Halts the CPU until the next interrupt arrives.

**Why Loop?**
- Interrupts can wake the CPU from HLT
- Loop ensures system stays halted even if woken
- Never returns (marked with `-> !`)

**Power Efficiency**: Using `hlt` in a loop is more power-efficient than busy-waiting:
```rust
// Bad: Busy-wait (100% CPU usage)
loop {}

// Good: Halt loop (minimal CPU usage)
loop { hlt() }
```

**CPU Behavior**:
1. Execute `HLT` instruction → CPU enters low-power state
2. Interrupt arrives → CPU wakes, handles interrupt
3. Returns to next instruction after `HLT` → loop repeats
4. Execute `HLT` again → CPU halts again

**Interrupt Handler Consideration**: Even in a halt loop, interrupt handlers (timer, keyboard) will still execute. This is intentional for debugging (serial output in handlers works).

### Panic vs Oops

| Aspect | `panic!()` | `oops()` |
|--------|-----------|----------|
| Origin | Rust runtime | Kernel-defined |
| Use Case | Software bugs | Hardware/CPU exceptions |
| Call Site | Explicit `panic!()` | Exception handlers |
| Unwinding | Abort mode (no unwinding) | N/A (direct halt) |
| Recovery | None (abort) | None (halt) |

**Example Panic Scenarios**:
```rust
// Array bounds check
let arr = [1, 2, 3];
let _ = arr[10];  // panic: index out of bounds

// Unwrap on None
let opt: Option<i32> = None;
let _ = opt.unwrap();  // panic: unwrap on None

// Division by zero (if not caught)
let x = 5 / 0;  // oops: CPU exception (not a panic)
```

## Dummy Allocator (lib.rs)

### Purpose

```rust
pub struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        ptr::null_mut()
    }
    
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
```

**Why Needed?**
- Rust requires a `#[global_allocator]` to compile `no_std` code
- Memory module provides the real allocator
- This is a placeholder that always fails

**Behavior**:
- `alloc()`: Always returns null pointer (allocation failure)
- `dealloc()`: Does nothing (no-op)

**When Active?**
- Only before memory module's `#[global_allocator]` is linked
- In practice, never used (memory module loaded first)

**Alternative Approach**: Could panic on alloc, but null pointer is clearer.

## Alloc Error Handler (lib.rs)

```rust
#[alloc_error_handler]
pub fn alloc_error_handler(_: core::alloc::Layout) -> !
```

**Purpose**: Called by Rust runtime when allocation fails.

**Parameters**: `Layout` describes the failed allocation (size, alignment).

**Implementation**:
```rust
loop {}
```

**Why Infinite Loop?**
- No recovery possible (out of memory)
- Could panic, but this is already a panic path
- Simple and predictable behavior

**Future Enhancement**:
```rust
#[alloc_error_handler]
pub fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    serial_println!("Allocation failed: size={}, align={}", 
        layout.size(), layout.align());
    panic!("Out of memory");
}
```

## Error Handling Strategy

### Current Approach: Fail Fast

```
Error Detected
     ↓
Log to Serial
     ↓
Halt System
```

**Rationale**:
- Early-stage kernel
- Most errors are bugs, not runtime conditions
- Fail fast for easier debugging
- No user-mode processes to protect (yet)

### Future: Selective Recovery

#### Recoverable Errors

**User-Mode Faults**:
```rust
if error_in_user_mode() {
    kill_process();  // Don't crash kernel
    schedule_next(); // Continue execution
}
```

**Transient Hardware Errors**:
```rust
if transient_error() {
    retry_operation();
    if still_failing() {
        log_error();
        // Continue or fail gracefully
    }
}
```

#### Non-Recoverable Errors

**Kernel Page Fault**:
```rust
if page_fault_in_kernel() {
    oops("Kernel page fault");  // Must halt
}
```

**Double Fault**:
```rust
// System in inconsistent state
panic!("Double fault");  // Must halt immediately
```

**Hardware Failure**:
```rust
if critical_hardware_failure() {
    oops("Hardware failure");  // Must halt
}
```

## Usage Examples

### CPU Exception Handler

```rust
extern "x86-interrupt" fn page_fault_handler(
    stack: InterruptStackFrame,
    error_code: PageFaultErrorCode
) {
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
    }
    
    serial_println!("Page Fault!");
    serial_println!("  Address: {:#x}", cr2);
    serial_println!("  Error: {:?}", error_code);
    serial_println!("  RIP: {:#x}", stack.instruction_pointer.as_u64());
    
    util::panic::oops("Page fault exception");
}
```

### Assertion Macros

```rust
macro_rules! kassert {
    ($cond:expr, $msg:expr) => {
        if !$cond {
            serial_println!("Assertion failed: {}", $msg);
            util::panic::oops(concat!("Assertion: ", $msg));
        }
    };
}

// Usage
kassert!(page_is_mapped(addr), "Page not mapped");
```

### Resource Validation

```rust
pub fn validate_pointer<T>(ptr: *const T) -> Result<(), &'static str> {
    if ptr.is_null() {
        return Err("Null pointer");
    }
    
    if !is_aligned(ptr) {
        return Err("Misaligned pointer");
    }
    
    if !is_mapped(ptr as u64) {
        return Err("Unmapped pointer");
    }
    
    Ok(())
}

// Usage
validate_pointer(ptr).unwrap_or_else(|e| {
    oops(e);
});
```

## Debugging

### Serial Output in Halt Loop

Even when halted, serial output remains functional:

```rust
pub fn halt_loop() -> !
{
    serial_println!("System halted. Debug info:");
    serial_println!("  Last function: ...");
    serial_println!("  Stack trace: ...");
    
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
```

**Why This Works**: Serial port is independent of CPU halt state. Interrupts can still fire and handlers can output to serial.

### Panic Information

The kernel panic handler (in `kernel/src/main.rs`) provides:

```rust
#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    serial_println!("[KERNEL PANIC]");
    
    if let Some(loc) = info.location() {
        serial_println!("Location: {}:{}", loc.file(), loc.line());
    }
    
    if let Some(msg) = info.message() {
        serial_println!("Message: {}", msg);
    }
    
    halt_loop();
}
```

**PanicInfo Fields**:
- `location()`: File and line number of panic
- `message()`: Panic message string
- `payload()`: Arbitrary panic payload (rarely used)

### Stack Trace (Future)

```rust
pub fn print_stack_trace() {
    serial_println!("Stack trace:");
    
    let mut rbp: u64;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
    }
    
    for i in 0..10 {
        if rbp == 0 {
            break;
        }
        
        unsafe {
            let return_addr = *(rbp as *const u64).offset(1);
            serial_println!("  #{}: {:#x}", i, return_addr);
            rbp = *(rbp as *const u64);
        }
    }
}
```

**Limitation**: Requires frame pointers (compile with `-Cforce-frame-pointers=yes`).

## Thread Safety

### Halt Loop

`halt_loop()` is safe in any context:
- Single-threaded: Only way to stop execution
- Multi-threaded: Halts only current CPU
- Interrupt context: Safe to call from handlers

### Oops Function

**Race Condition**: Multiple CPUs calling `oops()` simultaneously could interleave serial output.

**Solution (Future)**:
```rust
static OOPS_LOCK: Mutex<()> = Mutex::new(());

pub fn oops(msg: &str) {
    let _lock = OOPS_LOCK.lock();
    serial_println!("[KERNEL OOPS] {}", msg);
    halt_loop();
}
```

**Better Solution**: Disable interrupts and use atomic flag:
```rust
static OOPS_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn oops(msg: &str) {
    x86_64::instructions::interrupts::disable();
    
    if OOPS_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        // Already in oops on another CPU
        halt_loop();
    }
    
    serial_println!("[KERNEL OOPS] {}", msg);
    halt_loop();
}
```

## Performance Considerations

### Halt vs Busy-Wait

| Operation | CPU Usage | Power | Thermal | Responsiveness |
|-----------|-----------|-------|---------|----------------|
| Busy-wait (`loop {}`) | 100% | High | High | Immediate |
| Halt loop (`loop { hlt() }`) | <1% | Low | Low | ~1µs (interrupt latency) |

**Recommendation**: Always use `hlt` in idle loops for power efficiency.

### Panic Path Optimization

**Problem**: Panic path might be optimized away or poorly optimized.

**Solution**: Mark panic functions as cold:
```rust
#[cold]
#[inline(never)]
pub fn oops(msg: &str) -> ! {
    // ...
}
```

**`#[cold]`**: Tells compiler this function is rarely called, optimize for size not speed.

**`#[inline(never)]`**: Prevents inlining, keeps code size small at call sites.

## Future Enhancements

### Error Codes

```rust
pub enum KernelError {
    OutOfMemory,
    InvalidAddress,
    PermissionDenied,
    HardwareError,
    // ...
}

impl KernelError {
    pub fn oops(self) -> ! {
        serial_println!("[KERNEL OOPS] {:?}", self);
        halt_loop();
    }
}
```

### Error Context

```rust
pub struct ErrorContext {
    pub message: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub registers: RegisterState,
}

pub fn oops_with_context(ctx: ErrorContext) -> ! {
    serial_println!("[KERNEL OOPS] {}", ctx.message);
    serial_println!("  Location: {}:{}", ctx.file, ctx.line);
    serial_println!("  Registers:");
    serial_println!("    RAX: {:#x}", ctx.registers.rax);
    // ... more registers
    halt_loop();
}
```

### Graceful Degradation

```rust
pub enum RecoveryAction {
    Halt,           // Unrecoverable
    KillProcess,    // Kill user process, continue kernel
    Retry,          // Retry operation
    Ignore,         // Log and continue
}

pub fn handle_error(error: KernelError) -> RecoveryAction {
    match error {
        KernelError::OutOfMemory => RecoveryAction::Halt,
        KernelError::UserPageFault => RecoveryAction::KillProcess,
        // ...
    }
}
```

### Kernel Panic Dump

```rust
pub fn dump_kernel_state() {
    serial_println!("=== KERNEL STATE DUMP ===");
    serial_println!("Uptime: {} ticks", get_ticks());
    serial_println!("Active tasks: {}", get_task_count());
    serial_println!("Memory:");
    serial_println!("  Heap used: {} bytes", heap_used());
    serial_println!("  Heap free: {} bytes", heap_free());
    serial_println!("Interrupts: {}", are_interrupts_enabled());
    // ... more state
}
```

## Dependencies

### Internal Crates

- **hal**: Serial output for error messages

### External Crates

- **linked_list_allocator** (0.10.5): For dummy allocator trait

## Configuration

### Cargo.toml

```toml
[package]
name = "util"
version = "0.1.0"
edition = "2024"

[dependencies]
linked_list_allocator = "0.10.5"
hal = { path = "../hal" }
```

## Testing

### Unit Tests (Future)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_oops_formats_correctly() {
        // Test that oops formats message correctly
        // (Can't test halt behavior in unit test)
    }
}
```

### Integration Tests

```rust
// Test that panic handler is called correctly
#[test_case]
fn test_panic_handler() {
    // Cause a panic
    panic!("Test panic");
    
    // Should never reach here
    unreachable!();
}
```

## Best Practices

### Error Messages

**Good**:
```rust
oops("Page fault at address 0x12345678");
oops("Invalid syscall number: 999");
oops("Hardware error: disk controller timeout");
```

**Bad**:
```rust
oops("Error");  // Too vague
oops("Something went wrong");  // Not helpful
oops("");  // No information
```

### When to Panic vs Oops

**Use `panic!()`**:
- Software bugs (assertions, unwrap failures)
- Impossible states (`unreachable!()`)
- Contract violations

**Use `oops()`**:
- Hardware faults (page fault, divide by zero)
- Unrecoverable system errors
- From exception handlers

### Error Recovery

```rust
// Don't do this (hiding errors):
if let Err(_) = operation() {
    // Silently ignore
}

// Do this (explicit handling):
operation().unwrap_or_else(|e| {
    serial_println!("Operation failed: {:?}", e);
    oops("Unrecoverable error");
});
```

## References

- [Linux Kernel Oops](https://www.kernel.org/doc/html/latest/admin-guide/bug-hunting.html)
- [OSDev - Panic](https://wiki.osdev.org/Panic)
- [Rust Panic Handling](https://doc.rust-lang.org/nomicon/panic-handler.html)

## License

GPL-3.0 (see LICENSE file in repository root)
