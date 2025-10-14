# Keyboard Module

## Overview

The keyboard module provides PS/2 keyboard input handling for the Serix kernel. It processes keyboard scancodes from the PS/2 controller, translates them to ASCII characters, and outputs them to both the serial console and framebuffer console. This module enables basic user input functionality essential for interactive operating systems.

## Architecture

### Components

1. **Scancode Handler**: Processes raw keyboard scancodes from port 0x60
2. **Scancode to ASCII Translation**: Maps scancodes to printable characters
3. **Keyboard Interrupt Enable**: Configures PIC to deliver keyboard interrupts

### PS/2 Keyboard Overview

The PS/2 keyboard is a legacy input device still commonly supported:
- **Port 0x60**: Data port (read scancode, write commands)
- **Port 0x64**: Status/Command port
- **IRQ 1**: Hardware interrupt line (vector 33 after remapping)

**Scancode Sets**:
- **Set 1** (IBM XT): Most common, used by BIOS
- **Set 2** (IBM AT): Default for PS/2 keyboards (translated to Set 1 by controller)
- **Set 3**: Rarely used

Serix assumes scancode Set 1 (translated from Set 2 by PS/2 controller).

## Module Structure

```
keyboard/
├── src/
│   └── lib.rs      # Scancode handling and translation
└── Cargo.toml
```

## Scancode Translation

### Scancode Format

PS/2 keyboards send scancodes in two types:

**Make Code** (Key Press):
```
Bit 7: 0
Bits 0-6: Key code
```

**Break Code** (Key Release):
```
Bit 7: 1
Bits 0-6: Same key code as make code
```

**Example**:
- 'A' key pressed: Scancode 0x1E (0001 1110)
- 'A' key released: Scancode 0x9E (1001 1110)

**Extended Scancodes** (not yet implemented):
Some keys (arrows, Home, End, etc.) send a 0xE0 prefix byte followed by one or more scancode bytes.

### Scancode to ASCII Mapping

```rust
const SCANDCODE_TO_ASCII: [u8; 128] = [
    0, 27, b'1', b'2', b'3', b'4', b'5', b'6',     // 0x00-0x07
    b'7', b'8', b'9', b'0', b'-', b'=', 8, b'\t',  // 0x08-0x0F
    b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', // 0x10-0x17
    b'o', b'p', b'[', b']', b'\n', 0, b'a', b's',  // 0x18-0x1F
    b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', // 0x20-0x27
    b'\'', b'`', 0, b'\\', b'z', b'x', b'c', b'v', // 0x28-0x2F
    b'b', b'n', b'm', b',', b'.', b'/', 0, b'*',   // 0x30-0x37
    0, b' ', 0, 0, 0, 0, 0, 0,                     // 0x38-0x3F
    // ... remaining entries (mostly 0 for unimplemented keys)
];
```

**Mapping Details**:

| Scancode | Key | ASCII | Notes |
|----------|-----|-------|-------|
| 0x00 | (none) | 0 | Reserved |
| 0x01 | Escape | 27 | ASCII ESC |
| 0x02-0x0B | 1-9, 0 | '1'-'9', '0' | Number row |
| 0x0C | Minus | '-' | Hyphen |
| 0x0D | Equals | '=' | Equals sign |
| 0x0E | Backspace | 8 | ASCII BS |
| 0x0F | Tab | '\t' | ASCII TAB |
| 0x10-0x1B | Q-P | 'q'-'p' | Top letter row |
| 0x1C | Enter | '\n' | ASCII LF |
| 0x1D | Left Ctrl | 0 | Modifier (not printed) |
| 0x1E-0x28 | A-L | 'a'-'l' | Home row |
| 0x29 | ' (apostrophe) | '\'' | Single quote |
| 0x2A | Left Shift | 0 | Modifier (not printed) |
| 0x2B | Backslash | '\\' | Backslash |
| 0x2C-0x32 | Z-M | 'z'-'m' | Bottom row |
| 0x33-0x35 | , . / | ',' '.' '/' | Punctuation |
| 0x36 | Right Shift | 0 | Modifier (not printed) |
| 0x37 | Keypad * | '*' | Asterisk |
| 0x38 | Left Alt | 0 | Modifier (not printed) |
| 0x39 | Space | ' ' | ASCII space |

**Unimplemented**:
- Function keys (F1-F12)
- Arrow keys
- Insert, Delete, Home, End, Page Up, Page Down
- Num Lock, Caps Lock, Scroll Lock
- Keypad numbers (0-9, Enter, +, -, *, /)
- Windows/Super keys
- Menu/Context key

**Special Values**:
- 0: No ASCII representation (modifier keys, function keys, etc.)
- 8: Backspace (ASCII BS, 0x08)
- '\t': Tab (ASCII HT, 0x09)
- '\n': Enter (ASCII LF, 0x0A)
- 27: Escape (ASCII ESC, 0x1B)

### US QWERTY Layout

The mapping assumes a US QWERTY keyboard layout:

```
Row 1:  ` 1 2 3 4 5 6 7 8 9 0 - = Backspace
Row 2:  Tab Q W E R T Y U I O P [ ]
Row 3:  Caps A S D F G H J K L ; ' Enter
Row 4:  Shift Z X C V B N M , . / Shift
Row 5:  Ctrl Alt Space Alt Ctrl
```

**International Layouts** (future): Support for AZERTY, QWERTZ, Dvorak, etc.

## Scancode Handling

### Main Handler Function

```rust
pub fn handle_scancode(scancode: u8)
```

**Purpose**: Processes a scancode from the keyboard controller.

**Called From**: Keyboard interrupt handler (IDT vector 33).

**Implementation**:

```rust
// Ignore break codes (key release)
if scancode & 0x80 != 0 {
    return;
}

// Look up ASCII value
if let Some(&ascii) = SCANDCODE_TO_ASCII.get(scancode as usize) {
    if ascii != 0 {
        // Output to serial console
        hal::serial_print!("{}", ascii as char);
        
        // Output to framebuffer console
        graphics::fb_print!("{}", ascii as char);
    }
}
```

**Logic**:

1. **Check High Bit**: If bit 7 is set, scancode is a break code (key release) → ignore
2. **Array Lookup**: Use scancode as index into translation table
3. **Filter Zero**: Zero means no ASCII representation (modifier key) → ignore
4. **Output**: Print character to both serial and framebuffer consoles

**Why Ignore Break Codes?**
- Simple implementation: only handle key press, not release
- Sufficient for basic text input
- Future: track key state for modifiers (Shift, Ctrl, Alt)

### Keyboard Interrupt Handler

Located in `idt/src/lib.rs`:

```rust
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    // Read scancode from keyboard data port
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    // Process scancode
    keyboard::handle_scancode(scancode);
    
    // Send EOI to APIC
    unsafe {
        const APIC_EOI: *mut u32 = 0xFEE000B0 as *mut u32;
        APIC_EOI.write_volatile(0);
    }
}
```

**Sequence**:
1. Read scancode from port 0x60
2. Process scancode (translate and output)
3. Signal End of Interrupt (EOI) to APIC

**Critical**: Must read port 0x60 or next interrupt won't fire.

## Keyboard Interrupt Configuration

### Enabling Keyboard Interrupt

```rust
pub fn enable_keyboard_interrupt()
```

**Purpose**: Unmasks IRQ1 on the PIC to allow keyboard interrupts.

**Implementation**:
```rust
unsafe {
    let mut port = Port::new(0x21);  // PIC1 data port (master)
    let mask: u8 = port.read();
    port.write(mask & !0x02);  // Clear bit 1 (IRQ1)
}
```

**PIC Interrupt Mask Register (IMR)**:
- Port 0x21: Master PIC (IRQ 0-7)
- Port 0xA1: Slave PIC (IRQ 8-15)
- Bit N: 0 = unmasked (enabled), 1 = masked (disabled)

**IRQ 1 Bit**: Bit 1 of port 0x21

**Note**: With APIC enabled, this function may not be needed. I/O APIC handles IRQ routing.

## Keyboard States (Future Implementation)

### Modifier Keys

```rust
static mut KEYBOARD_STATE: KeyboardState = KeyboardState::new();

struct KeyboardState {
    shift_pressed: bool,
    ctrl_pressed: bool,
    alt_pressed: bool,
    caps_lock: bool,
    num_lock: bool,
    scroll_lock: bool,
}
```

**Usage**:
```rust
pub fn handle_scancode(scancode: u8) {
    if scancode == 0x2A || scancode == 0x36 {  // Shift
        unsafe { KEYBOARD_STATE.shift_pressed = true; }
        return;
    }
    
    if scancode == 0xAA || scancode == 0xB6 {  // Shift release
        unsafe { KEYBOARD_STATE.shift_pressed = false; }
        return;
    }
    
    // Apply shift modifier
    let ascii = SCANCODE_TO_ASCII[scancode as usize];
    let ascii = if unsafe { KEYBOARD_STATE.shift_pressed } {
        to_uppercase(ascii)
    } else {
        ascii
    };
    
    // Output
    hal::serial_print!("{}", ascii as char);
}
```

### Caps Lock Toggle

```rust
if scancode == 0x3A {  // Caps Lock
    unsafe {
        KEYBOARD_STATE.caps_lock = !KEYBOARD_STATE.caps_lock;
        set_keyboard_led(LED_CAPS_LOCK, KEYBOARD_STATE.caps_lock);
    }
}
```

### Shift Key Mappings

```rust
const SHIFT_MAP: [(u8, u8); 47] = [
    (b'1', b'!'), (b'2', b'@'), (b'3', b'#'), (b'4', b'$'),
    (b'5', b'%'), (b'6', b'^'), (b'7', b'&'), (b'8', b'*'),
    (b'9', b'('), (b'0', b')'), (b'-', b'_'), (b'=', b'+'),
    (b'[', b'{'), (b']', b'}'), (b';', b':'), (b'\'', b'"'),
    (b'`', b'~'), (b'\\', b'|'), (b',', b'<'), (b'.', b'>'),
    (b'/', b'?'),
    // ... plus uppercase letters
];
```

## Extended Scancodes (Future)

### Two-Byte Scancodes

Arrow keys, Home, End, etc. send 0xE0 prefix:

```
Right Arrow: 0xE0 0x4D (make), 0xE0 0xCD (break)
Up Arrow:    0xE0 0x48 (make), 0xE0 0xC8 (break)
Home:        0xE0 0x47 (make), 0xE0 0xC7 (break)
```

**Implementation**:
```rust
static mut EXPECT_EXTENDED: bool = false;

pub fn handle_scancode(scancode: u8) {
    if scancode == 0xE0 {
        unsafe { EXPECT_EXTENDED = true; }
        return;
    }
    
    if unsafe { EXPECT_EXTENDED } {
        handle_extended_scancode(scancode);
        unsafe { EXPECT_EXTENDED = false; }
        return;
    }
    
    // Normal scancode handling...
}
```

### Three-Byte Scancodes

Print Screen sends three bytes:

```
Make:  0xE0 0x2A 0xE0 0x37
Break: 0xE0 0xB7 0xE0 0xAA
```

**Implementation**: State machine to accumulate bytes.

## Input Buffering (Future)

### Circular Buffer

```rust
const BUFFER_SIZE: usize = 256;

struct KeyboardBuffer {
    buffer: [u8; BUFFER_SIZE],
    read_pos: usize,
    write_pos: usize,
}

impl KeyboardBuffer {
    pub fn push(&mut self, scancode: u8) {
        self.buffer[self.write_pos] = scancode;
        self.write_pos = (self.write_pos + 1) % BUFFER_SIZE;
    }
    
    pub fn pop(&mut self) -> Option<u8> {
        if self.read_pos == self.write_pos {
            None  // Buffer empty
        } else {
            let scancode = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % BUFFER_SIZE;
            Some(scancode)
        }
    }
}
```

**Usage**:
- Interrupt handler: Push scancodes to buffer
- Main loop or shell: Pop scancodes from buffer

**Benefits**:
- Decouples interrupt handling from processing
- Prevents lost input during slow operations
- Allows batching of input processing

## Keyboard Commands (Future)

### PS/2 Controller Commands

Send to port 0x60 (after ensuring controller ready):

```rust
pub fn send_keyboard_command(cmd: u8) {
    // Wait for input buffer empty
    while !input_buffer_empty() {
        core::hint::spin_loop();
    }
    
    // Send command
    unsafe {
        let mut port = Port::new(0x60);
        port.write(cmd);
    }
}
```

**Common Commands**:

| Command | Value | Description |
|---------|-------|-------------|
| Set LEDs | 0xED | Set Num/Caps/Scroll Lock LEDs |
| Echo | 0xEE | Returns 0xEE (diagnostic) |
| Get/Set Scancode Set | 0xF0 | Query or change scancode set |
| Identify Keyboard | 0xF2 | Returns keyboard ID bytes |
| Set Typematic Rate | 0xF3 | Configure repeat rate/delay |
| Enable Scanning | 0xF4 | Enable keyboard |
| Disable Scanning | 0xF5 | Disable keyboard |
| Set Defaults | 0xF6 | Reset to default parameters |
| Resend | 0xFE | Resend last byte |
| Reset | 0xFF | Reset and self-test |

### Setting Keyboard LEDs

```rust
pub fn set_keyboard_leds(num_lock: bool, caps_lock: bool, scroll_lock: bool) {
    let led_state = 
        (num_lock as u8) |
        ((caps_lock as u8) << 1) |
        ((scroll_lock as u8) << 2);
    
    send_keyboard_command(0xED);  // Set LEDs command
    wait_for_ack();
    send_keyboard_command(led_state);  // LED state byte
    wait_for_ack();
}
```

## Debugging

### Scancode Logging

```rust
pub fn handle_scancode(scancode: u8) {
    hal::serial_println!("Scancode: 0x{:02X}", scancode);
    
    if scancode & 0x80 != 0 {
        hal::serial_println!("  (Break code)");
        return;
    }
    
    hal::serial_println!("  (Make code)");
    
    if let Some(&ascii) = SCANDCODE_TO_ASCII.get(scancode as usize) {
        if ascii != 0 {
            hal::serial_println!("  ASCII: {} (0x{:02X})", ascii as char, ascii);
        } else {
            hal::serial_println!("  (No ASCII)");
        }
    }
}
```

### Testing Keyboard Input

```rust
// In kernel main loop
loop {
    // Keyboard interrupts will fire and call handler
    hal::cpu::halt();
}
```

**Expected Behavior**:
1. Press key on keyboard
2. Scancode appears in serial output
3. Character appears on framebuffer
4. Release key (break code ignored)

## Performance Considerations

### Interrupt Latency

**Goal**: Minimize time spent in interrupt handler.

**Current Implementation**:
- Read scancode: ~100 cycles
- Array lookup: ~10 cycles
- Serial output: ~1000 cycles (waiting for UART)
- Framebuffer output: ~500 cycles (rendering character)
- Total: ~1600 cycles ≈ 0.5 µs @ 3 GHz

**Optimization**: Buffer scancodes, process in main loop:
```rust
// In interrupt handler (fast)
let scancode = read_keyboard_port();
KEYBOARD_BUFFER.push(scancode);
send_eoi();

// In main loop (slow)
while let Some(scancode) = KEYBOARD_BUFFER.pop() {
    handle_scancode(scancode);
}
```

### Keyboard Repeat Rate

**Default**: ~10.9 characters/sec with 500ms delay

**Adjusting**:
```rust
pub fn set_typematic_rate(rate: u8, delay: u8) {
    send_keyboard_command(0xF3);  // Set typematic rate/delay
    wait_for_ack();
    
    let param = (delay & 0x03) << 5 | (rate & 0x1F);
    send_keyboard_command(param);
    wait_for_ack();
}
```

**Rate Values**:
- 0x00: 30 chars/sec
- 0x1F: 2 chars/sec (slowest)

**Delay Values**:
- 0x00: 250ms
- 0x03: 1000ms (longest)

## Thread Safety

### Keyboard State

**Problem**: Keyboard state accessed from interrupt and main code.

**Solution**: Use atomic operations or disable interrupts:

```rust
use core::sync::atomic::{AtomicBool, Ordering};

static SHIFT_PRESSED: AtomicBool = AtomicBool::new(false);

// In interrupt handler
SHIFT_PRESSED.store(true, Ordering::Relaxed);

// In main code
let shift = SHIFT_PRESSED.load(Ordering::Relaxed);
```

## Future Enhancements

### Full Scancode Set Support

- Scancode Set 1 (XT)
- Scancode Set 2 (AT) - native mode
- Scancode Set 3 - rarely used

### USB Keyboard Support

- USB HID (Human Interface Device) protocol
- USB keyboard enumeration
- USB transfer handling
- Compatibility with PS/2 emulation (legacy support)

### Input Method Support

- Compose key sequences (é = Compose + ' + e)
- Dead keys (´ + e = é)
- Unicode input (Alt+U+00E9 = é)

### Keyboard Layouts

- Multiple layout support (US, UK, German, French, etc.)
- Runtime layout switching
- Layout configuration file

### Advanced Features

- Macro recording and playback
- Key remapping
- Custom keybindings
- Hotkey support (Ctrl+Alt+Del, etc.)

## Dependencies

### Internal Crates

- **hal**: Serial output, port I/O
- **graphics**: Framebuffer console output
- **x86_64**: Port abstractions

### External Crates

None (keyboard is a leaf module)

## Configuration

### Cargo.toml

```toml
[package]
name = "keyboard"
version = "0.1.0"
edition = "2024"

[dependencies]
hal = { path = "../hal" }
x86_64 = "0.15.2"
graphics = { path = "../graphics" }
```

## References

- [OSDev - PS/2 Keyboard](https://wiki.osdev.org/PS/2_Keyboard)
- [OSDev - Keyboard](https://wiki.osdev.org/Keyboard)
- [PS/2 Keyboard Interface](http://www.computer-engineering.org/ps2keyboard/)
- [Scancode Reference](https://www.win.tue.nl/~aeb/linux/kbd/scancodes.html)

## License

GPL-3.0 (see LICENSE file in repository root)
