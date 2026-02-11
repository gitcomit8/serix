# Graphics Subsystem Demo Recordings

This directory contains asciinema recordings for the graphics subsystem
documentation (`GRAPHICS_API.md`).

## Required Recordings

### graphics-init.cast

Asciinema recording showing:

- Graphics subsystem initialization sequence
- Blue screen background fill
- Memory map visualization rendering
- Console initialization
- Text output demonstration (fb_println!)
- Drawing primitives examples

## Recording Instructions

### Prerequisites

Install asciinema:

```bash
# Ubuntu/Debian
sudo apt install asciinema

# Arch Linux
sudo pacman -S asciinema

# Fedora
sudo dnf install asciinema
```

### Capture Process

Since Serix runs in QEMU (not a terminal), we need to record the serial output:

```bash
# Option 1: Record QEMU serial output directly
make run 2>&1 | tee qemu-output.txt

# Convert to asciinema format
# (Custom script needed to convert raw output to .cast format)

# Option 2: Use serial console via screen/minicom
screen -L /dev/pts/X  # Where X is QEMU serial port
# Then convert screen log to asciinema format
```

### Alternative: Manual Creation

Create .cast file manually with timing information:

```json
{"version": 2, "width": 80, "height": 24}
[0.0, "o", "Serix Kernel Initializing...\r\n"]
[0.5, "o", "Graphics subsystem starting\r\n"]
[1.0, "o", "Framebuffer: 1920x1080x32\r\n"]
```

### Asciinema File Format

.cast files use JSON Lines format:

- Header: JSON object with metadata
- Events: JSON arrays with [time, type, data]

See: https://docs.asciinema.org/manual/asciicast/v2/

## Recording Specifications

- Format: asciinema v2 (.cast)
- Terminal size: 80×24 (standard)
- Duration: 30-60 seconds
- Timing: Natural pauses for readability

## Publishing

Upload to asciinema.org or embed directly in documentation:

```bash
asciinema upload graphics-init.cast
```
