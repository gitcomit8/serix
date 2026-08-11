# Phase 6.2+ Completion Plan

## Summary

Complete Phase 6.2–6.4 and the Phase 6 security review. Phase 7 is out of scope, but this plan defines the security contracts Phase 7 must consume.

Decisions:

- Replace raw-ID authorization with capability-aware syscall ABIs.
- Use C-space slot plus generation for ordinary syscalls.
- Reserve 128-bit handles for explicit capability transfer.
- Add a minimal LES authorization bridge for the existing filesystem syscalls.
- Parse policy in a privileged Ring-3 policy service.
- Defer full `clone()`/`execve()` lifecycle implementation.
- Implement synchronous revocation now; require Phase 7.3 to prove cross-CPU acknowledgement.
- Require host tests plus adversarial QEMU validation.

## Implementation Changes

### 6.2 — Syscall and IPC enforcement

- Add one syscall authorization metadata table covering every dispatcher arm.
- Replace raw port IDs, FD lookups, and physical/object IDs as authorization inputs with `(cspace_slot, generation)` references.
- Route every protected operation through `CapabilityStore::validate()`.
- Gate file read/write/seek/close/dup, VFS mutation, mmap/munmap, process controls, IPC send/receive/notify, port creation/control, and format/power operations.
- Validate before user-buffer copies or any observable mutation.
- Make `send_kernel()` private or require an unforgeable kernel-only token.
- Update `ulib` wrappers and syscall documentation to match the replacement ABI.
- Ensure denied operations leave queues, files, VMAs, scheduler state, and notification state unchanged.

### 6.3 — Delegation, revocation, expiry, and audit

- Complete strict subset checks for rights, object scope, expiry, and delegation depth.
- Add explicit grant and revoke interfaces using validated parent capabilities.
- Defer `clone()`/`execve()` implementation, but document the required inheritance and close-on-exec contract for the future process phase.
- Keep revocation lock-serialized and transitive in Phase 6.
- Define the epoch/acknowledgement interface required by Phase 7.3 before any per-CPU validation cache is introduced.
- Add bounded audit records for grants, denials, revocation, expiry, policy reload, and capability transfer.
- Expose `/proc/serix/cap-audit` with cursor, overflow, privilege, and redaction semantics.

### 6.4 — Minimal LES authorization bridge

- Add the smallest LES/bridge boundary needed for current `open`, `access`, `chmod`, `chown`, directory mutation, and path-based file operations.
- Resolve paths to canonical VFS identities before policy evaluation.
- Implement default-deny rules with deterministic precedence: canonical path, most-specific rule, UID/GID specificity, then reject ambiguous ties.
- Translate POSIX flags into minimal capability rights.
- Have the policy service return validated immutable policy snapshots; the kernel mints time-bounded capabilities tagged with policy generation.
- Make policy reload transactional and auditable.
- Preserve the previous policy after malformed reloads.
- Map native capability failures to the documented POSIX errno contract.

### Phase 7 dependency handoff

Document interfaces required by later hardware work:

- device, MMIO, IRQ, and DMA/IOMMU capability kinds;
- restricted driver domains;
- interrupt ownership and delivery;
- revocation during driver restart;
- cross-CPU revocation acknowledgement;
- capability audit events for device assignment and DMA mapping.

Do not implement ACPI, SMP, IOMMU, NVMe, or XHCI in this plan.

## Test Plan

- Capability unit tests for absent, stale, foreign, expired, revoked, wrong-kind, wrong-object, insufficient-rights, and scope-escape cases.
- ABI tests for every syscall with no capability, wrong capability, insufficient rights, and valid capability.
- Side-effect tests proving denied operations do not mutate protected state.
- Delegation-tree tests proving transitive revocation and non-amplification.
- Audit-ring tests for overflow, cursor gaps, rate limiting, and handle redaction.
- Policy tests for path normalization, symlinks, mount crossings, rename-after-open, overlapping rules, UID/GID precedence, malformed reloads, and policy generations.
- Adversarial QEMU tests covering handle forgery, stale slot reuse, raw-ID substitution, denial races, revocation during use, and audit visibility.
- QEMU multi-process tests verifying IPC and filesystem denial behavior end to end.
- Static review checks ensuring every syscall and public IPC entry has an authorization classification.

## Acceptance Criteria

Phase 6 is complete when:

- Every protected public entry reaches one centralized authorization path.
- Raw numeric identifiers cannot authorize access by themselves.
- Delegation cannot amplify authority.
- Revocation is transitive and synchronous under the Phase 6 execution model.
- Policy reload is atomic, fail-safe, and auditable.
- The minimal LES bridge cannot bypass native capability validation.
- The adversarial QEMU suite passes.
- Phase 7 has documented, stable capability contracts for SMP, IOMMU, interrupt, and driver work.

## Assumptions

- The existing 6.1 capability structures are treated as the starting point, but their incomplete ownership and slot-generation behavior will be corrected where required.
- Full Linux syscall coverage remains Phase 5 work.
- Full `clone()`/`execve()` lifecycle support remains deferred until the process/LES phase.
