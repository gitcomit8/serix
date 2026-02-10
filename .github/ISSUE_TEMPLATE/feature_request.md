---
name: Feature Request
about: Suggest a new feature or enhancement for Serix kernel
title: '[FEATURE] '
labels: enhancement
assignees: ''
---

## Feature Description

**A clear and concise description of the feature you'd like to see.**

## Motivation

**What problem does this feature solve? Why is it needed?**

- Use case: [Describe the use case or scenario]
- Current limitation: [What can't you do without this feature?]
- Benefit: [How would this improve Serix?]

## Proposed Solution

**Describe your proposed implementation or approach.**

### High-Level Design

[Outline the main components and how they would work]

### Technical Details

**Which subsystems would be affected?**
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
- [ ] Build system
- [ ] Documentation

**API Changes:**
```rust
// Example of proposed API or function signatures
pub fn new_feature_function() -> Result<(), Error> {
    // ...
}
```

**Configuration:**
[Any new configuration options or build flags needed]

## Alternatives Considered

**What other approaches did you consider?**

1. **Alternative 1:**
   - Description:
   - Pros:
   - Cons:

2. **Alternative 2:**
   - Description:
   - Pros:
   - Cons:

**Why is the proposed solution better?**

## Implementation Plan

**If you plan to implement this yourself, outline the steps:**

### Phase 1: Foundation
- [ ] Task 1
- [ ] Task 2

### Phase 2: Core Implementation
- [ ] Task 3
- [ ] Task 4

### Phase 3: Testing & Documentation
- [ ] Testing
- [ ] Documentation

**Estimated effort:** [e.g., Small (1-3 days), Medium (1-2 weeks), Large (1+ months)]

## Alignment with Roadmap

**Does this align with the [project roadmap](../../docs/ROADMAP.md)?**

- [ ] Yes, this is part of the current phase
- [ ] Yes, this is part of a future phase
- [ ] No, but it's a valuable addition
- [ ] Unsure

**Roadmap phase:** [e.g., Phase 3: Hardware Integration]

**Roadmap item:** [e.g., VirtIO Block Driver]

## Compatibility Considerations

**Breaking Changes:**
- [ ] This feature requires breaking changes
- [ ] This feature is backward compatible
- [ ] Not applicable

**If breaking changes are required, explain:**
[How would existing code need to be updated?]

**Dependencies:**
- New Rust crates needed: [e.g., `virtio-drivers = "0.1"`, or "none"]
- Hardware requirements: [e.g., "Requires IOMMU support", or "none"]
- Bootloader changes: [e.g., "Requires Limine v11.x", or "none"]

## Testing Strategy

**How would this feature be tested?**

- [ ] Manual testing in QEMU
- [ ] Real hardware testing required
- [ ] Automated tests (describe)
- [ ] Integration tests with existing subsystems

**Test scenarios:**
1. [Scenario 1]
2. [Scenario 2]

## Documentation Needs

**What documentation would need to be created/updated?**

- [ ] API documentation (rustdoc comments)
- [ ] Architecture documentation in `docs/`
- [ ] User guide / tutorial
- [ ] README.md updates
- [ ] CONTRIBUTING.md updates
- [ ] Code examples

## Performance Impact

**Expected performance characteristics:**

- Memory overhead: [e.g., "< 1KB per instance", "negligible", "unknown"]
- CPU overhead: [e.g., "Constant time O(1)", "depends on input size", "minimal"]
- Boot time impact: [e.g., "+ 10ms", "none", "to be measured"]

**Performance benchmarks needed:**
- [ ] Yes, benchmarking is critical
- [ ] No, performance impact is negligible
- [ ] Unsure

## Security Considerations

**Does this feature have security implications?**

- [ ] Yes, involves capability system
- [ ] Yes, involves memory isolation
- [ ] Yes, involves user/kernel boundary
- [ ] No security implications
- [ ] Needs security review

**Security concerns:**
[Describe any potential security issues and how they'd be addressed]

## Examples and References

**Similar features in other operating systems:**
- [OS Name]: [Brief description and reference link]
- [OS Name]: [Brief description and reference link]

**Relevant research papers or specifications:**
- [Paper/Spec title]: [Link]

**Code examples from other projects:**
```rust
// Example implementation from similar project
```

## Additional Context

**Add any other context, mockups, diagrams, or screenshots:**

[Any visual aids, diagrams, or additional information that helps explain the feature]

## Checklist

Before submitting, please ensure:
- [ ] I've searched existing issues to avoid duplicates
- [ ] I've provided a clear description of the feature
- [ ] I've explained the motivation and use case
- [ ] I've considered alternatives
- [ ] I've checked alignment with the roadmap
- [ ] I've read the [Contributing Guidelines](../../CONTRIBUTING.md)

## Willingness to Contribute

**Are you willing to implement this feature?**
- [ ] Yes, I'd like to implement this myself
- [ ] Yes, with guidance from maintainers
- [ ] No, but I can help with testing/review
- [ ] No, just suggesting the idea

**If yes, estimated availability:**
[e.g., "Can start next week, 10-15 hours per week available"]
