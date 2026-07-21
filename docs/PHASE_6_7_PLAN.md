# Phase 6 & Phase 7 Completion Plan

**Current State:** v0.0.6
**Target:** Complete Phase 6 (Security Bridge & Capability Enforcement) and Phase 7 (Hardware Enablement)

---

## Purpose and Completion Standard

Phase 6 makes the capability model an enforced security boundary instead of a collection of capability objects. Phase 7 supplies the platform mechanisms needed to run that boundary safely and efficiently on real multiprocessor hardware: authoritative firmware topology, inter-processor coordination, DMA isolation, power control, and two isolated high-performance device drivers.

The phases must be delivered in dependency order. A driver must not receive an MMIO or DMA capability before the capability authority can constrain it; a shared page-table update must not be enabled on multiple CPUs before TLB shootdown exists; and an IOMMU must be programmed before bus mastering is enabled for an untrusted Ring 3 driver.

This document is deliberately an implementation plan rather than a promise that all existing scaffolding is complete. Several relevant modules already contain prototypes (for example, IPC send-side checks, notification-port syscalls, LAPIC/SMP helpers, PCI enumeration, and per-CPU fields). They are useful starting points, but do not yet meet the invariants below.

### Required Security and Platform Invariants

1. **Deny by default.** A Ring 3 request that reaches a protected object must carry a capability owned by the calling task and valid for the requested operation. Absence, type mismatch, expiry, revocation, or wrong object returns `EPERM` without touching the object.
2. **No ambient authority.** Raw numeric identifiers (port IDs, FDs, paths, physical addresses, PCI BDFs) are lookup keys, not authorization. They cannot authorize an operation by themselves.
3. **Delegation never amplifies rights.** A child capability is a strict subset of its parent’s type, object scope, permissions, lifetime, and delegation depth.
4. **Revocation is transitive and race-safe.** Once revocation returns, every descendant is unusable at all syscall and IPC gates, including a capability concurrently being used by another CPU.
5. **Kernel-only bypasses are narrow and auditable.** `send_kernel()` and hardware bootstrap paths remain trusted internal interfaces; they must not be reachable through user-controlled syscall parameters.
6. **Firmware is input, not authority.** ACPI tables and PCI configuration data are parsed with length, checksum, range, and overflow validation. A malformed table must disable the dependent feature, not corrupt kernel state.
7. **DMA is denied until mapped.** A device may DMA only into frames in its active domain. The driver cannot manufacture host-physical addresses or map another device’s frames.
8. **Cross-CPU updates have an acknowledgement protocol.** No page-table, capability-epoch, or scheduler ownership transition relies on best-effort IPIs.
9. **Interrupt ownership is explicit.** Each external vector has one registered owner, a documented acknowledgement path, and a capability-backed delivery route to Ring 3.
10. **Hardware failure is contained.** Timeouts, controller resets, IOMMU faults, AP startup failures, and device removal leave the system in a defined state and generate observable diagnostics.

### Delivery Order

```text
6.1 capability semantics and C-space
        ├── 6.2 syscall/IPC gate coverage
        ├── 6.3 delegation, expiry, revocation, audit
        └── 6.4 LES policy bridge
                 └── 7.6 Ring-3 driver capability contracts

7.1 ACPI + CPU topology
        └── 7.2 AP bootstrap + per-CPU scheduler state
                 └── 7.3 IPIs / TLB shootdown
                         ├── 7.4 IOMMU
                         │      └── 7.6 NVMe
                         └── 7.7 XHCI
7.5 power management is independent after ACPI parsing, but is safest after SMP idle is stable.
```

### Suggested Milestones

| Milestone | Scope | Exit condition |
|---|---|---|
| M6-A | Capability authority | Every task has a C-space and capability lookup/validation is deterministic in unit tests. |
| M6-B | Enforcement and audit | All public syscall and IPC paths have a gate; denial and grant records are observable. |
| M6-C | LES bridge | File-related POSIX requests are translated by policy into least-privilege capabilities. |
| M7-A | Topology and SMP | MADT-driven CPU discovery, AP bring-up, per-CPU scheduler state, and safe IPI primitives work under QEMU `-smp`. |
| M7-B | DMA isolation | DMAR/IOMMU domain setup confines a deliberately malicious DMA test. |
| M7-C | Hardware drivers | NVMe and XHCI operate as restartable Ring 3 servers with MSI-X and IOMMU-backed memory. |
| M7-D | Power | FADT shutdown/reboot, idle-state selection, HWP bounds, and thermal policy are observable and fail safe. |

---

## Phase 6: Security Bridge & Capability Enforcement

### Current Implementation Status

**Already present, but incomplete:**

- `capability/` provides a global `CapabilityStore`, 128-bit `CapabilityHandle`, and basic `CapabilityType` variants.
- `task::TaskCB` carries a `cspace`; IPC `Port::send()` checks the sender C-space before queueing a normal send.
- IPC has trusted `send_kernel()`, direct-message scaffolding, notification state, and syscalls to create and signal ports.
- `kernel/src/syscall.rs` has distinct syscall groups for process, filesystem, memory, and IPC operations.
- `ulib/` exposes native Serix syscall wrappers; `ARCHITECTURE.md` establishes LES as the POSIX-to-capability bridge.

**Gaps to close:**

- The store is a global handle map, not an authority-aware per-task capability namespace; it has no generic `validate()` API, permission mask, ownership, parent link, expiry, or revocation state.
- `CapabilityHandle::generate()` uses `RDTSC` plus a small xorshift generator. That is not a cryptographic source and must not be described or relied upon as cryptographically random.
- Public syscall paths are not uniformly gated. In particular, ordinary file, task, memory, port lookup, notification, and lifecycle operations need centralized authorization decisions.
- Existing capability types do not express the requested file permissions, path scope, policy provenance, DMA/MMIO mappings, or delegation restrictions.
- There is no clone/exec inheritance policy, grant/revoke ABI, audit ring, policy parser, safe hot reload, or `/proc/serix/cap-audit` producer.
- Current IPC authorization is send-oriented; receive, notification, port creation/control, and kernel-to-server handoff rules must be defined explicitly.

### Capability Model Decisions

The implementation should use an object-capability model with an authoritative kernel registry and a per-task C-space. A handle is an opaque, unguessable reference; validation must always resolve it through the caller’s C-space and then the authoritative record. Never accept a handle merely because it occurs in the global store.

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rights(u64);

pub struct CapabilityRecord {
    pub object: ObjectId,
    pub kind: CapabilityKind,
    pub rights: Rights,
    pub owner: TaskId,
    pub parent: Option<CapabilityId>,
    pub expires_at: Option<Ticks>,
    pub delegable: Rights,
    pub max_delegate_depth: u8,
    pub revoked: bool,
    pub generation: u64,
    pub policy_generation: Option<u64>,
}

pub struct CapabilitySlot {
    pub id: CapabilityId,
    pub generation: u64,
    pub close_on_exec: bool,
}
```

`CapabilityId` should be an internal monotonically allocated identity, while a user-visible handle is a `(slot, generation, nonce)` representation. This permits stale-handle detection without exposing pointer values. The exact ABI may use a fixed 128-bit value, but it must include a generation check and be copied from userspace only with existing user-pointer validation.

Use a single rights vocabulary that can be composed by object kind:

| Object kind | Example rights |
|---|---|
| IPC port | `SEND`, `RECV`, `NOTIFY`, `CONTROL` |
| File or directory | `READ`, `WRITE`, `EXECUTE`, `APPEND`, `SEEK`, `CREATE`, `DELETE`, `METADATA` |
| Task | `SIGNAL`, `WAIT`, `INSPECT`, `SET_SCHED`, `DELEGATE` |
| Memory/VMA | `MAP`, `UNMAP`, `READ`, `WRITE`, `EXECUTE`, `DMA_MAP` |
| Device | `MMIO_MAP`, `DMA_MAP`, `IRQ_BIND`, `BUS_MASTER`, `RESET`, `POWER` |

Object-specific restrictions belong beside the bitset: an IPC capability carries a port object ID; a filesystem capability carries an inode/mount identity and canonical path scope; a DMA capability carries a device/domain and frame set; and a task capability carries a task or process-group identity. String paths are only policy inputs—runtime enforcement must use canonical VFS objects so rename and mount traversal cannot bypass a prefix rule.

### Task 6.1: Build the Authoritative Capability Store and Per-Task C-Spaces

**Why:** Phase 6 cannot be enforced while the global store treats a capability as an unscoped entry. The kernel needs one authority that can answer: *does this caller currently hold this right on this exact object?*

**Current State:**

- `capability/src/store.rs` holds `BTreeMap<[u8; 16], Capability>` under a mutex.
- `capability/src/types.rs` only represents task, memory, I/O device, FD, IPC-port, and notification kinds.
- `TaskCB.cspace` is used by IPC and is the appropriate integration point, but its lifecycle rules are not yet specified.

**Implementation Plan:**

1. Split the existing crate into explicit modules: `types`, `rights`, `object`, `cspace`, `store`, `validate`, `audit`, and `syscall`. Keep re-exports in `capability/src/lib.rs` so dependent crates do not import private layout types.
2. Replace the handle-keyed-only map with two registries:
   - an authoritative `CapabilityId -> CapabilityRecord` map, including parent/child links or a reverse-descendant index;
   - a `TaskId -> CapabilitySpace` map or a task-owned C-space containing `slot -> CapabilityId + generation`.
3. Define `ObjectId` as typed, not a bare `u64`. Use separate newtypes for `PortId`, `InodeId`, `DeviceId`, `FrameRangeId`, and `TaskId`; convert only at subsystem boundaries.
4. Define a `CapabilityRequest { object, required_rights, operation, caller }` and one non-allocating validation API:

   ```rust
   pub fn validate(
       &self,
       caller: TaskId,
       handle: CapabilityHandle,
       request: CapabilityRequest,
       now: Ticks,
   ) -> Result<ValidatedCapability, CapabilityError>;
   ```

   Validation must check, in order: C-space membership, slot generation, record existence, record generation, ownership/transfer semantics, revocation, expiry, object equality/scope containment, and rights containment. Return structured failures internally; convert only at the syscall boundary to `EPERM`, `EBADF`, `ENOENT`, or a deliberately documented error.
5. Make all state transitions explicit: `mint_root`, `insert_into_space`, `duplicate` (same authority; e.g. descriptor-style sharing), `delegate` (new restricted child), `remove_from_space`, `revoke`, `expire`, and `close_on_exec` cleanup.
6. Use a kernel entropy source appropriate to the platform. Prefer a health-checked `RDSEED`, then `RDRAND`, with failure handling and a boot-seeded CSPRNG fallback. Do not use `RDTSC` as the only entropy source. If secure entropy is unavailable, make this an explicit boot security state and disable untrusted capability handoff rather than silently claiming cryptographic properties.
7. Add lock-order documentation. A recommended order is `task table -> C-space -> capability store -> object lock`; IPC and VFS must follow it or use a short-lived validated token so they do not hold capability locks while sleeping.
8. Establish bootstrap authority intentionally: PID 1/Server Manager receives only the root capabilities needed to launch servers; each service receives a narrowed set. A kernel internal root is never serialized to Ring 3.

**Testing:**

- Unit test every failed validation predicate: absent slot, stale generation, foreign task, revoked record, expired record, wrong kind, wrong object, insufficient rights, and scope escape.
- Test that a valid handle copied into a second task fails until explicitly delegated.
- Test C-space slot reuse: close a handle, reuse its slot, then verify the old value cannot authorize the new capability.
- Use `cargo test -p capability` for pure logic where feasible; retain `#![no_std]` compatibility via an `alloc`-only implementation and test-only `std` feature if needed.

**Acceptance Criteria:**

- `CapabilityStore::validate()` is the only API used by public enforcement gates.
- Every task has a C-space by construction, including kernel-created tasks.
- No public handle refers directly to kernel memory or authorizes an object without a matching C-space slot.

### Task 6.2: Gate Every Syscall and IPC Entry

**Why:** Capability correctness is irrelevant if one public entry point reaches an object before the check. This task creates a reviewable enforcement matrix and removes ad hoc checks.

**Implementation Plan:**

1. Add a syscall metadata table in `kernel/src/syscall.rs` (or a small adjacent module) that identifies the operation class, required capability kind/rights, and whether an operation is bootstrap-only. The dispatcher should call a common `authorize_syscall()` helper before a protected subsystem operation.
2. Preserve syscalls that bootstrap capabilities—such as creating an initial port—but require a parent capability or a designated Server Manager bootstrap capability. Do not let arbitrary tasks mint authority by calling create APIs with no parent.
3. Pass a capability handle explicitly in new protected syscall ABIs. For legacy native interfaces where an FD is itself a capability slot, resolve the FD through the caller’s C-space before VFS access; do not treat a global `(task_id, fd)` table lookup as authorization.
4. Apply IPC checks symmetrically:
   - `send`: `SEND` on the target port;
   - `recv`/`recv_block`: `RECV` on the target port;
   - `notify`: `NOTIFY` on a notification port;
   - create/destroy/bind/configure: `CONTROL` on the controlling factory or port;
   - server-delivered hardware events: only the registered kernel interrupt dispatcher may produce them.
5. Validate before copying large user buffers, queueing messages, waking receivers, changing scheduling state, or mapping memory. A rejected request must have no externally visible partial effect.
6. Treat `send_kernel()` as a private kernel API. Keep it `pub(crate)` where possible; otherwise require a zero-sized trusted-token argument constructible only by `ipc`/kernel code. Audit every call site.
7. Gate process controls (`spawn`, `wait`, task inspection, future clone/exec), memory mapping, VFS mutation, device operations, format/reboot, and debug/proc interfaces. Standard input/output may be represented as initial file/console capabilities rather than special unauthenticated exceptions.
8. Add a table to the syscall documentation and keep it synchronized with `ulib`:

| Entry class | Required object/right | Expected denial |
|---|---|---|
| `OPEN`, read, write, seek, directory mutation | file/directory scope + relevant right | `EPERM` |
| `MMAP`, `MUNMAP` | VMA/file mapping right | `EPERM` |
| `SEND`, `RECV`, `NOTIFY` | port capability | `EPERM` |
| port creation/control | factory/control capability | `EPERM` |
| process lifecycle | task/process capability | `EPERM` |
| PCI/MMIO/DMA/IRQ | device capability | `EPERM` |
| reboot/format/power | platform-admin capability | `EPERM` |

**Testing:**

- Create a syscall denial matrix test harness in `ulib/examples/` or integration tests: invoke every protected syscall without a capability, with a wrong-kind capability, and with a correct one.
- Add regression tests for side effects: denied `write` does not change a file; denied `send` leaves the port queue untouched; denied `notify` does not wake a waiter; denied mapping leaves no VMA.
- Add a static review test or documented checklist that rejects a new syscall variant without an authorization classification.

**Acceptance Criteria:**

- A single list accounts for every `SYS_*` dispatcher arm and every public IPC operation.
- All unauthorized cases return an errno rather than panic or silently fall back to ambient access.
- `send_kernel()` has no Ring 3 call path.

### Task 6.3: Define Inheritance, Delegation, Revocation, Expiry, and Audit

**Why:** A usable capability system needs lifecycle semantics. Without them, `clone`, `execve`, service restart, and compromised-driver containment will all create authority leaks.

**Implementation Plan:**

1. Define `clone` behavior before implementing its ABI:
   - default: child receives duplicated references to the parent’s inheritable slots, with the same record and no additional rights;
   - `CLONE_FILES`: share the descriptor/C-space view only when its semantics are explicitly safe; otherwise preserve current descriptor behavior separately from capabilities;
   - `CLONE_THREAD`: share a process C-space only if task-group authority is intended; otherwise give each thread a C-space view with synchronized slots;
   - `execve`: close all slots marked `close_on_exec`; retain only explicitly inheritable slots.
2. Add `grant(parent_handle, target_task, reduced_spec)` and `revoke(handle)` syscalls or an IPC service interface. The grant request must supply a subset of rights, narrower/equal object scope, expiry no later than the parent, and decrementing delegation depth.
3. Store parent identity and maintain either a child list or revocation epoch tree. A simple, correct first version may mark descendants by traversal under the store lock; optimize later with generation/epoch checks only after proving that validation cannot race a grant or revoke.
4. Make revocation synchronous with enforcement: once `revoke()` reports success, validation on every CPU observes the revoked state. If per-CPU validation caches are introduced later, revocation must broadcast an IPI and wait for acknowledgements before returning.
5. Use monotonic ticks for expiry. Do not use wall-clock time until a trusted real-time source exists. Expired entries may be lazily reaped after a failed validation but must fail immediately.
6. Add a bounded, overwrite-on-full audit ring in the kernel. Each record should include monotonic sequence, ticks, CPU, caller task ID, operation, object kind/ID, requested rights, result, reason code, parent/policy generation, and a redacted handle fingerprint. Never emit the full capability value.
7. Expose a read-only `/proc/serix/cap-audit` producer through VFS. Define cursor/overflow semantics, privilege to read it, and a machine-parseable line or binary record format. Audit reads themselves need an inspection capability or root diagnostic authority.
8. Emit audit events for mint, grant, revoke, expiry, validation denial, privileged policy reload, IOMMU map/unmap, device assignment, and driver restart. Rate-limit repetitive denials so a hostile task cannot exhaust the ring.

**Testing:**

- Build a three-generation delegation tree. Verify each child loses access when the parent is revoked and that a sibling remains valid.
- Race a repeating validation loop against revoke on multiple QEMU CPUs; after the revoke acknowledgement, no successful validation is permitted.
- Verify `execve` closes non-inheritable capabilities and preserves explicitly inheritable ones.
- Fill the audit ring, verify sequence gaps/overflow reporting, and confirm no raw 128-bit handle appears in output.

**Acceptance Criteria:**

- Delegation is strictly monotonic and bounded.
- Revocation invalidates every descendant with a documented synchronization guarantee.
- The audit stream can explain every grant and denial without leaking reusable secrets.

### Task 6.4: Implement the POSIX-to-Capability Authorization Bridge

**Why:** LES must preserve expected POSIX behavior while keeping the kernel’s capability invariant. DAC is a policy input used to mint least-privilege authority; it is not a bypass around capability checks.

**Implementation Plan:**

1. Place the bridge at the LES syscall translation layer, before a POSIX request becomes a native VFS or IPC request. It must intercept at minimum `open`, `access`, `chmod`, and `chown`; include `openat`, `creat`, `unlink`, `mkdir`, `rmdir`, `rename`, `execve`, and directory traversal in the final review so path-based authority cannot leak through a cousin syscall.
2. Normalize path requests safely: resolve relative paths against the task’s CWD capability, reject embedded NULs, limit length/components, follow symlink policy deliberately, and obtain a canonical VFS identity. Policy decisions should use canonical mount/inode/path information, not an untrusted raw string.
3. Define a policy record format:

   ```toml
   [[rule]]
   uid = 1000
   gids = [1000, 100]
   path_prefix = "/home/alice"
   modes = ["read", "write", "append"]
   capability = "file"
   ttl_ms = 30000
   inheritable = false
   audit = true
   ```

   Matching must be deterministic: canonicalize, select the most-specific path prefix, then the most-specific UID/GID predicate, and deny on an ambiguous tie. Document default-deny and any bootstrap exceptions.
4. Translate POSIX open flags and mode checks into a minimal rights set. For example, `O_RDONLY` grants `READ|SEEK`; `O_APPEND` adds `APPEND` but does not imply arbitrary `WRITE`; directory traversal requires a distinct lookup/traverse right; `chmod` and `chown` require `METADATA` plus ownership policy.
5. Mint a time-bounded capability associated with the resolved object and policy generation, insert it into the caller’s C-space, then call the native operation through the normal gate. The native gate remains mandatory: a bridge bug must result in denial, not bypass.
6. Build an `alloc`-friendly TOML subset parser or run policy parsing in a privileged Ring 3 policy service, then hand a validated immutable rule set to the kernel. Avoid silently accepting a partial or malformed policy file.
7. Implement SIGHUP reload as transactional replacement: parse and validate a candidate, compile indexes, atomically publish a new generation, then retire the old rules only after readers finish. Failed reload retains the previous generation and produces an audit event. Existing minted capabilities retain their documented semantics; recommended default is that policy-generation revocation invalidates dynamic capabilities from replaced rules.
8. Ensure POSIX error mapping remains compatible: an authorization failure becomes `EACCES`/`EPERM` according to the LES contract, while native capability APIs use `EPERM`. Record both the POSIX decision and native validation result in the audit log.

**Testing:**

- Test overlapping path rules, UID/GID group selection, `..` normalization, symlinks, mount crossings, rename-after-open, and TOCTOU races.
- Test every requested open mode produces no more rights than necessary.
- Reload a valid policy while requests are running; then reload malformed TOML and verify the last valid policy remains active.
- Confirm a capability minted for `/a` cannot read `/ab`, and a handle obtained before a policy revocation fails if the selected policy requires invalidation.

**Acceptance Criteria:**

- No LES filesystem request reaches VFS without a policy decision and normal capability validation.
- Policy reload is atomic, observable, and fail-safe.
- DAC behavior is documented as translation policy rather than a second authorization path.

### Phase 6 Integration and Security Review

Before declaring Phase 6 complete, run a deliberate negative test pass:

- attempt handle forgery, stale-slot reuse, cross-task reuse, over-delegation, parent revocation bypass, expiry bypass, and raw-ID substitution;
- invoke every public syscall/IPC operation with no capability, wrong kind, wrong object, and insufficient rights;
- verify error paths do not mutate queues, VFS, VMAs, scheduling state, device state, or audit handle secrets;
- verify bootstrap authority is removed or narrowed once Server Manager has launched required services;
- review every `unsafe`, `pub`, `pub(crate)`, and `send_kernel` capability escape route.

---

## Phase 7: Hardware Enablement

### Current Implementation Status

**Already present, but incomplete:**

- `apic/` enables a legacy MMIO LAPIC, configures a timer, and provides basic I/O APIC register access.
- `apic/src/smp.rs` contains INIT/SIPI helper scaffolding and fixed-size AP readiness state.
- `kernel/src/gdt.rs` has a `PerCpuData` array and writes `KernelGsBase`; `task/` includes scheduler/per-CPU work.
- `drivers/src/pci.rs` can enumerate legacy PCI configuration space and read BARs/capabilities; it can enable bus mastering.
- Existing storage uses VirtIO, giving a useful block-I/O comparison point for the NVMe server.

**Gaps to close:**

- AP enumeration uses a QEMU-style heuristic; the ICR write path does not program destination correctly, and AP bootstrap, acknowledgement, timeout, and CPU-ID mapping are not production-safe.
- ACPI MADT, DMAR, and FADT parsing are absent. Current LAPIC/I/O APIC base addresses are defaults rather than firmware-authoritative mappings.
- The current per-CPU GDT/TSS and scheduler ownership are incomplete for SMP. `GS_BASE`/`KernelGsBase` and `swapgs` conventions must be made consistent before AP execution.
- No IPI mailbox/acknowledgement protocol, IOMMU implementation, ACPI power layer, NVMe driver, or XHCI driver exists.
- PCI BAR probing and bus mastering require hardening before being handed to Ring 3.

### Hardware Enablement Principles

1. **Bring up in emulation first, validate on hardware second.** QEMU provides repeatable topology, NVMe, xHCI, and fault injection; hardware validates firmware diversity and timing.
2. **One firmware parser.** Implement a shared ACPI table mapper/checker and typed table iterators; MADT, DMAR, and FADT must not each parse raw physical memory ad hoc.
3. **Use logical CPU IDs internally.** ACPI APIC IDs may be sparse or wider than eight bits. Maintain `LogicalCpuId <-> ApicId` mappings; never index an array directly by APIC ID.
4. **No unbounded hardware polling.** Every controller and AP state transition has a deadline, diagnostic, and recovery path.
5. **Allocate DMA memory explicitly.** Normal heap buffers are not automatically DMA-safe, physically contiguous, pinned, address-width compatible, or IOMMU mapped.

### Task 7.1: ACPI Discovery, MADT Parsing, and APIC Topology

**Why:** SMP, I/O APIC routing, IOMMU discovery, and power management all depend on trustworthy firmware information.

**Implementation Plan:**

1. Add an `acpi/` crate (or clearly bounded module) with `Rsdp`, `SdtHeader`, table mapper, checksum verifier, and table registry. Obtain RSDP from Limine/bootloader metadata if exposed; otherwise define the supported boot protocol handoff rather than scanning arbitrary memory once long mode is active.
2. Validate RSDP revision, length, both checksums where applicable, root table signature (`RSDT`/`XSDT`), entry alignment, physical address range, and every child SDT header/length/checksum. Map tables read-only through the physical-memory mapper and check overflow before slicing.
3. Parse MADT (`APIC`) entries into typed topology data:
   - local APIC and x2APIC processor entries, including enabled/online-capable flags;
   - I/O APIC ID, MMIO base, and global-system-interrupt base;
   - interrupt source overrides (polarity/trigger mode);
   - local APIC NMI/LINT configuration;
   - LAPIC address override.
4. Create `PlatformTopology` with a bounded CPU vector, IOAPIC vector, ISA IRQ-to-GSI translation table, and logical CPU mapping. Reject duplicate enabled APIC IDs and limits beyond configured `MAX_CPUS` with a clear degraded-mode diagnostic.
5. Update `apic/` to consume topology. I/O APIC routing must use GSI and the MADT flags, not assume ISA IRQ equals redirection entry and active-high edge triggering.
6. Decide x2APIC during early BSP initialization: check CPUID x2APIC support, verify policy/firmware compatibility, set IA32_APIC_BASE.x2APIC enable, and switch register/ICR access to MSRs. Keep an xAPIC fallback. Do not enable x2APIC after APs are live without a coordinated transition.
7. Expose a read-only diagnostic view (later `/proc/cpuinfo` can consume it) listing logical CPU, APIC ID, enabled state, BSP flag, APIC mode, I/O APICs, and IRQ overrides.

**Testing:**

- Unit test parser input with valid minimal tables plus bad checksum, short length, invalid entry length, duplicate APIC ID, sparse high APIC IDs, and malformed root pointers.
- Boot QEMU variants with one and multiple CPUs; compare discovered CPU count and I/O APIC/GSI data to QEMU output.
- Test xAPIC fallback and x2APIC branch independently using CPUID/feature abstraction tests.

**Acceptance Criteria:**

- No topology code guesses CPU count or assumes APIC IDs are contiguous.
- I/O APIC base and IRQ routing originate from validated MADT data.
- Malformed ACPI disables only dependent hardware features and leaves a serial diagnostic.

### Task 7.2: AP Bootstrap, Per-CPU State, and Scheduler Activation

**Why:** APs must reach a known long-mode state, own independent kernel state, and enter the scheduler without sharing BSP stacks, TSS, or run queues.

**Implementation Plan:**

1. Reserve a physically contiguous, identity-mapped trampoline below 1 MiB and aligned for SIPI vector requirements. Build it as a small assembly binary with an explicit parameter block rather than a Rust function pointer.
2. The trampoline must: start in real mode, load a temporary GDT, enter protected mode, enable PAE/long mode with a known page table, load a 64-bit GDT, load per-CPU stack, establish `GS`/`KernelGsBase` convention, and jump to a Rust `ap_entry(logical_cpu)` only after its parameter block is valid.
3. Program the ICR correctly for xAPIC and x2APIC. Send INIT and SIPI according to Intel timing requirements, wait for ICR delivery status where applicable, and target the APIC ID from `PlatformTopology`, not a loop index. The current helper must be replaced rather than extended around its heuristic.
4. Give each CPU: guarded kernel/IST stacks, GDT/TSS, IDT state as required by the architecture, `PerCpuData`, LAPIC timer configuration, idle task, run queue, current-task pointer, and online-state atomics aligned to cache lines.
5. Define the `swapgs` invariant once. `kernel/src/process.rs` already documents a `GS_BASE == 0` userspace convention; reconcile it with `KernelGsBase` initialization, interrupt entry, syscall entry, and context switch. Test both user-to-kernel and kernel-to-kernel interrupts on BSP and AP.
6. Use a staged online handshake: `Offline -> Starting -> EarlyInit -> SchedulerReady -> Online` with release/acquire ordering. BSP waits with a bounded timeout and records failures; an AP does not receive runnable work before `Online`.
7. Integrate per-CPU run queues only after CPU-local scheduler state is valid. Initial policy may pin tasks to the BSP and add explicit migration later; do not silently load-balance until task ownership, locking, and TLB behavior are correct.
8. Implement clean degradation: a failed AP remains offline; the BSP continues uniprocessor operation. Do not retry a failed AP indefinitely.

**Testing:**

- Boot QEMU with `-smp 1`, `-smp 2`, and `-smp 4`; log APIC/logical mapping and state transitions.
- Assert every online CPU has distinct stack ranges, TSS, `PerCpuData`, idle task, and run queue.
- Run user syscalls and timer interrupts from tasks scheduled on different CPUs; check `swapgs` and stack canaries.
- Force an AP timeout in a test build and verify the BSP reports it and remains usable.

**Acceptance Criteria:**

- All MADT-enabled QEMU CPUs reach `Online` or produce one bounded failure diagnostic.
- No CPU shares a mutable scheduler stack/TSS/run queue with another CPU.
- The system remains bootable when only the BSP is online.

### Task 7.3: IPI Primitives and TLB Shootdown

**Why:** SMP cannot safely change shared address spaces or recover from a CPU-local fatal error without reliable cross-CPU messages.

**Implementation Plan:**

1. Reserve named IPI vectors outside exception, legacy IRQ, LAPIC timer, and device MSI/MSI-X ranges: at minimum `TLB_SHOOTDOWN`, `SCHED_KICK`, `PANIC_STOP`, and optional `CALL_FUNCTION`. Centralize vector allocation so drivers cannot collide.
2. Define a per-CPU, cache-line-aligned IPI mailbox with operation, address-space ID/CR3, virtual range or full-flush flag, sequence number, and completion acknowledgement. The sender publishes the mailbox with Release ordering before sending the IPI; receiver reads with Acquire ordering.
3. Track active CPUs for each address space. A VMA/page-table update sends to exactly those CPUs currently running that CR3, excluding the local CPU after it flushes locally. Start with a conservative full-address-space flush if individual-page invalidation is not proven.
4. On the receiver, validate mailbox sequence, execute `invlpg` for a range or reload CR3/PCID-aware flush, publish acknowledgement, and issue EOI. Define bounded waiting and panic-safe behavior if a CPU does not acknowledge.
5. Implement scheduler kick as a separate operation: it only prompts a target CPU to re-evaluate its run queue and must never take remote run-queue locks from interrupt context. Use a pending bit to coalesce kicks.
6. Implement panic broadcast with a minimal interrupt-safe handler: mark CPU stopped, store diagnostic state, disable interrupts, and halt. The initiating CPU must still preserve serial output and avoid waiting forever for a dead CPU.
7. Add counters for sent/received IPIs, coalesced kicks, shootdown latency/timeouts, and stopped CPUs.

**Testing:**

- Map/unmap/protect a page in an address space that has run on two CPUs; repeatedly access it from both CPUs and verify no stale mapping survives the acknowledged shootdown.
- Stress concurrent VMA changes and task migration under QEMU `-smp 4`.
- Send scheduler kicks while idle and busy; verify no lost wakeup and no remote-lock deadlock.
- Invoke panic broadcast in a debug build and verify non-initiating CPUs stop without corrupting the report.

**Acceptance Criteria:**

- Every shared page-table change has a local flush and acknowledged remote flush before reclamation.
- IPIs are bounded, observable, and do not use user-controllable vector numbers.
- Scheduler wakeups use the kick primitive rather than polling.

### Task 7.4: IOMMU (Intel VT-d / AMD-Vi)

**Why:** Ring 3 drivers need DMA performance without unrestricted physical-memory access. IOMMU isolation is therefore a prerequisite for treating NVMe/XHCI servers as fault-contained processes.

**Implementation Plan:**

1. Define a vendor-neutral `Iommu` trait and common domain API (`create_domain`, `attach_device`, `map`, `unmap`, `invalidate`, `enable_interrupt_remapping`, `drain_faults`). Keep Intel VT-d and AMD-Vi table parsing/backends separate.
2. Parse ACPI DMAR first, including DRHD units, device scopes, register bases, segment IDs, interrupt-remapping flags, reserved-memory regions, and root-table flags. Reject overlapping/inconsistent units; use physical-address mapping with the same validation discipline as MADT.
3. For Intel VT-d, implement root table, context tables, 4-level second-level page tables, domain IDs, and queued/global invalidation. For AMD-Vi, implement the equivalent IVRS discovery, device table, page tables, command buffer, and IOTLB invalidation behind the shared trait. Do not claim platform support for a backend that has not been enabled and tested.
4. Create a DMA allocator interface: allocate pinned, page-aligned frames; return a driver-visible IOVA and a kernel-owned mapping token. Enforce device DMA address width from controller capabilities. Support contiguous allocations for queue rings and scatter/gather for data buffers.
5. Driver processes receive a device capability that permits only their BDF/domain, MMIO ranges, assigned IRQ vectors, and DMA mapping service. They never receive generic physical-memory mapping or arbitrary `enable_bus_master` authority.
6. Attach the device to a blocked/empty domain before enabling PCI bus mastering. Program mappings, invalidate IOTLB, then enable the driver. On process exit, reset the device if possible, disable bus mastering, detach or replace its domain, invalidate, revoke capability descendants, and notify Server Manager.
7. Enable interrupt remapping only after the IOMMU backend and CPU interrupt routing are stable. Allocate MSI/MSI-X vectors from the central allocator and bind a vector to the expected device/domain; reject arbitrary MSI address/data programming from Ring 3.
8. Drain IOMMU faults in an interrupt or polled early implementation, create a structured event, write audit data, and send a kernel-originated notification to Server Manager. Repeated faults should quarantine/reset the device rather than flood logs.

**Testing:**

- Unit test DMAR/IVRS parsing with corrupt lengths, unknown scopes, multiple remapping units, and reserved regions.
- In QEMU with an emulated IOMMU, map a test buffer and perform valid DMA; then attempt a DMA address outside the domain and verify the transaction faults without kernel memory corruption.
- Test driver crash/restart while DMA is in flight; confirm mappings are removed before frames are reused.
- Verify an MSI from an unassigned device/vector is rejected or never delivered to a driver endpoint.

**Acceptance Criteria:**

- Bus mastering for a Ring 3 driver is impossible without an active, restricted IOMMU domain.
- IOMMU faults are auditable and visible to Server Manager.
- Domain teardown prevents use-after-free DMA.

### Task 7.5: ACPI Power Management, Idle States, HWP, and Thermal Policy

**Why:** Correct shutdown/reset and efficient idle behavior are platform requirements. Performance controls must be advisory, bounded by firmware capabilities, and never compromise scheduling correctness.

**Implementation Plan:**

1. Parse FADT with version-aware field offsets and GAS (Generic Address Structure) access. Validate `PM1a_CNT_BLK`/length for S5 and `RESET_REG`/`RESET_VALUE` for reboot; use legacy fields only when the extended fields are absent and valid.
2. Add a small AML-independent S5 package strategy. FADT points to ACPI control registers, but sleep type values normally originate in AML `_S5`; either implement the minimal safe AML extraction needed for `_S5` or explicitly scope initial support to a platform-provided/validated S5 source. Do not invent `SLP_TYP` constants.
3. Implement `poweroff()` as: quiesce services/devices, flush or explicitly report unflushed storage, disable/route interrupts safely, write `SLP_TYP | SLP_EN` to PM1 control, wait boundedly, then use a documented fallback (QEMU debug exit only in test builds). Implement reboot via FADT reset register first, then a carefully documented architecture fallback.
4. Build a CPU idle governor. Query CPUID leaf `0x05`, validate MONITOR/MWAIT availability and platform policy, and select `HLT` by default. Use `MWAIT` only with safe monitored memory and supported C-state hints; fall back immediately on unsupported hardware/virtualization.
5. Add Intel HWP support behind CPUID/MSR feature checks. Read `IA32_HWP_CAPABILITIES`, set `IA32_PM_ENABLE`, and write bounded requests to `IA32_HWP_REQUEST` using caps-derived minimum/maximum. Expose a policy interface in terms of performance preference, not raw unvalidated MSR bitfields. Keep a no-HWP fallback.
6. Poll or receive thermal data from `IA32_THERM_STATUS` with calibrated cadence. Introduce hysteresis and rate limits: warning records telemetry; critical condition caps HWP and reduces scheduler admission of batch work; emergency condition performs a controlled shutdown only if hardware policy requires it. Do not busy-loop in an interrupt handler.
7. Expose per-CPU idle, HWP, thermal, and power-transition diagnostics for later `/proc` consumers and audit privileged power/performance changes.

**Testing:**

- Validate FADT/GAS parser with synthetic tables; test invalid widths, addresses, and reset register formats.
- Test power/reboot under QEMU with a detectable shutdown outcome; never use physical hardware as the first test of S5.
- Verify idle governor chooses `HLT` when MWAIT is absent/disabled and records its choice when enabled.
- Mock MSR access to validate HWP bounds and thermal hysteresis without writing host MSRs.

**Acceptance Criteria:**

- Shutdown/reboot are firmware-driven, bounded, and have test-only fallbacks isolated from production.
- Unsupported MWAIT/HWP/thermal features degrade to safe behavior.
- Thermal throttling is observable and cannot starve critical kernel work.

### Task 7.6: NVMe Storage Driver as a Ring 3 Server

**Why:** NVMe provides a modern, parallel storage path and validates the complete device-capability, DMA, IOMMU, MSI-X, IPC, and restart story.

**Implementation Plan:**

1. Define the server contract before register code. Server Manager assigns one PCI NVMe function, MMIO capability for BAR0, DMA domain capability, MSI-X IRQ capabilities, and a block-service IPC endpoint. The server returns completions and health events only through its endpoint.
2. Extend PCI discovery to classify NVMe (`class 0x01`, subclass `0x08`, programming interface `0x02`), validate BAR0 as a memory BAR, determine 32/64-bit size safely, and enumerate MSI-X capability/table/PBA without allowing userspace arbitrary config writes.
3. Map only the controller BAR pages into the server. The kernel owns PCI command register writes, bus master enable, reset authority, and MSI-X routing; the server requests these through device-capability operations.
4. Implement controller initialization precisely: disable controller, wait for `CSTS.RDY=0`, derive page/queue limits from `CAP`, allocate DMA-safe admin SQ/CQ, program `AQA`, `ASQ`, `ACQ`, set `CC`, and wait for `CSTS.RDY=1` with deadlines. On failure, reset/quarantine and report controller status.
5. Implement admin commands with command IDs and completion tracking: Identify Controller, Identify Active Namespace List/Namespace, Set Features (number of queues), Create I/O CQ, and Create I/O SQ. Parse values with endianness and bounds checks; do not assume namespace 1 or 512-byte sectors.
6. Create one I/O queue pair per active logical CPU only after SMP scheduler/interrupt affinity works. Each queue has DMA-safe rings, a submission lock appropriate to its ownership, phase-tag completion processing, doorbell stride from `CAP.DSTRD`, and an MSI-X vector/CPU affinity.
7. Implement read/write block requests with PRP1/PRP2 and PRP-list construction. Validate alignment, namespace LBA format, transfer limits (`MDTS`), user buffer pinning/copy policy, IOVA lifetime, and request cancellation/reset behavior. Start with bounce buffers if user-page pinning is not yet safe, then optimize.
8. On MSI-X completion, acknowledge/dispatch in the kernel, notify the server through its assigned event endpoint, drain CQ entries, complete IPC requests, and replenish queue state. Define interrupt moderation and polling fallback only after basic correctness.
9. Implement robust reset/recovery: stop new I/O, wait/cancel outstanding work, mask interrupts, disable bus mastering/DMA mappings, reset controller, recreate queues, and either recover or mark the namespace offline. Never reuse a request/IOMMU mapping until completion or reset proves it is inactive.
10. Expose the server through the existing block/VFS abstraction so FAT32/ext4 consumers need not know whether the backing device is VirtIO or NVMe.

**Testing:**

- QEMU NVMe boot: identify controller/namespace, read a known sector, write a scratch namespace, reboot, and verify persistence.
- Test transfers that cross page boundaries, require multiple PRP entries, exceed one command’s maximum, and complete out of order across queues.
- Inject controller-not-ready, invalid completion, DMA fault, MSI-X loss, and server restart; verify no kernel panic or stale DMA mapping.
- Stress parallel reads/writes on `-smp 4` and compare data hashes against the host image.

**Acceptance Criteria:**

- NVMe runs outside Ring 0 with BAR, DMA, and IRQ access limited to its assigned controller.
- Read/write correctness survives concurrent queues and recovery paths.
- A crashed NVMe server cannot DMA after its domain is torn down.

### Task 7.7: XHCI USB Driver as a Ring 3 Server

**Why:** XHCI exercises hot-plug, ring protocols, DMA, interrupts, and untrusted input parsing. USB HID provides the first broadly useful input path beyond legacy PS/2.

**Implementation Plan:**

1. Use the same server contract as NVMe: assigned xHCI PCI function, BAR0-only MMIO mapping, IOMMU domain, MSI/MSI-X vector(s), and input-service IPC endpoint. Device assignment and teardown are managed by Server Manager.
2. Discover xHCI (`class 0x0c`, subclass `0x03`, programming interface `0x30`); map capability, operational, runtime, and doorbell register regions only after validating offsets and lengths against `CAPLENGTH`/controller bounds.
3. Reset the controller by halting it, waiting for `HCHalted`, asserting `HCRST`, waiting for clear, and verifying readiness with timeouts. Read `HCSPARAMS`/`HCCPARAMS` to size structures and discover context size, scratchpad count, ports, and interrupters.
4. Allocate and initialize DCBAA, scratchpad pointer array/buffers, command ring, event ring segment table, event ring, transfer rings, and input/output contexts in DMA-safe memory. Honor alignment and cycle-bit requirements; initialize CRCR, DCBAAP, ERST, ERDP, and interrupter registers in specification order.
5. Implement command submission/completion with TRB cycle state and command completion events. Required early commands include Enable Slot, Address Device, Configure Endpoint, Evaluate Context, Stop Endpoint, Reset Endpoint, Disable Slot, and No-op for test synchronization.
6. Handle port-status-change events: read/clear change bits, debounce, reset/link-train the port, enumerate the default control endpoint, fetch descriptors with strict length limits, and configure only the interfaces/endpoints needed for HID initially. Disconnect must cancel rings, free IOVA mappings only after controller acknowledgement/reset, and revoke the child device object.
7. Implement HID boot protocol first for keyboard and mouse, then bounded report-descriptor parsing. Treat every descriptor/report as hostile: cap nesting, collection count, report size/count, string lengths, and transfer size; never use device-provided lengths for unchecked allocation or pointer arithmetic.
8. Translate parsed reports into a stable Serix input-event IPC schema with device ID, timestamp, event type/code/value, and overflow/reconnect markers. Keep keymap/layout policy out of the driver.
9. Recover from controller halt, event-ring overrun, command timeout, and malformed device behavior by quiescing the affected device or controller, cancelling pending transfers, remapping/resetting as needed, and reporting a health event. A device detach must never leave an interrupt endpoint referencing freed memory.

**Testing:**

- Boot QEMU with xHCI plus USB keyboard/mouse emulation; verify attach, input events, detach, and reattach.
- Test synthetic malformed descriptors and oversized reports in a parser harness.
- Exercise multiple rapid plug/unplug cycles, event ring wraparound, transfer cancellation, and server restart.
- Verify IOMMU fault/quarantine behavior with a deliberately invalid DMA transfer in a debug environment.

**Acceptance Criteria:**

- xHCI and HID parsing run in an isolated server with bounded resource use.
- Keyboard/mouse input events survive normal hot-plug and do not expose raw USB memory to consumers.
- Device removal and server crash leave no active DMA or dangling IRQ endpoint.

### Phase 7 Integration, Bring-Up Matrix, and Exit Checklist

Run Phase 7 incrementally. Each row must pass before depending on it:

| Environment | Required checks |
|---|---|
| QEMU, 1 CPU | ACPI parse, xAPIC fallback, FADT diagnostic, no regression in boot/VirtIO. |
| QEMU, 4 CPUs | MADT topology, AP online handshake, distinct per-CPU state, scheduler kick, TLB shootdown. |
| QEMU with IOMMU | DMAR parse, isolated test domain, fault event, safe teardown. |
| QEMU NVMe | admin identify, single queue I/O, multi-queue I/O, reset/restart, persistence. |
| QEMU xHCI | controller reset, HID enumeration, attach/detach, malformed input harness. |
| Physical Intel | x2APIC/HWP/VT-d feature-gated paths, thermal telemetry, FADT power controls. |
| Physical AMD | AMD-Vi feature-gated backend or documented unsupported/degraded status; no false enablement. |

Before Phase 7 is marked complete:

- [ ] ACPI parsers validate all tables and publish one platform topology.
- [ ] APIC IDs are not used as array indexes; every online CPU has isolated state.
- [ ] IPI senders wait for documented acknowledgement or enter a controlled failure path.
- [ ] IOMMU domain activation precedes device bus mastering; teardown precedes frame reuse.
- [ ] IRQ vectors are centrally allocated and bound to the expected device/domain.
- [ ] NVMe and xHCI execute in Ring 3 with only assigned capabilities.
- [ ] Every MMIO/DMA/interrupt/power action has an audit or diagnostic event.
- [ ] Feature absence and malformed firmware/device behavior degrade safely, without a kernel panic.

---

## Documentation Updates Required During Implementation

Update these documents as each task lands so the plan and implementation do not diverge:

- `docs/ROADMAP.md`: mark only tested sub-items complete and link to the implementation/verification evidence.
- `docs/ARCHITECTURE.md`: document the final C-space, LES policy, driver assignment, and IOMMU trust boundaries.
- `docs/KERNEL_API.md` and `ulib/`: publish capability-bearing syscall ABI, errno behavior, and clone/exec inheritance semantics together.
- `docs/INTERRUPT_HANDLING.md`: reserve IPI/MSI-X vector ranges, routing ownership, acknowledgement rules, and APIC mode behavior.
- `docs/MEMORY_LAYOUT.md`: document trampoline placement, per-CPU stacks, DMA allocator/IOMMU mappings, and TLB shootdown lifecycle.
- New focused documents as appropriate: `docs/CAPABILITY_MODEL.md`, `docs/ACPI_PLATFORM.md`, `docs/IOMMU.md`, `docs/NVME_DRIVER.md`, and `docs/XHCI_DRIVER.md`.

The roadmap should remain a concise status view. This document is the detailed execution and verification checklist for Phase 6 and Phase 7.
