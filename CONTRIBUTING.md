# Contributing to Serix Kernel

Thank you for your interest in contributing to Serix! This document provides guidelines and instructions for contributing to this next-generation operating system kernel written in Rust.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Code Style Guidelines](#code-style-guidelines)
- [Building and Testing](#building-and-testing)
- [Commit Guidelines](#commit-guidelines)
- [Pull Request Process](#pull-request-process)
- [Areas to Contribute](#areas-to-contribute)
- [Community and Communication](#community-and-communication)

## Code of Conduct

### Our Pledge

We are committed to providing a welcoming and inclusive environment for all contributors. We expect all participants to:

- Use welcoming and inclusive language
- Be respectful of differing viewpoints and experiences
- Accept constructive criticism gracefully
- Focus on what is best for the community
- Show empathy towards other community members

### Unacceptable Behavior

- Harassment, trolling, or discriminatory comments
- Publishing others' private information without permission
- Any conduct which could reasonably be considered inappropriate in a professional setting

## Getting Started

### Prerequisites

Before you begin, ensure you have the following installed:

- **Rust Nightly Toolchain**: Install via `rustup default nightly`
- **Target Support**: `rustup target add x86_64-unknown-none`
- **GNU Make**: For build automation
- **QEMU**: `qemu-system-x86_64` for testing
- **xorriso**: For ISO image creation
- **Git**: For version control

### First Steps

1. **Fork the Repository**

   ```bash
   # Visit https://github.com/gitcomit8/serix and click "Fork"
   ```

2. **Clone Your Fork**

   ```bash
   git clone https://github.com/YOUR_USERNAME/serix.git
   cd serix
   ```

3. **Add Upstream Remote**

   ```bash
   git remote add upstream https://github.com/gitcomit8/serix.git
   ```

4. **Build and Test**

   ```bash
   make run
   ```

   You should see a **solid blue screen** in QEMU upon successful boot.

## Development Setup

### Repository Structure

The Serix kernel is organized as a Cargo workspace with multiple crates:

| Crate | Purpose |
|-------|---------|
| `kernel/` | Kernel entry point and initialization |
| `memory/` | Page tables, heap, frame allocation |
| `hal/` | Hardware abstraction (serial, CPU topology) |
| `apic/` | APIC interrupt controller |
| `idt/` | Interrupt Descriptor Table |
| `graphics/` | Framebuffer console and drawing |
| `task/` | Async task executor and scheduler |
| `capability/` | Capability-based security system |
| `drivers/` | Device drivers (VirtIO, PCI, console) |
| `vfs/` | Virtual filesystem |
| `ipc/` | Inter-process communication |
| `loader/` | ELF loader for userspace binaries |
| `ulib/` | Userspace library |

### Build Commands

```bash
# Build kernel only
cargo build --release --manifest-path kernel/Cargo.toml --target x86_64-unknown-none

# Build bootable ISO (includes kernel + init + Limine)
make iso

# Build and run in QEMU
make run

# Clean build artifacts
make clean
cargo clean

# Format code (uses tabs, not spaces)
cargo fmt

# Run Clippy linter
cargo clippy --target x86_64-unknown-none
```

### Understanding the Boot Process

1. **Firmware (BIOS/UEFI)** → **Limine bootloader** → **Kernel `_start()`**
2. Kernel initialization sequence:
   - Initialize serial console (COM1) for debugging
   - Disable legacy PIC, enable APIC
   - Load IDT with exception/interrupt handlers
   - Enable interrupts and start LAPIC timer
   - Process Limine responses (framebuffer, memory map)
   - Initialize page tables and heap
   - Initialize graphics console
   - Initialize VFS with ramdisk
   - Load and execute init binary
   - Enter idle loop

See [`docs/BOOT_PROCESS.md`](docs/BOOT_PROCESS.md) for more details.

## Code Style Guidelines

### Rust Conventions

Serix follows specific coding conventions that differ from standard Rust in some ways:

#### Formatting Rules

- **Tabs, not spaces**: Configured in `.rustfmt.toml` (`hard_tabs = true`, `tab_spaces = 8`)
- **100-character line width**: `max_width = 100`
- **Run `cargo fmt` before committing**: Ensures consistent formatting

#### Comment Style

- **C-style block comments for functions**:

  ```rust
  /*
   * Initializes the kernel heap allocator.
   * Must be called before any heap allocations.
   */
  pub fn init_heap() {
      // ...
  }
  ```

- **Line comments for inline explanations**:

  ```rust
  // Disable legacy PIC before enabling APIC
  unsafe { pic_disable(); }
  ```

#### Naming Conventions

- **Subsystem crates**: Lowercase, single word (`apic`, `idt`, `vfs`)
- **Syscall prefix**: All syscalls start with `serix_` (e.g., `serix_write`, `serix_exit`)
- **Interrupt handlers**: Suffix with `_handler` (e.g., `timer_interrupt_handler`)

#### Rust Patterns

- **`#![no_std]` everywhere**: This is a kernel environment, no standard library
- **`extern crate alloc`**: Use after heap initialization for `Vec`, `Box`, `String`
- **`unsafe` blocks**: Common for hardware access (I/O ports, MSRs, MMIO)
  - Always document why `unsafe` is needed
  - Keep unsafe blocks as small as possible
- **`static` + `Once`/`Mutex`**: Pattern for global state
- **Error handling**: Use `Result` types where possible

### Memory Safety

- **Use `x86_64::PhysAddr` and `x86_64::VirtAddr`** for addresses
- **Use `FrameAllocator` trait** for frame allocation
- **Use `x86_64::structures::paging::Mapper`** for page table access
- **Always use HHDM offset from Limine** - never hardcode physical memory offsets

### Common Pitfalls to Avoid

1. **Never use heap before `init_heap()`**: `Vec`, `Box`, `String` require initialized heap
2. **Never enable interrupts before IDT is loaded**: Will cause triple fault
3. **Always initialize serial console first**: Essential for debugging
4. **Always signal EOI to APIC**: All interrupt handlers must do this
5. **Check Limine responses**: Ensure they are `Some` before accessing

### Debug Output

- **Use `serial_println!` for debug output**: Primary debugging mechanism
- **Use `fb_println!` for framebuffer output**: After graphics initialization
- **Checkpoint pattern**: Use descriptive checkpoints during initialization

  ```rust
  serial_println!("[CHECKPOINT] Initializing APIC");
  ```

## Building and Testing

### Building the Project

```bash
# Full build with ISO
make iso

# Build kernel only (faster for iteration)
cargo build --manifest-path kernel/Cargo.toml --target x86_64-unknown-none
```

### Testing in QEMU

```bash
# Standard run
make run

# Run with debug output (interrupts, CPU resets)
qemu-system-x86_64 -cdrom serix.iso -serial stdio -m 4G \
    -d int,cpu_reset -no-reboot
```

### Debugging

- **Serial output**: Primary debugging mechanism (QEMU redirects to stdio with `-serial stdio`)
- **Triple fault**: Usually means:
  - Stack overflow
  - Invalid page table access
  - Exception before IDT loaded
  - Heap allocation before `init_heap()`
- **QEMU monitor**: Press `Ctrl-A C` to access QEMU monitor

### No Automated Tests Yet

The kernel is currently validated by:

- Booting in QEMU
- Verifying serial console output shows initialization messages
- Checking blue framebuffer appears
- Testing keyboard and timer interrupts work

Automated testing infrastructure is planned for the future (see roadmap).

## Commit Guidelines

### Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

#### Type

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style changes (formatting, no logic changes)
- **refactor**: Code refactoring (no functional changes)
- **perf**: Performance improvements
- **test**: Adding or updating tests
- **build**: Build system changes
- **ci**: CI/CD changes
- **chore**: Other changes (dependencies, tooling)

#### Scope

The affected subsystem: `kernel`, `memory`, `apic`, `idt`, `graphics`, `hal`, `task`, etc.

#### Examples

```
feat(apic): add LAPIC timer interrupt handler

Implemented periodic timer interrupt at vector 49.
Timer fires at approximately 625 Hz.

Closes #42
```

```
fix(memory): prevent heap allocation before init

Added assertion to catch heap usage before init_heap() is called.
This prevents silent corruption and triple faults.

Fixes #38
```

```
docs(contributing): add comprehensive contributor guide

Created CONTRIBUTING.md with code style guidelines,
build instructions, and commit message format.
```

### Best Practices

- **Keep commits atomic**: One logical change per commit
- **Write descriptive messages**: Explain *why*, not just *what*
- **Reference issues**: Use `Fixes #123` or `Closes #456`
- **Test before committing**: Ensure code builds and boots in QEMU

## Pull Request Process

### Before Submitting

1. **Create a feature branch**:

   ```bash
   git checkout -b feature/my-new-feature
   ```

2. **Make your changes** following the code style guidelines

3. **Format and lint**:

   ```bash
   cargo fmt
   cargo clippy --target x86_64-unknown-none
   ```

4. **Test thoroughly**:

   ```bash
   make run
   # Verify boot succeeds and your changes work as expected
   ```

5. **Commit with descriptive messages** (see Commit Guidelines)

6. **Update documentation** if needed:
   - Update README.md for user-facing changes
   - Update relevant docs/ files for architecture changes
   - Add comments for complex code

### Submitting the PR

1. **Push to your fork**:

   ```bash
   git push origin feature/my-new-feature
   ```

2. **Create Pull Request** on GitHub

3. **Fill out the PR template** completely:
   - Describe what the PR does
   - Reference related issues
   - List testing performed
   - Note any breaking changes

4. **Ensure CI passes** (when available)

5. **Address review feedback** promptly

### PR Review Process

- Maintainers will review your PR
- Reviews may request changes for:
  - Code style compliance
  - Memory safety concerns
  - Architecture consistency
  - Documentation completeness
- Be responsive to feedback and questions
- Once approved, a maintainer will merge your PR

### After Merging

1. **Update your local repository**:

   ```bash
   git checkout main
   git pull upstream main
   ```

2. **Delete your feature branch** (optional):

   ```bash
   git branch -d feature/my-new-feature
   ```

## Areas to Contribute

Based on the [roadmap](docs/ROADMAP.md), here are areas where contributions are welcome:

### High Priority (Phase 3 - Current Focus)

- **VirtIO Block Driver**: Disk access via MMIO
- **Chroma VFS**: RAM disk implementation, `/dev/console` node
- **Pulse IPC Core**: Inter-process message passing
- **Preemptive Scheduling**: Timer-triggered context switches

### Medium Priority (Phase 4-5)

- **Linux ABI Layer**: ELF loader improvements, `libc` stubs
- **Threading Support**: `pthread_create`/`join` implementation
- **Filesystem Operations**: POSIX file API
- **Scheduler Tuning**: WFQ implementation
- **IOMMU Protection**: DMA memory isolation

### Always Welcome

- **Documentation**: Improve existing docs or add new tutorials
- **Bug Fixes**: Fix issues in existing code
- **Testing**: Add test infrastructure
- **Examples**: Create example userspace programs
- **Code Cleanup**: Improve code quality and readability

### Not Ready Yet

These areas require more groundwork before accepting contributions:

- SMP (multi-core) support - foundation not complete
- Networking stack - driver layer needed first
- GUI framework - graphics primitives too basic

## Community and Communication

### Getting Help

- **GitHub Issues**: For bug reports and feature requests
- **GitHub Discussions**: For questions and general discussion
- **Documentation**: Check the [`docs/`](docs/) folder first

### Asking Questions

When asking for help:

- Provide context about what you're trying to do
- Include error messages and logs
- Describe what you've already tried
- Share minimal reproducible examples if possible

### Reporting Bugs

See our [bug report template](.github/ISSUE_TEMPLATE/bug_report.md) for details on what information to include.

### Suggesting Features

See our [feature request template](.github/ISSUE_TEMPLATE/feature_request.md) to propose new features.

## License

By contributing to Serix, you agree that your contributions will be licensed under the [GNU General Public License v3.0](LICENSE).

All contributions must:

- Be your original work or properly attributed
- Not contain proprietary or copyrighted code without permission
- Comply with the GPLv3 license terms

---

## Thank You!

Your contributions help make Serix better for everyone. Whether you're fixing a typo, adding a feature, or improving documentation, we appreciate your effort and time.

If you have questions about contributing, feel free to open an issue for discussion.

Happy hacking! 🦀🚀
