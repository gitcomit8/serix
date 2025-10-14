# Graphics Module

## Overview

The graphics module provides framebuffer manipulation and text console capabilities for the Serix kernel. It interfaces directly with the linear framebuffer provided by the Limine bootloader to render pixels, draw visual elements, and display text using a bitmap font. This module is essential for visual feedback during boot and runtime output.

## Architecture

### Components

1. **Framebuffer Manipulation**: Direct pixel writing to display memory
2. **Text Console**: Character rendering with scrolling support
3. **Memory Visualization**: Graphical representation of memory maps
4. **Global Console**: Thread-safe global console instance (optional feature)

### Design Philosophy

The graphics module operates at the lowest level of the display stack, providing:
- **No Dependencies on OS Services**: Works immediately after boot
- **Simple and Direct**: Raw framebuffer access without abstraction layers
- **Performance**: Direct memory operations without buffering
- **Flexibility**: Can be built with or without global console feature

## Module Structure

```
graphics/
├── src/
│   ├── lib.rs              # Core framebuffer functions
│   └── console/
│       ├── mod.rs          # Text console implementation
│       └── font8x16.bin    # 8x16 pixel bitmap font
└── Cargo.toml
```

## Framebuffer Basics (lib.rs)

### Pixel Format

The framebuffer uses a 32-bit BGRA format:

```
Byte 0: Blue  (0x00 - 0xFF)
Byte 1: Green (0x00 - 0xFF)
Byte 2: Red   (0x00 - 0xFF)
Byte 3: Alpha (typically 0x00, unused)
```

**Color Example**:
```rust
let blue_pixel = [0xFF, 0x00, 0x00, 0x00];   // Pure blue
let green_pixel = [0x00, 0xFF, 0x00, 0x00];  // Pure green
let red_pixel = [0x00, 0x00, 0xFF, 0x00];    // Pure red
let white_pixel = [0xFF, 0xFF, 0xFF, 0x00];  // White
let black_pixel = [0x00, 0x00, 0x00, 0x00];  // Black
```

### Framebuffer Layout

```
Physical Address: Provided by bootloader
Virtual Address:  Mapped by bootloader (directly accessible)

Structure:
- Width: Screen width in pixels
- Height: Screen height in pixels
- Pitch: Bytes per scanline (may be > width × BPP due to padding)
- BPP: Bits per pixel (typically 32)

Memory Layout:
Address = Base + (y × Pitch) + (x × BytesPerPixel)
```

### Core Pixel Writing

```rust
pub unsafe fn write_pixel(ptr: *mut u8, offset: usize, color: &[u8; 4])
```

**Purpose**: Writes a single pixel to the framebuffer at the specified offset.

**Parameters**:
- `ptr`: Base pointer to framebuffer memory
- `offset`: Byte offset from base (calculated as `y × pitch + x × 4`)
- `color`: BGRA color array

**Implementation**:
```rust
unsafe {
    core::ptr::copy_nonoverlapping(color.as_ptr(), ptr.add(offset), 4);
}
```

**Why `copy_nonoverlapping`?**
- Efficiently copies 4 bytes in a single operation
- Compiler can optimize to a single 32-bit write
- Semantically correct for non-overlapping memory regions

**Safety Considerations**:
- Caller must ensure `offset + 4` is within framebuffer bounds
- Framebuffer memory must be valid and writable
- No bounds checking for performance

### Filling Screen

```rust
pub fn fill_screen_blue(fb: &Framebuffer)
```

**Purpose**: Fills entire screen with a solid blue color (classic boot indicator).

**Implementation**:
```rust
let width = fb.width() as usize;
let height = fb.height() as usize;
let pitch = fb.pitch() as usize;
let bpp = fb.bpp() as usize;
let ptr = fb.addr() as *mut u8;
let blue_pixel = [0xFF, 0x00, 0x00, 0x00]; // BGRA

for y in 0..height {
    for x in 0..width {
        let offset = y * pitch + x * (bpp / 8);
        unsafe {
            write_pixel(ptr, offset, &blue_pixel);
        }
    }
}
```

**Performance**: For a 1920×1080 display, this writes 2,073,600 pixels. At ~3 cycles per pixel, this takes ~6 million cycles or ~2ms on a 3GHz CPU.

**Why Blue?**
Historical convention from old operating systems (e.g., Windows "Blue Screen of Death", but inverted here as "Blue Screen of Success").

### Memory Map Visualization

```rust
pub fn draw_memory_map(fb: &Framebuffer, entries: &[&Entry])
```

**Purpose**: Visualizes system memory map as colored bars at the bottom of the screen.

**Color Coding**:
- **Green** ([0x00, 0xFF, 0x00, 0x00]): Usable RAM
- **Yellow** ([0xFF, 0xFF, 0x00, 0x00]): Bootloader reclaimable
- **Gray** ([0x80, 0x80, 0x80, 0x00]): Reserved/other

**Layout**:
```
+-----------------------------------+
|                                   |
|     Screen content                |
|                                   |
|                                   |
+-----------------------------------+
| [===Memory Map Bars (40px)===]  | ← Bottom 40 pixels
+-----------------------------------+
```

**Implementation Details**:
```rust
let count = entries.len();
let max_count = width.min(count);
let bar_width = width / max_count.max(1);

for (i, entry) in entries.iter().take(max_count).enumerate() {
    let color = match entry.entry_type {
        EntryType::USABLE => [0x00, 0xFF, 0x00, 0x00],
        EntryType::BOOTLOADER_RECLAIMABLE => [0xFF, 0xFF, 0x00, 0x00],
        _ => [0x80, 0x80, 0x80, 0x00],
    };
    
    let x_start = i * bar_width;
    let x_end = (x_start + bar_width).min(width);
    
    for x in x_start..x_end {
        for y in (height - 40)..height {
            let offset = y * pitch + x * (bpp / 8);
            unsafe { write_pixel(ptr, offset, &color); }
        }
    }
}
```

**Visual Example**:
```
[Green][Green][Yellow][Gray][Green][Green][Gray]...
  ^       ^       ^      ^
  |       |       |      |
  |       |       |      +-- Reserved (ACPI, MMIO)
  |       |       +--------- Bootloader code
  |       +----------------- More usable RAM
  +------------------------- Usable RAM
```

## Text Console (console/mod.rs)

### Font System

#### Font Format

The module uses an 8x16 bitmap font stored in `font8x16.bin`:

```rust
const FONT_8X16: &[u8] = include_bytes!("font8x16.bin");
```

**Font Layout**:
- ASCII characters 32-127 (96 printable characters)
- Each character: 16 bytes (16 rows × 8 pixels)
- Each byte represents one row of 8 pixels
- Bit 7 (MSB) = leftmost pixel, Bit 0 = rightmost pixel

**Character Indexing**:
```rust
let glyph_offset = (ascii_value - 32) * 16;
let glyph = &FONT_8X16[glyph_offset..glyph_offset + 16];
```

**Example - Letter 'A' (ASCII 65)**:
```
Offset: (65 - 32) × 16 = 528

Byte:  Binary        Visual
0:     00011000      ...XX...
1:     00111100      ..XXXX..
2:     01100110      .XX..XX.
3:     01100110      .XX..XX.
4:     01100110      .XX..XX.
5:     01111110      .XXXXXX.
6:     01100110      .XX..XX.
7:     01100110      .XX..XX.
8:     01100110      .XX..XX.
9-15:  00000000      ........
```

### Console Structure

```rust
pub struct FramebufferConsole {
    framebuffer: *mut u8,  // Pointer to framebuffer memory
    width: usize,          // Screen width in pixels
    height: usize,         // Screen height in pixels
    pitch: usize,          // Bytes per scanline
    cursor_x: usize,       // Cursor column (in characters)
    cursor_y: usize,       // Cursor row (in characters)
}
```

**Character Grid**:
- Columns: `width / 8` (each character is 8 pixels wide)
- Rows: `height / 16` (each character is 16 pixels tall)
- Example: 1920×1080 → 240 columns × 67 rows

### Console Methods

#### Initialization

```rust
pub unsafe fn new(framebuffer: *mut u8, width: usize, height: usize, pitch: usize) -> Self
```

**Purpose**: Creates a new console instance with cursor at (0, 0).

**Safety**: Caller must ensure framebuffer pointer is valid for the entire lifetime of the console.

#### Character Output

```rust
fn put_char(&mut self, c: char)
```

**Purpose**: Outputs a single character at the current cursor position.

**Special Characters**:
- `'\n'` (newline): Move cursor to start of next line, scroll if needed
- `'\r'` (carriage return): Move cursor to start of current line
- Other: Render as glyph and advance cursor

**Cursor Advancement**:
```rust
self.cursor_x += 1;
if self.cursor_x * 8 >= self.width {
    self.cursor_x = 0;
    self.cursor_y += 1;
    self.scroll_if_needed();
}
```

#### Character Rendering

```rust
fn draw_char(&mut self, c: char, x_char: usize, y_char: usize)
```

**Purpose**: Renders a single character glyph to the framebuffer.

**Algorithm**:
```rust
let glyph = &FONT_8X16[(c - 32) * 16..][..16];
let x_pixel = x_char * 8;
let y_pixel = y_char * 16;

for (row, &bits) in glyph.iter().enumerate() {
    for bit in 0..8 {
        let pixel_on = (bits & (1 << (7 - bit))) != 0;
        let pixel = if pixel_on {
            [0xFF, 0xFF, 0xFF, 0x00]  // White foreground
        } else {
            [0x00, 0x00, 0x00, 0x00]  // Black background
        };
        
        let offset = (y_pixel + row) * pitch + (x_pixel + bit) * 4;
        unsafe {
            for p in 0..4 {
                write_volatile(fb.add(offset + p), pixel[p]);
            }
        }
    }
}
```

**Performance**: Each character renders 128 pixels (8×16). At ~4 cycles per pixel, this is ~512 cycles or ~0.17μs per character on a 3GHz CPU.

#### Scrolling

```rust
fn scroll_if_needed(&mut self)
```

**Purpose**: Checks if cursor has moved past bottom of screen and scrolls up if needed.

```rust
let max_lines = self.height / 16;
if self.cursor_y >= max_lines {
    self.scroll_up();
    self.cursor_y = max_lines - 1;
}
```

```rust
fn scroll_up(&mut self)
```

**Purpose**: Scrolls screen contents up by one character line (16 pixels).

**Implementation**:
```rust
let fb = self.framebuffer;
let pitch = self.pitch;
let height_bytes = self.height * pitch;

// Move all lines up by 16 pixels
let src = fb.add(16 * pitch);
core::ptr::copy(src, fb, height_bytes - 16 * pitch);

// Clear the last line
let clear_start = fb.add(height_bytes - 16 * pitch);
for i in 0..(16 * pitch) {
    write_volatile(clear_start.add(i), 0);
}
```

**Performance**: For 1920×1080×4 bytes, this copies ~8MB of data. Modern CPUs can do this in ~2ms using optimized `memcpy` implementations.

### Global Console (Feature: `global-console`)

#### Feature Flag

```toml
[features]
default = []
global-console = ["spin"]
```

**Purpose**: Enables global console instance with thread-safe access via mutex.

#### Global Instance

```rust
#[cfg(feature = "global-console")]
static GLOBAL_CONSOLE: Mutex<Option<FramebufferConsole>> = Mutex::new(None);
```

#### Initialization

```rust
#[cfg(feature = "global-console")]
pub fn init_console(framebuffer: *mut u8, width: usize, height: usize, pitch: usize)
```

**Purpose**: Initializes the global console instance.

**Usage in Kernel**:
```rust
let fb = framebuffer_response.framebuffers().next().expect("No framebuffer");
graphics::console::init_console(
    fb.addr(),
    fb.width() as usize,
    fb.height() as usize,
    fb.pitch() as usize
);
```

#### Console Access

```rust
#[cfg(feature = "global-console")]
pub fn console() -> impl Write + 'static
```

**Purpose**: Returns a locked handle to the global console that implements `core::fmt::Write`.

**Implementation**:
```rust
struct ConsoleGuard<'a> {
    guard: MutexGuard<'a, Option<FramebufferConsole>>,
}

impl<'a> Write for ConsoleGuard<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(console) = &mut *self.guard {
            console.write_string(s);
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}
```

**Lock Duration**: The lock is held only for the duration of the write operation, then automatically released when the guard is dropped.

### Macros

#### `fb_print!`

```rust
#[macro_export]
macro_rules! fb_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = $crate::console::console().write_fmt(format_args!($($arg)*));
    }};
}
```

**Purpose**: Prints formatted text to the framebuffer console (no newline).

**Usage**:
```rust
fb_print!("Hello, ");
fb_print!("value = {}", 42);
```

#### `fb_println!`

```rust
#[macro_export]
macro_rules! fb_println {
    () => {
        $crate::fb_print!("\n")
    };
    ($fmt:expr) => {
        $crate::fb_print!(concat!($fmt, "\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::fb_print!(concat!($fmt, "\n"), $($arg)*)
    };
}
```

**Purpose**: Prints formatted text to the framebuffer console with a newline.

**Usage**:
```rust
fb_println!("Booting Serix...");
fb_println!("Memory: {} MB", mem_size / 1024 / 1024);
fb_println!();  // Blank line
```

## Usage Examples

### Basic Framebuffer Manipulation

```rust
// Fill screen with color
let fb = framebuffer_response.framebuffers().next().unwrap();
graphics::fill_screen_blue(&fb);

// Draw memory map visualization
let mmap = memory_map_response.entries();
graphics::draw_memory_map(&fb, mmap);
```

### Text Console

```rust
// Initialize console
graphics::console::init_console(
    fb.addr(),
    fb.width() as usize,
    fb.height() as usize,
    fb.pitch() as usize
);

// Print text
graphics::fb_println!("Serix OS v0.1.0");
graphics::fb_println!("Copyright 2024");
graphics::fb_println!();
graphics::fb_println!("Booting...");
```

### Custom Character Rendering

```rust
use graphics::console::FramebufferConsole;

let mut console = unsafe {
    FramebufferConsole::new(fb.addr(), fb.width(), fb.height(), fb.pitch())
};

// Manual character output
console.put_char('H');
console.put_char('e');
console.put_char('l');
console.put_char('l');
console.put_char('o');
console.put_char('\n');
```

## Performance Characteristics

### Pixel Write Performance

| Operation | Pixels | Typical Time (3GHz CPU) |
|-----------|--------|-------------------------|
| Single pixel | 1 | ~10 ns |
| Character (8×16) | 128 | ~1 μs |
| Line (240 chars) | 30,720 | ~100 μs |
| Full screen (1920×1080) | 2,073,600 | ~7 ms |
| Scroll (1920×1080) | - | ~2 ms (memcpy) |

### Optimization Opportunities

1. **Buffered Rendering**: Render to offscreen buffer, then blit
2. **Dirty Rectangles**: Only redraw changed regions
3. **Hardware Acceleration**: Use GPU for blitting (requires driver)
4. **SIMD Instructions**: Use SSE/AVX for parallel pixel operations

## Thread Safety

### With `global-console` Feature

The global console uses a spinlock mutex:

```rust
use spin::Mutex;
static GLOBAL_CONSOLE: Mutex<Option<FramebufferConsole>> = Mutex::new(None);
```

**Thread Safety Guarantees**:
- Only one thread can access console at a time
- Interrupts should be disabled during console access (or use interrupt-safe mutex)
- No deadlocks if interrupts are properly managed

**Potential Issue**: If an interrupt fires while holding the console lock and the interrupt handler tries to print, it will deadlock.

**Solution**: Use interrupt-safe mutex or disable interrupts around console operations:

```rust
x86_64::instructions::interrupts::without_interrupts(|| {
    fb_println!("Critical message");
});
```

## Memory Safety

### Framebuffer Access

The framebuffer pointer is inherently unsafe:
- Points to memory-mapped hardware
- No Rust lifetime tracking
- Can be invalidated by hardware changes

**Safety Invariants**:
1. Framebuffer pointer must be valid for console lifetime
2. Width, height, pitch must accurately describe framebuffer
3. All pixel writes must be within bounds
4. Framebuffer memory must be writable

### Font Data

Font data is embedded at compile time:

```rust
const FONT_8X16: &[u8] = include_bytes!("font8x16.bin");
```

**Safety**: Immutable static data, safe to access from any context.

## Debugging

### Common Issues

#### No Output on Screen

**Checks**:
1. Framebuffer address valid? (check bootloader response)
2. Console initialized? (`init_console` called?)
3. Cursor position valid?
4. Text color different from background?

#### Corrupted Text

**Causes**:
- Incorrect pitch value (should be from bootloader, not calculated)
- Wrong BPP assumption (should be 32-bit BGRA)
- Framebuffer pointer invalidated

#### Scrolling Issues

**Causes**:
- Height not accounting for menu bars or reserved areas
- Pitch vs. width mismatch
- `memcpy` overlap (use `memmove` instead)

### Debug Utilities

```rust
pub fn dump_framebuffer_info(fb: &Framebuffer) {
    hal::serial_println!("Framebuffer Address: {:#x}", fb.addr());
    hal::serial_println!("Width: {}", fb.width());
    hal::serial_println!("Height: {}", fb.height());
    hal::serial_println!("Pitch: {}", fb.pitch());
    hal::serial_println!("BPP: {}", fb.bpp());
}
```

## Future Enhancements

### Graphics Primitives

```rust
pub fn draw_line(fb: &Framebuffer, x1: i32, y1: i32, x2: i32, y2: i32, color: &[u8; 4]);
pub fn draw_rect(fb: &Framebuffer, x: i32, y: i32, w: u32, h: u32, color: &[u8; 4]);
pub fn draw_circle(fb: &Framebuffer, x: i32, y: i32, radius: u32, color: &[u8; 4]);
```

### Image Support

```rust
pub fn draw_image(fb: &Framebuffer, x: i32, y: i32, image: &Image);
pub fn draw_bitmap(fb: &Framebuffer, x: i32, y: i32, bitmap: &[u8], w: u32, h: u32);
```

### Advanced Font Support

- Multiple font sizes
- TrueType font rendering
- Unicode support (UTF-8 decoding)
- Font styles (bold, italic)
- Anti-aliasing

### Window System Foundation

- Layered rendering
- Window management
- Clipping regions
- Z-order sorting

## Dependencies

### Internal Crates

None (graphics is a leaf module)

### External Crates

- **limine** (0.5.0): Framebuffer structure definitions
- **spin** (0.10.0, optional): Mutex for global console (with `global-console` feature)

## Configuration

### Cargo.toml

```toml
[package]
name = "graphics"
version = "0.1.0"
edition = "2024"

[features]
default = []
global-console = ["spin"]

[dependencies]
limine = "0.5.0"
spin = { version = "0.10.0", optional = true }
```

## References

- [OSDev - Printing To Screen](https://wiki.osdev.org/Printing_To_Screen)
- [OSDev - Text Mode Cursor](https://wiki.osdev.org/Text_Mode_Cursor)
- [VGA Hardware](https://wiki.osdev.org/VGA_Hardware)
- [Limine Protocol - Framebuffer](https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md#framebuffer-feature)

## License

GPL-3.0 (see LICENSE file in repository root)
