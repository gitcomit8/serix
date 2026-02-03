---
name: Bug Report
about: Report a bug or unexpected behavior in Serix kernel
title: '[BUG] '
labels: bug
assignees: ''
---

## Bug Description

**A clear and concise description of what the bug is.**

## Environment

**Hardware/Emulator:**
- [ ] QEMU
- [ ] Real Hardware (specify model/CPU)
- [ ] Other (specify):

**Serix Version:**
- Commit hash: [e.g., abc123def]
- Branch: [e.g., main, develop]

**Build Configuration:**
- Rust version: [output of `rustc --version`]
- Build command used: [e.g., `make run`, `cargo build --release`]
- Target: [e.g., x86_64-unknown-none]

**Host System:**
- OS: [e.g., Ubuntu 22.04, macOS 14.1, Windows 11]
- QEMU version (if applicable): [e.g., 8.1.0]

## Steps to Reproduce

**Detailed steps to reproduce the behavior:**

1. Clone repository from '...'
2. Build with '...'
3. Run with '...'
4. Observe '...'

**Minimal reproducible example (if applicable):**
```rust
// Paste minimal code that demonstrates the issue
```

## Expected Behavior

**A clear and concise description of what you expected to happen.**

## Actual Behavior

**What actually happened instead.**

## Logs and Output

**Serial Console Output:**
```
[Paste serial console output here - captured from QEMU -serial stdio]
```

**Error Messages:**
```
[Paste any error messages, panic output, or stack traces]
```

**Screenshots:**
If applicable, add screenshots showing:
- Framebuffer output
- Triple fault behavior
- QEMU window state

## Analysis

**What subsystem is affected?**
- [ ] Kernel initialization
- [ ] Memory management (`memory/`)
- [ ] APIC/Interrupts (`apic/`, `idt/`)
- [ ] Graphics/Framebuffer (`graphics/`)
- [ ] HAL (`hal/`)
- [ ] Task/Scheduling (`task/`)
- [ ] Capabilities (`capability/`)
- [ ] Drivers (`drivers/`)
- [ ] VFS (`vfs/`)
- [ ] IPC (`ipc/`)
- [ ] Loader (`loader/`)
- [ ] Build system (`Makefile`, `Cargo.toml`)
- [ ] Documentation
- [ ] Other (specify):

**When does this occur?**
- [ ] During kernel initialization
- [ ] After boot completes
- [ ] When specific hardware is present
- [ ] Only in QEMU
- [ ] Only on real hardware
- [ ] Intermittently/Race condition

**Potential cause (if known):**
[If you've debugged this or have a hypothesis about the cause]

## Debugging Information

**Have you tried debugging this?**
- [ ] Yes, using serial output
- [ ] Yes, using QEMU monitor
- [ ] Yes, using GDB
- [ ] No, haven't debugged yet

**Debug commands used (if any):**
```bash
# Example: qemu-system-x86_64 -cdrom serix.iso -serial stdio -d int,cpu_reset
```

**Relevant findings:**
[Any useful information discovered during debugging]

## Workaround

**Is there a workaround for this issue?**
[Describe any temporary workaround if you've found one]

## Additional Context

**Any other context about the problem:**
- Does this happen with specific configurations only?
- Did this work in a previous version/commit?
- Related to specific recent changes?
- Similar issues in other OS projects?

**Related Issues:**
- Relates to #[issue number]
- Duplicates #[issue number]

## Checklist

Before submitting, please ensure:
- [ ] I've searched existing issues to avoid duplicates
- [ ] I've provided all relevant environment information
- [ ] I've included serial console output or error messages
- [ ] I've specified clear steps to reproduce
- [ ] I've checked if this happens on latest `main` branch
- [ ] I've read the [Contributing Guidelines](../../CONTRIBUTING.md)
