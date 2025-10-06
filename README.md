# Serix Kernel

Serix is a next-generation operating system kernel written in Rust, booted via the
modern [Limine](https://github.com/limine-bootloader/limine) bootloader. The current status: a minimal kernel that
paints the screen blue if successfully booted.

---

## Features

- **Boots on real hardware and QEMU** using Limine (BIOS and UEFI supported)
- **No `std` or OS dependencies**
- **Framebuffer initialized and cleared to blue** as a visual bringup milestone

---

## Building and Running

### Prerequisites

- Nightly Rust toolchain (`rustup default nightly` and `rustup target add x86_64-unknown-none`)
- GNU Make
- QEMU (`qemu-system-x86_64`)
- `xorriso`
- `git`
- Internet connection (for Limine submodule)

### Quick Start

```shell
git clone https://github.com/gitcomit8/serix.git
cd serix
make run
```

This will:

- Build the kernel
- Prepare a bootable ISO image with Limine
- Launch in QEMU for instant feedback

You should see a **solid blue screen** upon successful boot.

---

## Repository Layout

| Path        | Purpose                     |
|-------------|-----------------------------|
| `kernel/`   | Rust kernel source code     |
| `apic/`     | APIC Subsystem              |
| `graphics/` | Graphics Subsystem          |
| `hal/`      | Hardware Abstraction Layer  |
| `idt/`      | Interrupt Descriptor Table  |
| `memory/`   | Memory Management Subsystem |
| `util/`     | Utilities                   |
| `limine/`   | Limine bootloader binaries  |
| `iso_root/` | Filesystem image root       |
| `Makefile`  | Build, ISO, run automation  |
| `docs/`     | Project documentation       |

---

## Documentation

For architecture, contributing, and future roadmap, see the [`docs/`](docs/) folder.

---

## License

This project is open source under the [GPLv3](LICENSE) license.

