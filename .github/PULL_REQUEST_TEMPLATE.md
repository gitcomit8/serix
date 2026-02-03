## Description

<!-- Provide a clear and concise description of what this PR does -->

## Related Issue(s)

<!-- Link to related issues using "Fixes #123" or "Closes #456" syntax -->
<!-- Use "Related to #789" for non-closing references -->

Fixes #
Related to #

## Type of Change

<!-- Check all that apply -->

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] Code refactoring (no functional changes)
- [ ] Performance improvement
- [ ] Build/CI change
- [ ] Other (please describe):

## Subsystems Affected

<!-- Check all subsystems modified by this PR -->

- [ ] Kernel core (`kernel/`)
- [ ] Memory management (`memory/`)
- [ ] APIC/Interrupts (`apic/`, `idt/`)
- [ ] Graphics (`graphics/`)
- [ ] HAL (`hal/`)
- [ ] Task/Scheduling (`task/`)
- [ ] Capabilities (`capability/`)
- [ ] Drivers (`drivers/`)
- [ ] VFS (`vfs/`)
- [ ] IPC (`ipc/`)
- [ ] Loader/Userspace (`loader/`, `ulib/`)
- [ ] Build system (`Makefile`, `Cargo.toml`)
- [ ] Documentation

## Changes Made

<!-- Detailed list of changes -->

### Key Changes

-
-
-

### Technical Details

<!-- Explain the implementation approach, architectural decisions, or complex logic -->

## Testing Performed

<!-- Describe the testing you've done -->

### Build Testing

- [ ] `cargo fmt` passes (code is properly formatted)
- [ ] `cargo clippy --target x86_64-unknown-none` passes (no warnings)
- [ ] `cargo build --manifest-path kernel/Cargo.toml --target x86_64-unknown-none` succeeds
- [ ] `make iso` successfully creates bootable ISO

### Functional Testing

- [ ] Tested in QEMU (`make run`)
- [ ] Verified boot completes successfully
- [ ] Checked serial console output for errors
- [ ] Tested on real hardware (specify model):

### Test Scenarios

<!-- List specific test scenarios you executed -->

1.
2.
3.

### Test Results

<!-- Paste relevant test output, serial console logs, or screenshots -->

```
[Serial console output or test results]
```

## Compatibility

### Breaking Changes

<!-- If this PR includes breaking changes, describe them -->

- [ ] No breaking changes
- [ ] Breaking changes (describe below)

**Breaking change description:**
[If applicable, describe what breaks and how to migrate]

### Dependencies

- [ ] No new dependencies
- [ ] New Rust crates added (list below)
- [ ] Updated existing dependencies (list below)

**New/Updated dependencies:**
- `crate-name = "version"` - Reason:

### Hardware Requirements

- [ ] No new hardware requirements
- [ ] Requires specific hardware (describe):

## Documentation

<!-- Documentation updates included or needed -->

- [ ] Code comments added/updated
- [ ] Rustdoc comments added/updated
- [ ] README.md updated (if user-facing changes)
- [ ] Documentation in `docs/` updated
- [ ] CONTRIBUTING.md updated (if process changes)
- [ ] No documentation changes needed

**Documentation added/updated:**
-
-

## Performance Impact

<!-- Describe any performance implications -->

- [ ] No performance impact
- [ ] Performance improved (describe)
- [ ] Performance may be affected (describe)

**Performance notes:**
[Boot time changes, memory usage, CPU usage, etc.]

## Security Considerations

<!-- Security implications of this change -->

- [ ] No security implications
- [ ] Affects capability system (describe)
- [ ] Affects memory isolation (describe)
- [ ] Affects user/kernel boundary (describe)

**Security notes:**
[Describe any security considerations or reviews needed]

## Screenshots (if applicable)

<!-- For UI changes, framebuffer changes, or visual features -->
<!-- Attach screenshots showing before/after or demonstrating the feature -->

## Checklist

<!-- Ensure all items are complete before requesting review -->

- [ ] I have read the [Contributing Guidelines](../CONTRIBUTING.md)
- [ ] My code follows the project's code style (tabs, 100-char width)
- [ ] I have performed a self-review of my code
- [ ] I have commented my code, particularly in hard-to-understand areas
- [ ] My changes generate no new warnings or errors
- [ ] I have tested my changes in QEMU
- [ ] I have updated relevant documentation
- [ ] My commits follow the project's commit message guidelines
- [ ] I have checked my code for common pitfalls:
  - [ ] No heap allocations before `init_heap()`
  - [ ] No interrupt enabling before IDT is loaded
  - [ ] Serial console initialized for debugging
  - [ ] APIC EOI signaled in interrupt handlers (if applicable)
  - [ ] Limine responses checked for `Some` before use
  - [ ] Memory addresses use proper types (`PhysAddr`/`VirtAddr`)

## Additional Context

<!-- Any other context, considerations, or information about this PR -->

## Reviewer Notes

<!-- Anything specific you want reviewers to focus on or be aware of -->

---

**For Maintainers:**
- [ ] CI/CD checks pass (when available)
- [ ] Code review completed
- [ ] Documentation reviewed
- [ ] Ready to merge
