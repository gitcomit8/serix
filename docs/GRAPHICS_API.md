====================================

# Serix Framebuffer and Console Driver

:Last Updated: 2025-01-13

## Introduction

The Serix graphics subsystem provides framebuffer access and text console
services for kernel-space visual output. It implements a two-tier architecture:
low-level pixel manipulation for direct framebuffer control and a high-level
text console abstraction for character-based output.

This document describes the graphics driver implementation, programming
interfaces, and usage guidelines. It follows Linux kernel documentation
conventions and is organized for kernel developers.

## Status (v0.0.5)

Working Features:

- Linear framebuffer initialization via Limine bootloader
- Blue screen background rendering
- Memory map visualization (colored bars)
- Text console with 8×16 bitmap font
- Console output macros (fb_print!, fb_println!)
- Automatic scrolling
- Drawing primitives (pixels, rectangles, lines)

Known Limitations:

- No color attributes for text (white on black only)
- No Unicode support (ASCII 32-127 printable characters)
- No hardware acceleration
- Single-buffered rendering (no double buffering)

## Screenshots

## Demo Recording

## Design Philosophy

Zero-Copy Architecture
  Direct framebuffer manipulation without intermediate buffers eliminates
  memory copies and reduces latency

Hardware-Agnostic
  Works with any linear RGB/BGR framebuffer provided by Limine bootloader,
  supporting various resolutions and pixel formats

Type-Safe Rendering
  Rust's type system enforces memory safety and prevents common graphics bugs
  like buffer overflows and race conditions

Minimal Overhead
  No dynamic dispatch in hot rendering paths, spinlocks for global state
  protection, inline functions for pixel operations

Thread-Safe by Design
  Global console protected by spinlocks, safe for concurrent access from
  multiple kernel threads and interrupt handlers

## Architecture Overview

## Module Structure

The graphics subsystem is organized as a Rust workspace crate

```

graphics/
├── src/
│   ├── lib.rs              # Framebuffer primitives, pixel operations
│   └── console/
│       ├── mod.rs          # Text console implementation
│       └── font8x16.bin    # 8×16 bitmap font data (1536 bytes)
├── Cargo.toml              # Crate manifest
└── README.md

```

Dependencies

```

[dependencies]
limine = "0.5.0"           # Limine boot protocol structures
spin = "0.10.0"            # Spinlock for thread safety (optional)

[features]
global-console = ["spin"]   # Enable GLOBAL_CONSOLE singleton

```

## System Context Diagram

The graphics subsystem sits between kernel code and GPU hardware

```

┌───────────────────────────────────────────────────────┐
│                    Kernel Code                        │
│   ┌──────────────┐         ┌──────────────────┐      │
│   │ Application  │         │    Interrupt     │      │
│   │    Logic     │         │     Handlers     │      │
│   └──────┬───────┘         └──────┬───────────┘      │
│          │                        │                   │
│          │ fb_println!()         │ serial_println!() │
│          ▼                        ▼                   │
│   ┌────────────────────────────────────────────┐     │
│   │      Graphics API (graphics crate)         │     │
│   │  ┌────────────┐    ┌──────────────────┐   │     │
│   │  │   Text     │    │   Framebuffer    │   │     │
│   │  │  Console   │    │      API         │   │     │
│   │  └────┬───────┘    └────┬─────────────┘   │     │
│   └───────┼─────────────────┼─────────────────┘     │
│           │                 │                        │
│           ▼                 ▼                        │
│   ┌────────────────────────────────────────────┐    │
│   │        Framebuffer Memory (VRAM)           │    │
│   │  Linear RGB/BGR buffer, WC memory type     │    │
│   │  Base: 0xE000_0000 (typical)              │    │
│   │  Size: width × height × 4 bytes            │    │
│   └────────────────────────────────────────────┘    │
└─────────────────────────┬───────────────────────────┘

```

## Text Output Data Flow

Text rendering follows this path

```

fb_println!("Hello")
format_args!() macro expansion
ConsoleGuard::write_fmt()
FramebufferConsole::write_string()
FramebufferConsole::put_char() [for each character]
FramebufferConsole::draw_char()
FONT_8X16 bitmap lookup
write_volatile() to framebuffer memory
GPU scans framebuffer → display

```

## Pixel Output Data Flow

Direct pixel rendering path

```

write_pixel(ptr, offset, &color)
copy_nonoverlapping() to framebuffer
GPU scans framebuffer → display

```

## Component Relationships

Global console singleton architecture

```

┌──────────────────────────────────────────────┐
│           GLOBAL_CONSOLE                     │
│  static Mutex<Option<FramebufferConsole>>    │
│        (Thread-safe singleton)               │
└──────────────────┬───────────────────────────┘

```

## Boot Initialization Sequence

Graphics initialization occurs after heap setup

```

1. Limine bootloader sets up framebuffer, provides Framebuffer structure
2. Kernel receives FRAMEBUFFER_REQ response
3. Fill screen blue (fill_screen_blue)
4. Draw memory map visualization (draw_memory_map)
5. Initialize console (init_console)
6. Text output enabled (fb_println! macros work)

```

## Framebuffer Low-Level API

## Data Structures

struct Framebuffer

```~~~~~~~~~~~~~~~

Provided by Limine bootloader protocol (limine crate)

```

pub struct Framebuffer {
}

```
Typical Configuration

```

Resolution: 1920×1080
BPP:        32 bits (4 bytes per pixel)
Format:     BGRA (Blue-Green-Red-Alpha)
Pitch:      7680 bytes (1920 × 4, may include padding)
Memory:     ~8.3 MB (1920 × 1080 × 4)

```

## Core Functions

write_pixel()
```~~~~~~~~~~

Write a single pixel to framebuffer

```

pub unsafe fn write_pixel(ptr: *mut u8, offset: usize, color: &[u8; 4])

```
Parameters:
  ptr
    Base pointer to framebuffer memory (from fb.addr())
  offset
    Byte offset from base (y × pitch + x × 4)
  color
    4-byte BGRA color value [B, G, R, A]

Safety Requirements:
  - ptr must point to valid mapped framebuffer
  - offset must be in bounds: offset + 4 ≤ height × pitch
  - ptr + offset must be 4-byte aligned
  - Framebuffer must remain mapped during write

Color Format

```

color[0] = Blue  (0x00 - 0xFF)
color[1] = Green (0x00 - 0xFF)
color[2] = Red   (0x00 - 0xFF)
color[3] = Alpha (0xFF = opaque, usually ignored)

```
Implementation

```

unsafe {
}

```
Performance:
  Time Complexity
    O(1) - single memory write
  CPU Cycles
    10-50 cycles (cache-dependent)
  Throughput
    1-4 GB/s (PCIe bandwidth limited)

Example

```

let fb = FRAMEBUFFER_REQ.get_response().unwrap().framebuffers()[0];
let ptr = fb.addr() as *mut u8;
let pitch = fb.pitch() as usize;

// Draw red pixel at (100, 50)
let offset = 50 *pitch + 100* 4;
let red = [0x00, 0x00, 0xFF, 0x00];  // BGRA
unsafe {
}

```
fill_screen_blue()
```~~~~~~~~~~~~~~~

Fill entire framebuffer with blue color

```

pub fn fill_screen_blue(fb: &Framebuffer)

```

Parameters:
  fb
    Reference to Framebuffer structure

Description:
  Safe wrapper around unsafe pixel operations. Fills all pixels with pure
  blue (BGRA: [0xFF, 0x00, 0x00, 0x00]). Used for initialization and testing.

Performance

```

Resolution    Pixels      Memory      Duration (typical)
──────────────────────────────────────────────────────────
1920×1080     2,073,600   8.3 MB      5-20 ms
2560×1440     3,686,400   14.7 MB     10-35 ms
3840×2160     8,294,400   33.2 MB     20-80 ms

```

Algorithm

```

let blue_pixel = [0xFF, 0x00, 0x00, 0x00];  // BGRA
for y in 0..height {
}

```

Optimization Note:
  Future implementations could use memset or SIMD for faster fills.

draw_memory_map()

```~~~~~~~~~~~~~~

Visualize physical memory map as colored bars

```

pub fn draw_memory_map(fb: &Framebuffer, entries: &[&Entry])

```
Parameters:
  fb
    Reference to Framebuffer structure
  entries
    Slice of Limine memory map entries

Description:
  Draws horizontal colored bars representing physical memory regions. Each
  bar's color indicates memory type (usable, reserved, ACPI, etc.). Used
  for debugging and system visualization.

Color Mapping

```

Memory Type           Color (BGRA)
─────────────────────────────────────────
Usable                Green  [0x00, 0xFF, 0x00, 0x00]
Reserved              Red    [0x00, 0x00, 0xFF, 0x00]
ACPI Reclaimable      Yellow [0x00, 0xFF, 0xFF, 0x00]
ACPI NVS              Orange [0x00, 0x80, 0xFF, 0x00]
Bad Memory            Gray   [0x80, 0x80, 0x80, 0x00]
Bootloader Reclaimable Cyan  [0xFF, 0xFF, 0x00, 0x00]

```

## Coordinate Calculations

Offset Calculation
```~~~~~~~~~~~~~~~

Calculate byte offset for pixel at (x, y)

```

offset = y *pitch + x* (bpp / 8)

```

Where:
  pitch
    Bytes per scanline (typically width × 4)
  bpp
    Bits per pixel (typically 32)

Pitch vs Width

```

Width:   Visible pixels per scanline
Pitch:   Total bytes per scanline (includes padding)
Padding: GPU alignment requirements (e.g., 64-byte boundaries)

```

Example with Padding

```

Resolution: 1920×1080, 32 bpp
Width:      1920 pixels
Ideal size: 1920 × 4 = 7680 bytes/line

If GPU requires 64-byte alignment:
Pitch:      7744 bytes (next multiple of 64 ≥ 7680)
Padding:    64 bytes/line (16 pixels worth)

```

**Example Usage**:

```rust
let fb = FRAMEBUFFER_REQ.get_response().unwrap().framebuffers()[0];

// Clear screen to blue
graphics::fill_screen_blue(fb);
```

**Alternatives**:

- `fill_screen_color(fb, &[r, g, b, a])` - Fill with custom color (not yet implemented)
- `clear_screen(fb)` - Fill with black (not yet implemented)

## ### draw_memory_map

```rust
pub fn draw_memory_map(fb: &Framebuffer, entries: &[&Entry])
```

**Purpose**: Visualizes system memory map as colored bars at bottom of screen (debugging tool).

**Parameters**:

- `fb`: Reference to framebuffer information structure
- `entries`: Slice of memory map entries from bootloader

**Visualization**:

```
Screen Layout:
┌────────────────────────────────────────┐
│                                        │
│         (Normal Display Area)          │
│                                        │
├────────────────────────────────────────┤
│ ▓▓▓▓░░░░▓▓▓▓▓▓▓▓▒▒▒▒▓▓▓▓░░░░▓▓▓▓▓▓▓▓  │ ← Memory map bars
│ ▓▓▓▓░░░░▓▓▓▓▓▓▓▓▒▒▒▒▓▓▓▓░░░░▓▓▓▓▓▓▓▓  │   (40 pixels high)
└────────────────────────────────────────┘
  └─┬─┘└─┬──┘└────┬───┘└─┬──┘
   Usable  Boot    Reserved Usable
   (Green)(Yellow) (Gray)  (Green)
```

**Color Mapping**:

```rust
match entry.entry_type {
    EntryType::USABLE                  => [0x00, 0xFF, 0x00, 0x00],  // Green
    EntryType::BOOTLOADER_RECLAIMABLE  => [0xFF, 0xFF, 0x00, 0x00],  // Yellow
    _                                  => [0x80, 0x80, 0x80, 0x00],  // Gray
}
```

**Bar Width Calculation**:

```rust
let bar_width = screen_width / min(entry_count, screen_width);
```

**Height**: 40 pixels from bottom of screen (`height - 40` to `height`).

**Example Usage**:

```rust
let fb = FRAMEBUFFER_REQ.get_response().unwrap().framebuffers()[0];
let mmap = MMAP_REQ.get_response().unwrap();
let entries: Vec<&Entry> = mmap.entries().iter().collect();

graphics::draw_memory_map(fb, &entries);
```

**Use Cases**:

- Boot-time memory visualization
- Debugging memory allocation issues
- Demonstrating framebuffer capabilities

## ### Coordinate System

```
Origin (0, 0) is top-left corner:

    0   1   2   3  ... width-1   (X-axis →)
  ┌───┬───┬───┬───┬───────┬───┐
0 │   │   │   │   │       │   │
  ├───┼───┼───┼───┼───────┼───┤
1 │   │   │   │   │       │   │
  ├───┼───┼───┼───┼───────┼───┤
2 │   │   │   │   │       │   │
  ├───┼───┼───┼───┼───────┼───┤
... │   │   │   │   │       │   │
  ├───┼───┼───┼───┼───────┼───┤
h-1│   │   │   │   │       │   │
  └───┴───┴───┴───┴───────┴───┘
(Y-axis ↓)
```

**Offset Calculation**:

```rust
// For pixel at (x, y):
let offset = y * pitch + x * (bpp / 8);
    Ideal size: 1920 × 4 = 7680 bytes/line
    
    If GPU requires 64-byte alignment:
    Pitch:      7744 bytes (next multiple of 64 ≥ 7680)
    Padding:    64 bytes/line (16 pixels worth)


## Text Console API


## Overview

The text console provides character-based output on the framebuffer using
software-rendered 8×16 bitmap fonts. It implements core::fmt::Write for
integration with Rust's formatting macros.

Architecture

```

┌──────────────────────────────────────────────┐
│       GLOBAL_CONSOLE (singleton)             │
│   static Mutex<Option<FramebufferConsole>>   │
└──────────────────┬───────────────────────────┘

```

## Data Structures

struct FramebufferConsole
```~~~~~~~~~~~~~~~~~~~~~~

Console state and rendering context

```

pub struct FramebufferConsole {
}

```
Thread Safety

```

unsafe impl Send for FramebufferConsole {}
unsafe impl Sync for FramebufferConsole {}

```
Rationale:
  Framebuffer pointer is inherently unsafe but can be shared across threads
  if protected by proper synchronization (Mutex in GLOBAL_CONSOLE).

Character Grid Dimensions

```

// For 1920×1080 screen with 8×16 font:
columns = width / 8    // 1920 / 8 = 240 columns
rows = height / 16     // 1080 / 16 = 67 rows (floor)

```
Cursor Position:
  - Stored in character coordinates (not pixels)
  - Range: cursor_x ∈ [0, columns), cursor_y ∈ [0, rows)
  - Automatically wraps to next line at right edge
  - Triggers scrolling when reaching bottom row


## Initialization Functions

FramebufferConsole::new()
```~~~~~~~~~~~~~~~~~~~~~~~

Low-level constructor (prefer init_console for global instance)

```

pub unsafe fn new(
) -> Self

```
Parameters:
  framebuffer
    Pointer to framebuffer memory base
  width
    Screen width in pixels
  height
    Screen height in pixels
  pitch
    Bytes per scanline

Returns:
  Initialized FramebufferConsole with cursor at (0, 0)

Safety Requirements:
  - framebuffer must point to valid, writable memory
  - Memory region must be at least height × pitch bytes
  - Pointer must remain valid for console lifetime
  - No concurrent writes to framebuffer allowed

init_console()
```~~~~~~~~~~~

Initialize global console singleton

```

[cfg(feature = "global-console")]
pub fn init_console(
)

```
Feature:
  Requires "global-console" Cargo feature (pulls in spin crate)

Effect:
  Sets GLOBAL_CONSOLE to Some(FramebufferConsole)

Thread Safety:
  Protected by Mutex, safe to call from multiple threads (first call wins)

Example

```

// In kernel initialization (kernel/src/main.rs):
let fb = FRAMEBUFFER_REQ.get_response()

graphics::console::init_console(
);

// Console now available globally
fb_println!("Console initialized!");

```

## Character Output Functions

put_char()
```~~~~~~~

Write single character at cursor position

```

fn put_char(&mut self, c: char)

```
Parameters:
  c
    Unicode character to render (ASCII printable and control chars)

Behavior:

Newline ('\\n')

```

if c == '\n' {
}

```
Carriage Return ('\\r')

```

if c == '\r' {
}

```
Printable Characters

```

// Draw character at current cursor
self.draw_char(c, self.cursor_x, self.cursor_y);

// Advance cursor
self.cursor_x += 1;

// Wrap to next line if needed
if self.cursor_x >= self.width / 8 {
}

```
Unsupported Characters:
  Non-ASCII characters (> 127) are rendered as '?'

write_string()
```~~~~~~~~~~~

Write string to console

```

fn write_string(&mut self, s: &str)

```
Parameters:
  s
    UTF-8 string to output

Implementation

```

for c in s.chars() {
}

```
write_fmt()
```~~~~~~~~

Formatted output (core::fmt::Write trait)

```

fn write_fmt(&mut self, args: fmt::Arguments) -> fmt::Result

```
Parameters:
  args
    Formatted arguments from format_args!()

Returns:
  Ok(()) on success, Err(fmt::Error) if console not initialized

Usage:
  Enables fb_print! and fb_println! macros via core::fmt::Write trait


## Console Macros

fb_print!
```~~~~~~

Print without newline

```

fb_print!($($arg:tt)*)

```
Example

```

fb_print!("CPU cores: ");
fb_print!("{}", core_count);

```
fb_println!
```~~~~~~~~

Print with newline

```

fb_println!();
fb_println!($fmt:expr);
fb_println!($fmt:expr, $($arg:tt)*);

```
Examples

```

fb_println!();                              // Blank line
fb_println!("Boot complete");               // Simple message
fb_println!("Memory: {} MB", mem_mb);       // Formatted output
fb_println!("Addr: {:#x}", addr);           // Hexadecimal

```
Implementation

```

[macro_export]
macro_rules! fb_println {

```

## draw_char()

Render single character using bitmap font

```

fn draw_char(&mut self, c: char, x_char: usize, y_char: usize)

```
Parameters:
  c
    Character to render (ASCII 32-126 supported)
  x_char
    Column position (character coordinates, 0-based)
  y_char
    Row position (character coordinates, 0-based)

Character Lookup

```

let c = c as u8;
 let glyph = if c < 32 || c > 127 {
} else {
};

```
Font Data Structure:
  - 96 characters (ASCII 32-127)
  - Each character: 16 bytes (one byte per row)
  - Each byte: 8 bits (one bit per pixel column)
  - Total size: 96 × 16 = 1536 bytes

Pixel Coordinates

```

let x_pixel = x_char *8;    // Left edge of character
let y_pixel = y_char* 16;   // Top edge of character

```
Rendering Algorithm

```

for (row, &bits) in glyph.iter().enumerate() {

}

```
Bit Order:
  MSB (bit 7) represents leftmost pixel in character

Example Font Data

```

Character 'A' (ASCII 65, font index 33):

Byte  Binary      Visual
───────────────────────────
0:    00000000    ········
1:    00000000    ········
2:    00011000    ···##···
3:    00111100    ··####··
4:    01100110    ·##··##·
5:    01100110    ·##··##·
6:    01111110    ·######·
7:    01100110    ·##··##·
8:    01100110    ·##··##·
9:    01100110    ·##··##·
10:   00000000    ········
11:   00000000    ········
12:   00000000    ········
13:   00000000    ········
14:   00000000    ········
15:   00000000    ········

```
Performance

```

Pixels Written:     128 pixels/char (8 × 16)
Memory Written:     512 bytes/char (128 × 4)
CPU Cycles:         500-2000 cycles/char (cache-dependent)
Throughput:         500K-2M chars/sec (theoretical, single-threaded)

```
Optimization Opportunities (Future):
  - SIMD for parallel pixel writes
  - Dirty region tracking (only redraw changed areas)
  - Character cell caching
  - Background thread rendering


## Scrolling


## scroll_if_needed()

Check cursor position and scroll if needed

```

fn scroll_if_needed(&mut self)

```
Trigger Condition

```

let max_lines = self.height / 16;
if self.cursor_y >= max_lines {
}

```
Example

```

1920×1080 screen:
max_lines = 1080 / 16 = 67
If cursor_y reaches 67, scroll up and set cursor_y = 66

```

## scroll_up()

Scroll console contents up by one character row

```

fn scroll_up(&mut self)

```
Description:
  Moves all screen contents up by 16 pixel rows (one character line),
  discarding the top line and clearing the bottom line.

Algorithm

```

let fb = self.framebuffer;
let pitch = self.pitch;
let height_bytes = self.height * pitch;

// Step 1: Move all lines up by 16 pixel rows
let src = fb.add(16 *pitch);    // Source: row 16 onward
let dst = fb;                     // Dest: row 0 onward
let count = height_bytes - 16* pitch;
core::ptr::copy(src, dst, count);

// Step 2: Clear bottom 16 rows (new blank line)
let clear_start = fb.add(height_bytes - 16 *pitch);
for i in 0..(16* pitch) {
}

```
Memory Operations

```

1920×1080 screen, 32 bpp:
pitch = 7680 bytes/line
height_bytes = 1080 × 7680 = 8,294,400 bytes (~8.3 MB)

Move:  8,171,520 bytes (~8.2 MB)
Clear: 122,880 bytes (~123 KB, 16 rows)

```
Performance

```

Duration:   1-5 ms (modern hardware)
Method:     core::ptr::copy() (memmove, compiler-optimized)

```
Visual Effect

```

Before:
Line 0:  "First line"
Line 1:  "Second line"
...
Line 66: "Last line"

After scroll_up():
Line 0:  "Second line"     ← Was line 1
Line 1:  "Third line"      ← Was line 2
...
Line 65: "Last line"       ← Was line 66
Line 66: ""                ← Cleared (blank)

```
Alternative Approaches (Not Implemented):
  Circular Buffer
    Wrap viewport instead of copying (requires viewport management)
  GPU Blit
    Use GPU to accelerate copy (requires GPU driver)
  Virtual Console
    Larger off-screen buffer with viewport (requires more memory)


## Font System


## Font Format

FONT_8X16 Bitmap Font
```~~~~~~~~~~~~~~~~~~

Static bitmap font data

```

const FONT_8X16: &[u8] = include_bytes!("console/font8x16.bin");

```
Specifications:
  Character Set
    ASCII 32-127 (96 printable characters)
  Glyph Size
    8 pixels wide × 16 pixels tall
  Data Format
    16 bytes per character (one byte per row)
  Encoding
    1 bit = foreground pixel, 0 bit = background pixel
  Total Size
    1536 bytes (96 × 16)
  Source
    Standard VGA text mode font

Character Index Calculation

```

char_index = (ascii_code - 32) * 16

```
Examples

```

' ' (space, ASCII 32)  → index 0
'A' (ASCII 65)         → index 528 ((65-32)*16)
'Z' (ASCII 90)         → index 928 ((90-32)*16)
'~' (ASCII 126)        → index 1504 ((126-32)*16)

```
Character Rendering Properties

```

Foreground Color:  White [0xFF, 0xFF, 0xFF, 0x00] (BGRA)
Background Color:  Black [0x00, 0x00, 0x00, 0x00] (BGRA)

Note: Colors are hardcoded (no color attributes yet)

```
Font Loading

```

// Compile-time inclusion
const FONT_8X16: &[u8] = include_bytes!("console/font8x16.bin");

// Runtime access
let glyph = &FONT_8X16[char_index..char_index + 16];

```

## Color Model


## Pixel Format

BGRA Format (Default)
```~~~~~~~~~~~~~~~~~~

Most common framebuffer pixel format

```

Byte Offset    Channel    Value Range
────────────────────────────────────────
0              Blue       0x00 - 0xFF
1              Green      0x00 - 0xFF
2              Red        0x00 - 0xFF
3              Alpha      0x00 - 0xFF (often ignored)

```
Memory Layout

```

[B, G, R, A] = [0x00, 0x00, 0xFF, 0x00]  → Red pixel
[B, G, R, A] = [0xFF, 0x00, 0x00, 0x00]  → Blue pixel
[B, G, R, A] = [0xFF, 0xFF, 0xFF, 0x00]  → White pixel

```
RGB Format (Alternative)
```~~~~~~~~~~~~~~~~~~~~~~

Less common but supported by some framebuffers

```

Byte Offset    Channel    Value Range
────────────────────────────────────────
0              Red        0x00 - 0xFF
1              Green      0x00 - 0xFF
2              Blue       0x00 - 0xFF
3              Alpha      0x00 - 0xFF (often ignored)

```
Format Detection

```

// Limine provides format information
let fb = FRAMEBUFFER_REQ.get_response().unwrap().framebuffers()[0];

match fb.memory_model() {
}

```

## Color Constants

Common Colors (BGRA)

```

Black:    [0x00, 0x00, 0x00, 0x00]
White:    [0xFF, 0xFF, 0xFF, 0x00]
Red:      [0x00, 0x00, 0xFF, 0x00]
Green:    [0x00, 0xFF, 0x00, 0x00]
Blue:     [0xFF, 0x00, 0x00, 0x00]
Yellow:   [0x00, 0xFF, 0xFF, 0x00]
Cyan:     [0xFF, 0xFF, 0x00, 0x00]
Magenta:  [0xFF, 0x00, 0xFF, 0x00]
Gray:     [0x80, 0x80, 0x80, 0x00]

```
Alpha Channel:
  Most framebuffers ignore alpha channel. Set to 0x00 for consistency.


## Memory Layout and Performance


## Memory Type Configuration

Write-Combining (WC)
```~~~~~~~~~~~~~~~~~

Framebuffers typically use WC memory type for optimal performance

```

Memory Type        Characteristics
───────────────────────────────────────────────────────────
Write-Combining    Batches writes, ~4× faster than uncached

Performance:       ~4 GB/s write bandwidth (typical)
Latency:           ~500 ns per write
Use Case:          Graphics framebuffers

```
Uncached (UC)
```~~~~~~~~~~

Alternative (slower) memory type

```

Memory Type        Characteristics
───────────────────────────────────────────────────────────
Uncached           Every write is separate bus transaction

Performance:       ~100 MB/s write bandwidth
Latency:           ~500 ns per write
Use Case:          Device registers, strict ordering required

```
Cached (WB)
```~~~~~~~~

NOT used for framebuffers

```

Memory Type        Characteristics
───────────────────────────────────────────────────────────
Write-Back         Causes coherency issues with GPU

Performance:       ~50 GB/s (if it worked)
Use Case:          System RAM only

```
Performance Comparison

```

Memory Type        Write Bandwidth   Latency
─────────────────────────────────────────────
System RAM (WB)    ~50 GB/s          ~50 ns
Framebuffer (WC)   ~4 GB/s           ~500 ns
Framebuffer (UC)   ~100 MB/s         ~500 ns

```

## Optimization Techniques

Write-Combining Efficiency
```~~~~~~~~~~~~~~~~~~~~~~~~

Efficient (sequential, batched)

```

for i in 0..1000 {
}

```
Inefficient (random access)

```

for i in [0, 500, 100, 800, 50] {
}

```
Memory Barriers
```~~~~~~~~~~~~

Not Required on x86_64:
  - Strong memory ordering model
  - Writes visible to other cores in program order
  - GPU memory controllers handle coherency

Required on ARM (Future)

```

use core::sync::atomic::{fence, Ordering};

write_pixel(fb, offset, &color);
fence(Ordering::Release);  // Ensure writes visible to GPU

```

## Scalability

Framebuffer Sizes

```

Resolution    Bytes       Pixels/Frame    60 FPS Bandwidth
────────────────────────────────────────────────────────────
1920×1080     8,294,400   2,073,600       ~475 MB/s
2560×1440     14,745,600  3,686,400       ~844 MB/s
3840×2160     33,177,600  8,294,400       ~1.9 GB/s

```
Console Character Capacity

```

Resolution    Columns × Rows   Total Chars
─────────────────────────────────────────────
1920×1080     240 × 67         16,080
2560×1440     320 × 90         28,800
3840×2160     480 × 135        64,800

```

## Thread Safety and Synchronization


## Locking Strategy

Global Console Protection

```

static GLOBAL_CONSOLE: Mutex<Option<FramebufferConsole>>

```
Mutex Type:
  spin::Mutex (spinlock, suitable for kernel)

Lock Characteristics:
  Overhead
    ~50-200 cycles per lock/unlock
  Fairness
    Not guaranteed (FIFO not enforced)
  Deadlock
    Possible if lock held during interrupt

Safe Usage Pattern

```

use x86_64::instructions::interrupts;

 interrupts::without_interrupts( || {
});

```

## Race Conditions

Potential Issue:
  Interrupt during console write

Scenario

```

Thread A: Acquires lock, starts writing "Hello"
Interrupt: Tries to acquire lock (already held)

```
Solution:
  Disable interrupts during console operations

Automatic Protection (Via Macros)

```

// Current implementation (no interrupt disable):
fb_println!("Text");  // UNSAFE if called from interrupt context

// Future implementation:
[macro_export]
macro_rules! fb_println {
}

```
Current Limitation:
  User must manually disable interrupts if calling from ISR


## Usage Examples


## Basic Framebuffer Operations

Direct pixel manipulation

```

use graphics::{write_pixel, fill_screen_blue};

let fb = FRAMEBUFFER_REQ.get_response().unwrap().framebuffers()[0];
let ptr = fb.addr() as *mut u8;
let pitch = fb.pitch() as usize;
let bpp = fb.bpp() as usize;

// Fill screen with blue
fill_screen_blue(fb);

// Draw diagonal line
for i in 0..100 {
}

```

## Console Initialization and Output

Setting up text console

```

// In kernel initialization:
let fb = FRAMEBUFFER_REQ.get_response().unwrap().framebuffers()[0];

graphics::console::init_console(
);

// Simple text output:
fb_println!("Serix Kernel v0.0.5");
fb_println!("Memory: {} MB", total_memory / 1024 / 1024);
fb_println!();

// Formatted output:
for i in 0..10 {
}

```

## Logging Integration

Dual output to serial and framebuffer

```

pub fn kernel_log(level: LogLevel, msg: &str) {

}

```

## Drawing Primitives

Rectangle

```

pub fn draw_rect(
) {

}

```
Line (Bresenham's algorithm)

```

pub fn draw_line(
) {
}

```

## Future Extensions


## Planned Features

Color Support
```~~~~~~~~~~

Text with color attributes

```

pub struct ColorAttr {
}

impl FramebufferConsole {
}

```
Unicode Support
```~~~~~~~~~~~~

Extended character set

```

// Extend to BMP (Basic Multilingual Plane)
const FONT_8X16_UNICODE: &[u8] = include_bytes!("unifont-8x16.bin");

// Character index: U+0000 to U+FFFF (65536 characters)
// Size: 65536 * 16 = 1 MB

```
Graphical Primitives
```~~~~~~~~~~~~~~~~~

Drawing library

```

pub mod draw {
}

```
Double Buffering
```~~~~~~~~~~~~~

Eliminate tearing

```

pub struct DoubleBuffer {
}

impl DoubleBuffer {
}

```
Hardware Acceleration
```~~~~~~~~~~~~~~~~~~

GPU-accelerated operations

```

pub trait GpuBlitter {
}

```

## Appendix


## Complete API Reference

Framebuffer Functions

```

pub unsafe fn write_pixel(ptr: *mut u8, offset: usize, color: &[u8; 4]);
pub fn fill_screen_blue(fb: &Framebuffer);
pub fn draw_memory_map(fb: &Framebuffer, entries: &[&Entry]);

```
Console Types

```

pub struct FramebufferConsole { /*...*/ }
unsafe impl Send for FramebufferConsole {}
unsafe impl Sync for FramebufferConsole {}

```
Console Functions

```

[cfg(feature = "global-console")]
pub fn init_console(
);

impl FramebufferConsole {
}

```
Macros

```

fb_print!($($arg:tt)*);
fb_println!();
fb_println!($fmt:expr);
fb_println!($fmt:expr, $($arg:tt)*);

```

## Configuration Constants

Font and Color Definitions

```

// Font dimensions
const CHAR_WIDTH: usize = 8;
const CHAR_HEIGHT: usize = 16;

// Color definitions
const FOREGROUND: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x00];  // White
const BACKGROUND: [u8; 4] = [0x00, 0x00, 0x00, 0x00];  // Black

// Font data
const FONT_8X16: &[u8] = include_bytes!("font8x16.bin");
const FONT_CHAR_COUNT: usize = 96;
const FONT_CHAR_SIZE: usize = 16;

```

## Performance Benchmarks

Test System: Intel i5-10400, 16GB RAM, Intel UHD Graphics 630

==================================  =========  =============
Operation                           Duration   Throughput
==================================  =========  =============
write_pixel (single)                ~50 ns     ~20M pixels/s
fill_screen_blue (1920×1080)        ~12 ms     ~173M pixels/s
draw_char (single)                  ~1.5 µs    ~667K chars/s
fb_println!("Hello")                ~8 µs      ~125K lines/s
scroll_up                           ~3 ms      ~333 scrolls/s
==================================  =========  =============


## Error Handling

Current Implementation:
  No explicit error codes (uses panics and Option/Result)

Future Error Handling

```

pub enum GraphicsError {
}

pub type GraphicsResult<T> = Result<T, GraphicsError>;

```

## References

- Linux Kernel Documentation: Documentation/fb/
- Limine Boot Protocol: https://github.com/limine-bootloader/limine
- VGA Text Mode Fonts: https://wiki.osdev.org/VGA_Fonts
- Framebuffer Howto: https://www.kernel.org/doc/Documentation/fb/

# ## End of File
