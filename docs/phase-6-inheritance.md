# Capability Inheritance Contracts

This document specifies the contracts for capability inheritance and closure
during `fork()`/`clone()` and `execve()` syscalls. These contracts are referenced
by future implementations of process cloning and execution replacement.

## 1. Close-on-Exec

When a task calls `execve()`, all capabilities with the `close_on_exec` flag
set are automatically closed (removed from the task's cspace).

### Flag Semantics

- **`close_on_exec = true`**: The capability is inherited across `fork()`/`clone()`
  but automatically closed on `execve()`. This is the default for delegated
  capabilities (not root capabilities).
- **`close_on_exec = false`**: The capability persists across both `fork()` and
  `execve()`. This is the default for root capabilities minted at bootstrap.

### Implementation Contract

```rust
impl CapabilitySpace {
    pub fn close_on_exec(&mut self) {
        for slot in 0..self.slots.len() {
            if self.slots[slot].close_on_exec {
                self.remove(slot);
            }
        }
    }
}
```

The kernel must call `cspace.close_on_exec()` as part of the `execve()` handler
before loading the new ELF image.

### Delegation Depth

When a capability is delegated (via `delegate()` or `grant()`), the resulting
child capability has `close_on_exec = false` by default. The granting code may
set it to `true` if the delegated capability should not survive exec.

Maximum delegation depth is **32 levels**. Each `delegate()` call increments
depth by 1. The `grant()` and `delegate_checked()` methods must reject
delegations that would exceed this depth.

## 2. Capability Inheritance on Fork/Clone

When a task calls `fork()` or `clone()`, the child process receives a **deep
clone** of the parent's `CapabilitySpace`:

### Clone Semantics

- The child gets a **new** `Vec<Slot>` — each slot is copied (new slot index
  may differ from the parent).
- Each `CapabilityId` in the child's cspace **remains the same** as the parent's
  (same internal store key).
- The child and parent share the same `CapabilityStore` records — the store is
  **not** cloned. Only the per-task cspace is cloned.
- After a fork, both processes can independently use the same capability
  handles (same `CapabilityId` → same object authority).

### Thread-Safety Contract

Since the store is shared between parent and child after fork, concurrent
access to the store's `Mutex` is expected and correct. The store's internal
locking ensures safe concurrent reads and writes.

## 3. Delegation Depth Limit

The maximum delegation depth is **32 levels**. This is enforced by:

1. `CapabilityStore::delegate_depth()` — walks the parent chain and returns
   the current depth.
2. `CapabilityStore::delegate_checked()` — calls `delegate_depth()` and
   rejects delegations that would exceed 32.

A delegation at depth 32 returns `DelegateError::DelegationDepthExhausted`.

### Rationale

32 levels is sufficient for practical capability-based security hierarchies:
- PID 1 (root) → user process (depth 1)
- User process → spawned child (depth 2)
- Child → delegated file cap (depth 3)

The limit prevents unbounded authority expansion through deep delegation chains
and protects against potential denial-of-service via extremely long chains.

## 4. Summary Table

| Event         | Action                                       | Default `close_on_exec` |
|---------------|----------------------------------------------|------------------------|
| `mint_root`   | No parent, depth 0                           | `false`               |
| `delegate`    | Child inherits parent's expiry, depth + 1    | `false`               |
| `grant`       | Same as delegate, inserted into cspace        | `false`               |
| `fork`/`clone`| Deep clone cspace, same CapabilityIds         | Preserved             |
| `execve`      | Close all `close_on_exec` slots               | —                     |

## 5. Future Work

- `execve()` syscall implementation (currently stubbed).
- `fork()`/`clone()` system calls with capability space cloning.
- Policy-controlled `close_on_exec` defaults via LES policy engine (Phase 6.4).
