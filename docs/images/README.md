# Graphics Documentation Images

This directory contains screenshots and visual assets for the graphics subsystem
documentation (`GRAPHICS_API.md`).

## Required Images

### framebuffer-screen.png

Screenshot showing:

- Blue framebuffer background
- Colored memory map visualization bars
- Memory regions (usable, reserved, ACPI, etc.)

To capture: Boot Serix in QEMU and take screenshot after graphics initialization

### console-output.png

Screenshot showing:

- Text console rendering with 8×16 bitmap font
- Kernel initialization messages
- System information output (memory, CPU cores, etc.)

To capture: Boot Serix and take screenshot after console text output

### graphics-primitives.png

Screenshot demonstrating:

- Rectangle drawing (filled and outline)
- Line drawing (diagonal, horizontal, vertical)
- Pixel plotting
- Various colors

To capture: Run graphics primitives demo and take screenshot

## Capture Instructions

Using QEMU with Serix:

```bash
# Run QEMU with monitor
make run

# In QEMU monitor (Ctrl+Alt+2):
screendump framebuffer-screen.png

# Or use external tools:
scrot -s framebuffer-screen.png  # Linux with scrot
```

Convert to web-friendly format:

```bash
convert framebuffer-screen.png -resize 800x600 framebuffer-screen.png
optipng -o7 framebuffer-screen.png
```

## Image Specifications

- Format: PNG
- Max width: 800px (for documentation embedding)
- Compression: Optimized with optipng or similar
- Color depth: 24-bit RGB
