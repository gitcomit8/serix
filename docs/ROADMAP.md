### **Serix Development Roadmap**  

*(Focus: x86_64)*

*(Start Date: July 2025)*

---

#### **Phase 1: Core Foundation** (July 2025)  

- [X] **Week 1-2**: Bootloader Integration

- Handoff from Firmware to kernel

- Memory map parsing

- Framebuffer initialization

- [X] **Week 3**: Memory Management Basics

- Page table initialization

- Physical memory allocator

- Heap allocation (`alloc` crate integration)

- [X] **Week 4**: Hardware Abstraction Layer

- CPU exception handling

- Interrupt controller (APIC) setup

- Basic timer driver (HPET/LAPIC)

---

#### **Phase 2: System Infrastructure** (August 2025)  

- [X] **Week 1**: Task Skeleton

- Task control block (TCB) structure

- Async task creation prototype

- [X] **Week 2**: Capability System Core

- Cryptographic handle implementation

- Capability storage/retrieval

- [X] **Week 3**: Basic Syscalls

- `serix_write` (debug console)

- `serix_exit`

- `serix_yield`

- [X] **Week 4**: Hybrid Core Detection

- Intel CPUID parsing

- P-core/E-core classification

---

#### **Phase 3: Hardware Integration** (September 2025)  

- [ ] **Week 1**: VirtIO Block Driver

- Disk access via MMIO

- Basic ATA commands

- [ ] **Week 2**: Chroma VFS Foundation

- RAM disk implementation

- `/dev/console` node

- [ ] **Week 3**: Pulse IPC Core

- Intra-process message passing

- Shared memory regions

- [ ] **Week 4**: Preemptive Scheduling

- Timer-triggered context switches

- Priority inheritance prototype

---

#### **Phase 4: POSIX Compatibility** (October 2025)  

- [ ] **Week 1**: Linux ABI Layer

- ELF loader with section mapping

- Basic `libc` stubs (open/read/write)

- [ ] **Week 2**: Threading Support

- `pthread_create`/`join`

- Scheduler class binding

- [ ] **Week 3**: Filesystem Operations

- POSIX file API (open/close/read/write)

- Capability-gated permissions

- [ ] **Week 4**: Signal Handling

- IPC-based delivery

- Default handlers (SIGKILL, SIGTERM)

---

#### **Phase 5: Optimization & Tools** (November 2025)  

- [ ] **Week 1**: Scheduler Tuning

- WFQ implementation

- Cache warmth tracking

- [ ] **Week 2**: IOMMU Protection

- DMA memory isolation

- Safe zero-copy IPC

- [ ] **Week 3**: Debugging Tools

- `serix-dbg` GDB stub

- Kernel panic backtraces

- [ ] **Week 4**: Performance Profiling

- Context switch timing

- Syscall latency metrics

---

#### **Phase 6: Release Preparation** (December 2025)  

- [ ] **Week 1**: Minimal Shell

- Command execution

- Built-in commands (ls, cat, echo)

- [ ] **Week 2**: CI/CD Pipeline

- Automated QEMU testing

- MIRI/Kani verification

- [ ] **Week 3**: Documentation

- Getting started guide

- Contributor handbook

- [ ] **Week 4**: v0.1.0 Release

- Public GitHub release

- Demo applications (HTTP server)

---

### **Key Milestones**  

- **Alpha Release**: October 15, 2025 (Boots to shell)

- **Beta Release**: November 30, 2025 (Runs `busybox`)

- **v1.0 Release**: December 31, 2025
