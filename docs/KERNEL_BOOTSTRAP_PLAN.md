# Serix Kernel Bootstrap Plan: Building from Specification (C Implementation)

**Document Version:** 1.0  
**Created:** 2025-12-03  
**Target Architecture:** x86_64 (primary), RISC-V (future)  
**Language:** C (with minimal assembly for architecture-specific code)  
**Repository:** github.com/gitcomit8/serix

---

## Executive Summary

This document provides a comprehensive, phase-by-phase roadmap for building the Serix operating system kernel **from scratch** based purely on the Architecture Design Document (ADD.md). This plan does NOT assume any existing code exists - it starts from a blank slate and builds a complete microkernel in C.

**Specification Source:** docs/ADD.md (Architecture Design Document)

**Target System:**
- **Type:** Microkernel (hybrid scheduling)
- **Language:** C (ISO C11 or later, with GCC/Clang extensions where needed)
- **Architecture:** x86_64 long mode (64-bit)
- **Key Innovation:** Cryptographic capability system + async-native scheduler
- **End Goal:** POSIX-compatible microkernel with 87/112 syscalls, capable of running user applications

---

## Specification Summary (From ADD.md Analysis)

### Core Identity

| Attribute | Specification |
|-----------|---------------|
| **Type** | Microkernel (hybrid scheduling) |
| **Language** | C (specified as "kernel in C" in requirements) |
| **Targets** | x86_64 (w/ Intel Hybrid Core), RISC-V (future) |
| **Key Innovation** | Cryptographic capability system + async-native scheduler |
| **Security Model** | Zero-unsafe core principle (minimize unsafe C, extensive validation) |

### Finalized Subsystems (From ADD.md)

#### 1. Chronos Scheduler

**Scheduling Classes:**
```c
enum sched_class {
    SCHED_REALTIME,  // FIFO policy, priority 0-99
    SCHED_FAIR,      // Weighted policy, priority 100-139
    SCHED_BATCH,     // Idle policy, priority 140
    SCHED_ISO        // Isochronous (SCHED_DEADLINE)
};
```

**Performance Guarantees:**
- Context switch: ≤500ns (P-core)
- Cache warmth tracking via PMU
- Intel Hybrid Core support (latency-sensitive → P-core, background → E-core)

#### 2. Aegis Memory Manager

**Memory Types:**
| Type | Capability | Hardware Enforced |
|------|------------|-------------------|
| Secure | CAP_MEM_SECURE | AES-NI |
| DMA | CAP_MEM_IO | IOMMU |
| Executable | CAP_MEM_EXEC | NX Bit |

**Allocators:**
- Zone-based NUMA allocator
- SLAB for kernel objects

#### 3. Hermes Syscall Layer

**Calling Convention (Linux ABI compatible):**
```c
long syscall(
    uint64_t nr,     // RAX
    uint64_t arg1,   // RDI
    uint64_t arg2,   // RSI
    uint64_t arg3,   // RDX
    uint64_t arg4,   // R10
    uint64_t arg5,   // R8
    uint64_t arg6    // R9
);
```

**POSIX Coverage:** 87/112 syscalls implemented (Phase 1 target)

#### 4. Cerberus Capabilities

**Handle Structure:**
```c
struct capability {
    uint128_t id;              // XChaCha20 nonce
    uint32_t rights;           // Fine-grained permissions (bitflags)
    cap_origin_t origin;       // Delegation chain
    uint64_t expires;          // Optional temporal validity (cycle count)
};
```

**Revocation:** TLB shootdown + cache invalidation

#### 5. Pulse IPC

**Performance by Payload Size:**
| Payload Size | Latency | Method |
|--------------|---------|--------|
| <128B | 400ns | Register copy |
| 128B-4KB | 1.2μs | Shared memory |
| >4KB | 2.5μs | Page remapping |

### Performance Targets

| Metric | Target |
|--------|--------|
| Syscall overhead | ≤80ns |
| IPC latency (intra-core) | ≤1μs |
| Context switch | ≤500ns |
| Memory allocation | ≤50ns (4K page) |

### Hardware Abstraction Requirements

**x86_64 Support:**
| Feature | Implementation |
|---------|----------------|
| SMP | ✓ (APIC) |
| IOMMU | VT-d/AMD-Vi |
| Power Management | C-states + P-states |

**Driver Model:** VirtIO-only for Phase 1 (block, net, console)

---

## Design Principles and Constraints

### C Language Constraints

Since we're building in C (not Rust with its safety guarantees), we must be explicit about safety practices:

1. **Memory Safety:**
   - All pointers validated before dereference
   - Bounds checking on all array accesses
   - No use-after-free (careful object lifecycle management)
   - No double-free (clear ownership semantics in comments)

2. **Integer Safety:**
   - Check for overflow in arithmetic operations
   - Use sized types (`uint64_t`, not `long`)
   - Explicit casts with range validation

3. **Concurrency Safety:**
   - All shared data protected by locks or atomic operations
   - Lock ordering documented to prevent deadlock
   - Minimize critical sections

4. **Error Handling:**
   - All functions return error codes (no exceptions)
   - All error codes checked by callers
   - Use negative errno values for errors, non-negative for success

### Project Structure

```
serix/
├── boot/              # Bootloader interface and early init
│   ├── multiboot2.h   # Multiboot2 or Limine boot protocol
│   └── start.S        # Assembly entry point
├── kernel/            # Core kernel
│   ├── main.c         # Kernel main
│   ├── panic.c        # Kernel panic handler
│   └── printk.c       # Kernel logging
├── mm/                # Memory management
│   ├── pmm.c          # Physical memory manager
│   ├── vmm.c          # Virtual memory manager
│   ├── slab.c         # SLAB allocator
│   └── page.c         # Page table management
├── sched/             # Scheduler (Chronos)
│   ├── sched.c        # Core scheduler
│   ├── task.c         # Task management
│   ├── context.S      # Context switching (assembly)
│   └── hybrid.c       # Hybrid core detection
├── ipc/               # Inter-process communication (Pulse)
│   ├── msg.c          # Message passing
│   ├── shm.c          # Shared memory
│   └── cap_ipc.c      # Capability-based IPC
├── cap/               # Capability system (Cerberus)
│   ├── capability.c   # Core capability logic
│   ├── crypto.c       # XChaCha20 for capability IDs
│   └── revoke.c       # Capability revocation
├── syscall/           # System call interface (Hermes)
│   ├── syscall.S      # Syscall entry (assembly)
│   ├── table.c        # Syscall dispatch table
│   └── impl_*.c       # Syscall implementations
├── drivers/           # Device drivers
│   ├── virtio/        # VirtIO infrastructure
│   ├── console.c      # Console driver
│   └── timer.c        # Timer driver
├── fs/                # Filesystem (Chroma VFS)
│   ├── vfs.c          # VFS layer
│   ├── ramfs.c        # RAM filesystem
│   └── devfs.c        # /dev filesystem
├── arch/              # Architecture-specific code
│   └── x86_64/        # x86_64 support
│       ├── cpu.c      # CPU initialization
│       ├── idt.c      # Interrupt Descriptor Table
│       ├── apic.c     # APIC (Local + I/O)
│       ├── msr.c      # MSR access
│       └── iommu.c    # IOMMU (VT-d)
├── lib/               # Kernel library functions
│   ├── string.c       # String operations (memcpy, etc.)
│   ├── math.c         # Math operations
│   └── bitmap.c       # Bitmap operations
├── include/           # Header files
│   ├── kernel/        # Kernel headers
│   ├── mm/            # Memory management headers
│   ├── arch/          # Architecture headers
│   └── uapi/          # User API headers (syscall numbers, etc.)
├── tools/             # Build and development tools
│   ├── mkiso.sh       # Create bootable ISO
│   └── gdbinit        # GDB initialization script
├── docs/              # Documentation
│   ├── ADD.md         # Architecture Design Document (spec)
│   └── KERNEL_BOOTSTRAP_PLAN.md  # This document
├── Makefile           # Build system
└── linker.ld          # Kernel linker script
```

---

## Bootstrap Roadmap: Phase-by-Phase

### Phase 0: Project Setup and Minimal Boot (Weeks 1-2, 80 hours)

**Goal:** Establish project infrastructure and achieve first boot to a working kernel that prints "Hello, World!" to serial console.

#### Week 1: Build System and Boot Infrastructure (40 hours)

**Deliverables:**

1. **Build System Setup (8 hours)**
   - Create Makefile with targets: `kernel`, `iso`, `run`, `clean`
   - Set up cross-compiler toolchain (x86_64-elf-gcc)
   - Configure compiler flags:
     ```makefile
     CFLAGS = -std=c11 -ffreestanding -nostdlib -fno-builtin \
              -fno-stack-protector -mno-red-zone -mcmodel=kernel \
              -Wall -Wextra -Werror
     ```
   - Configure linker script for higher-half kernel (load at -2GB from top)

2. **Bootloader Integration (12 hours)**
   - Choose bootloader: Limine (modern, well-documented) or Multiboot2
   - Implement boot protocol structures
   - Parse bootloader info (memory map, framebuffer, modules)
   - File: `boot/boot.c`, `boot/limine.c`

3. **Assembly Entry Point (10 hours)**
   - Write `boot/start.S`:
     - Verify CPU is in long mode (64-bit)
     - Set up initial stack
     - Jump to C kernel main
   - Handle bootloader handoff (save bootloader info pointer)

4. **Kernel Main Skeleton (10 hours)**
   - Write `kernel/main.c`:
     ```c
     void kernel_main(void *boot_info) {
         // Initialize serial console
         serial_init();
         serial_puts("Serix kernel starting...\n");
         
         // Halt
         while (1) {
             asm volatile("hlt");
         }
     }
     ```
   - Implement `serial_init()` and `serial_puts()` for early debugging
   - File: `drivers/serial.c`

**Success Criteria:**
- Kernel compiles without warnings
- Kernel boots in QEMU and prints "Serix kernel starting..." to serial console
- Kernel doesn't triple fault or reboot loop

**Testing:**
```bash
make kernel    # Build kernel
make iso       # Create bootable ISO
make run       # Run in QEMU with: qemu-system-x86_64 -serial stdio -cdrom serix.iso
```

**Dependencies:** None (foundational)

**Risks:**
- Bootloader integration can be tricky (Mitigation: Use Limine examples, test incrementally)
- Cross-compiler setup varies by platform (Mitigation: Document setup for Linux, macOS)

---

#### Week 2: Basic Output and Memory Detection (40 hours)

**Deliverables:**

1. **Framebuffer Graphics Initialization (12 hours)**
   - Parse framebuffer info from bootloader
   - Implement pixel write function
   - Fill screen with solid color (e.g., blue) to confirm boot
   - File: `drivers/fb.c`
   - Functions: `fb_init()`, `fb_write_pixel()`, `fb_clear()`

2. **Basic Printf Implementation (12 hours)**
   - Implement `printk()` - kernel printf function
   - Support format specifiers: `%s`, `%d`, `%u`, `%x`, `%p`
   - Output to both serial and framebuffer
   - File: `kernel/printk.c`
   - Example:
     ```c
     printk("Memory: base=%p, size=%uMB\n", mem_base, mem_size_mb);
     ```

3. **Memory Map Parsing (16 hours)**
   - Parse memory map from bootloader
   - Identify usable RAM regions
   - Print memory map for debugging
   - File: `mm/memmap.c`
   - Data structure:
     ```c
     struct memory_region {
         uint64_t base;
         uint64_t length;
         uint32_t type;  // USABLE, RESERVED, ACPI, etc.
     };
     ```

**Success Criteria:**
- Framebuffer displays solid blue screen (visual confirmation of boot)
- `printk()` outputs formatted text to serial console
- Memory map printed and shows usable RAM regions

**Milestone: First Boot Complete** ✓ (Kernel boots and outputs diagnostics)

---

### Phase 1: Memory Management (Weeks 3-5, 120 hours)

**Goal:** Implement Aegis memory manager with physical memory allocation, virtual memory management, and heap allocation.

#### Week 3: Physical Memory Manager (40 hours)

**Deliverables:**

1. **Bitmap-Based Physical Frame Allocator (20 hours)**
   - Track free/used 4KB frames with bitmap
   - Functions:
     ```c
     void pmm_init(struct memory_region *regions, size_t count);
     void *pmm_alloc(void);           // Allocate one 4KB frame
     void pmm_free(void *frame);      // Free one frame
     void *pmm_alloc_contiguous(size_t count);  // Allocate contiguous frames
     ```
   - File: `mm/pmm.c`
   - Store bitmap in low memory (below kernel)

2. **Memory Statistics (10 hours)**
   - Track total memory, used memory, free memory
   - Functions:
     ```c
     size_t pmm_total_memory(void);
     size_t pmm_free_memory(void);
     size_t pmm_used_memory(void);
     ```
   - Print statistics during boot

3. **Testing and Validation (10 hours)**
   - Test: Allocate 1000 frames, free all, verify no leaks
   - Test: Allocate until out of memory, verify error handling
   - Test: Allocate contiguous, verify frames are adjacent

**Success Criteria:**
- Can allocate and free physical frames
- Memory statistics accurate
- No memory leaks after 10,000 allocate/free cycles

---

#### Week 4: Virtual Memory Manager (40 hours)

**Deliverables:**

1. **Page Table Management (25 hours)**
   - Implement 4-level paging (PML4 → PDP → PD → PT)
   - Functions:
     ```c
     void vmm_init(void);
     void *vmm_map_page(void *virt, void *phys, uint64_t flags);
     void vmm_unmap_page(void *virt);
     void *vmm_get_physical(void *virt);  // Translate virt to phys
     ```
   - File: `mm/vmm.c`
   - Flags: PRESENT, WRITABLE, USER, NX (no-execute)

2. **Higher-Half Kernel Mapping (10 hours)**
   - Map kernel at -2GB (0xFFFFFFFF80000000)
   - Map physical memory at offset (e.g., 0xFFFF800000000000)
   - Identity map first 4MB (for APIC MMIO, etc.)

3. **TLB Management (5 hours)**
   - Implement TLB invalidation:
     ```c
     void vmm_flush_tlb(void);           // Flush all
     void vmm_flush_tlb_single(void *virt);  // Flush one page
     ```
   - Use `invlpg` instruction

**Success Criteria:**
- Kernel runs in higher half
- Can map/unmap arbitrary pages
- TLB correctly invalidated after page table changes

---

#### Week 5: Heap Allocator (40 hours)

**Deliverables:**

1. **Basic kmalloc/kfree (20 hours)**
   - Implement simple first-fit allocator
   - Functions:
     ```c
     void *kmalloc(size_t size);
     void kfree(void *ptr);
     void *krealloc(void *ptr, size_t size);
     ```
   - File: `mm/heap.c`
   - Use linked list of free blocks initially

2. **SLAB Allocator for Fixed-Size Objects (15 hours)**
   - Implement SLAB for common kernel objects
   - Functions:
     ```c
     struct slab_cache *slab_create(size_t obj_size, const char *name);
     void *slab_alloc(struct slab_cache *cache);
     void slab_free(struct slab_cache *cache, void *obj);
     ```
   - File: `mm/slab.c`
   - Pre-create caches for: task structures, file descriptors, etc.

3. **Testing (5 hours)**
   - Test: Allocate various sizes, free in random order
   - Test: Allocate until out of memory
   - Test: Check for memory leaks (use statistics)

**Success Criteria:**
- `kmalloc()`/`kfree()` work reliably
- SLAB allocator faster than `kmalloc()` for fixed-size objects
- No fragmentation issues in stress test (1 hour of random alloc/free)

**Milestone: Memory Management Complete** 💾

---

### Phase 2: Interrupt Handling and Drivers (Weeks 6-8, 120 hours)

**Goal:** Implement interrupt infrastructure (IDT, APIC) and basic drivers (timer, keyboard).

#### Week 6: IDT and Exception Handling (40 hours)

**Deliverables:**

1. **Interrupt Descriptor Table Setup (15 hours)**
   - Create IDT with 256 entries
   - Implement exception handlers (0-31):
     - Divide by zero, page fault, general protection, double fault, etc.
   - File: `arch/x86_64/idt.c`, `arch/x86_64/exceptions.S`
   - Use interrupt gates (disable interrupts during handler)

2. **Exception Handler Implementation (15 hours)**
   - Print detailed error information:
     ```c
     void exception_handler(uint64_t vector, uint64_t error_code,
                           struct interrupt_frame *frame) {
         printk("EXCEPTION: vector=%lu, error=%lx, rip=%p\n",
                vector, error_code, frame->rip);
         // Print register dump
         // Print stack trace
         kernel_panic("Unhandled exception");
     }
     ```
   - File: `kernel/panic.c`
   - Implement stack unwinding for better error messages

3. **Testing (10 hours)**
   - Deliberately trigger each exception (divide by zero, invalid opcode, page fault)
   - Verify detailed error output

**Success Criteria:**
- All 32 exceptions have handlers
- Exceptions print useful debug info (registers, stack trace)
- Kernel doesn't triple fault on exception

---

#### Week 7: APIC and Timer (40 hours)

**Deliverables:**

1. **Local APIC Initialization (15 hours)**
   - Detect APIC via CPUID
   - Enable APIC (MSR 0x1B)
   - Configure SVR (Spurious Interrupt Vector Register)
   - File: `arch/x86_64/apic.c`
   - Functions:
     ```c
     void apic_init(void);
     void apic_send_eoi(void);
     void apic_send_ipi(uint8_t dest, uint8_t vector);
     ```

2. **I/O APIC Configuration (10 hours)**
   - Parse ACPI tables to find I/O APIC address
   - Configure redirection table entries
   - Map IRQs to vectors (e.g., IRQ1 → vector 33 for keyboard)
   - File: `arch/x86_64/ioapic.c`

3. **Timer Driver (15 hours)**
   - Configure LAPIC timer for periodic interrupts
   - Frequency: 100 Hz initially (10ms per tick)
   - Implement tick counter:
     ```c
     volatile uint64_t system_ticks = 0;
     
     void timer_interrupt_handler(void) {
         system_ticks++;
         apic_send_eoi();
     }
     ```
   - File: `drivers/timer.c`
   - Functions: `timer_init()`, `timer_get_ticks()`, `timer_sleep(uint64_t ms)`

**Success Criteria:**
- Timer interrupts fire at 100 Hz
- System tick counter increments correctly
- `timer_sleep()` accurately delays

---

#### Week 8: Keyboard Driver (40 hours)

**Deliverables:**

1. **PS/2 Keyboard Driver (25 hours)**
   - Read scancodes from port 0x60
   - Translate scancodes to ASCII (US layout)
   - Implement keyboard buffer (circular buffer)
   - File: `drivers/keyboard.c`
   - Functions:
     ```c
     void keyboard_init(void);
     char keyboard_getchar(void);  // Blocking read
     bool keyboard_available(void);
     ```

2. **Console Input Integration (10 hours)**
   - Echo typed characters to console
   - Handle special keys (Enter, Backspace, etc.)
   - Implement line editing (basic)

3. **Testing (5 hours)**
   - Test: Type characters, verify they appear on screen
   - Test: Backspace deletes characters
   - Test: Enter submits input

**Success Criteria:**
- Keyboard input works reliably
- Typed characters echo to console
- Can read lines of text from keyboard

**Milestone: Interrupts and Basic I/O Working** ⌨️

---

### Phase 3: Scheduler (Chronos) (Weeks 9-11, 120 hours)

**Goal:** Implement Chronos scheduler with multiple scheduling classes and preemptive multitasking.

#### Week 9: Task Management (40 hours)

**Deliverables:**

1. **Task Structure (10 hours)**
   - Define task control block (TCB):
     ```c
     struct task {
         uint64_t id;
         char name[32];
         enum sched_class sched_class;
         uint8_t priority;
         enum task_state state;  // READY, RUNNING, BLOCKED, TERMINATED
         struct cpu_context context;  // Saved registers
         void *kernel_stack;
         void *user_stack;      // NULL for kernel tasks
         struct page_table *page_table;
         struct list_head tasks_list;
         struct list_head sched_list;
     };
     ```
   - File: `sched/task.c`

2. **Task Creation and Destruction (15 hours)**
   - Functions:
     ```c
     struct task *task_create(const char *name, void (*entry)(void),
                             enum sched_class class, uint8_t priority);
     void task_destroy(struct task *task);
     void task_exit(int status);
     ```
   - Allocate kernel stack (8KB per task)
   - Initialize CPU context to point to entry function

3. **Context Switching (15 hours)**
   - Implement `context_switch()` in assembly:
     ```asm
     context_switch:
         ; Save current context (push all registers)
         ; Switch stack pointer
         ; Load new context (pop all registers)
         ; Return
     ```
   - File: `sched/context.S`
   - Save/restore: All GPRs, RIP, RSP, RFLAGS, segment registers

**Success Criteria:**
- Can create kernel tasks
- Context switch saves/restores all state correctly
- Simple test: Two tasks ping-pong with context switch

---

#### Week 10: Scheduler Core (40 hours)

**Deliverables:**

1. **Scheduler Queue Management (15 hours)**
   - Implement per-CPU run queues
   - Separate queue per scheduling class
   - Functions:
     ```c
     void sched_init(void);
     void sched_add_task(struct task *task);
     void sched_remove_task(struct task *task);
     struct task *sched_pick_next(void);
     ```
   - File: `sched/sched.c`

2. **Scheduling Algorithm (20 hours)**
   - Implement Fair scheduler (CFS-inspired):
     - Track virtual runtime per task
     - Select task with minimum virtual runtime
     - Adjust for priority (weight)
   - Implement Realtime scheduler (FIFO):
     - Run highest priority task until blocks or yields
   - Implement Batch scheduler:
     - Low priority, only runs when no other tasks ready

3. **Timer Integration (5 hours)**
   - Call scheduler on timer interrupt
   - Preempt current task if time slice expired
   - Time slice: 10ms initially

**Success Criteria:**
- Multiple tasks can coexist
- Tasks scheduled according to class and priority
- No starvation (all tasks eventually run)

---

#### Week 11: Hybrid Core Support (40 hours)

**Deliverables:**

1. **CPU Topology Detection (15 hours)**
   - Use CPUID to detect P-cores and E-cores
   - Parse CPUID leaf 0x1A (Intel Hybrid)
   - Build CPU topology map:
     ```c
     struct cpu_info {
         uint8_t core_id;
         enum core_type { CORE_P, CORE_E } type;
         uint8_t cache_size;
     };
     ```
   - File: `sched/hybrid.c`

2. **Affinity Hints (15 hours)**
   - Add affinity hint to task structure:
     ```c
     enum task_affinity {
         AFFINITY_ANY,
         AFFINITY_P_CORE,  // Prefer P-core
         AFFINITY_E_CORE   // Prefer E-core
     };
     ```
   - Scheduler prefers hinted core type when available

3. **Basic Load Balancing (10 hours)**
   - Periodically balance load between P-cores and E-cores
   - Move tasks based on affinity and current load

**Success Criteria:**
- P-cores and E-cores correctly detected
- Tasks with affinity hints prefer correct core type
- Load balanced across all cores

**Milestone: Preemptive Multitasking Working** 🔄

---

### Phase 4: System Calls and User Space (Weeks 12-14, 120 hours)

**Goal:** Implement Hermes syscall interface and support user space processes.

#### Week 12: Syscall Infrastructure (40 hours)

**Deliverables:**

1. **Syscall Entry Point (15 hours)**
   - Use `syscall` instruction (fast system calls)
   - Implement syscall entry in assembly:
     ```asm
     syscall_entry:
         ; Save user context
         ; Switch to kernel stack
         ; Call syscall dispatcher
         ; Restore user context
         ; sysretq
     ```
   - File: `syscall/entry.S`
   - Configure MSR for syscall (LSTAR, STAR, FMASK)

2. **Syscall Dispatch Table (10 hours)**
   - Create dispatch table:
     ```c
     typedef long (*syscall_fn_t)(uint64_t arg1, uint64_t arg2, ...);
     syscall_fn_t syscall_table[512];
     ```
   - Implement dispatcher:
     ```c
     long syscall_dispatch(uint64_t nr, uint64_t arg1, uint64_t arg2,
                          uint64_t arg3, uint64_t arg4, uint64_t arg5,
                          uint64_t arg6);
     ```
   - File: `syscall/table.c`

3. **Basic Syscalls (15 hours)**
   - Implement 10 essential syscalls:
     - `sys_write(int fd, const void *buf, size_t count)` → console output
     - `sys_exit(int status)` → terminate process
     - `sys_getpid(void)` → process ID
     - `sys_brk(void *addr)` → adjust heap
     - `sys_mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off)`
     - `sys_munmap(void *addr, size_t len)`
     - `sys_open(const char *path, int flags)`
     - `sys_read(int fd, void *buf, size_t count)`
     - `sys_close(int fd)`
     - `sys_nanosleep(const struct timespec *req, struct timespec *rem)`
   - File: `syscall/impl_*.c`

**Success Criteria:**
- Syscall entry/exit works correctly
- User can't crash kernel with invalid syscall number
- Basic syscalls implemented and tested from kernel mode

---

#### Week 13: User Space Support (40 hours)

**Deliverables:**

1. **User Space Address Space (15 hours)**
   - Create separate page table for each process
   - Map user space at low addresses (0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF)
   - Keep kernel mapped in upper half (shared across all processes)
   - File: `mm/uspace.c`

2. **User/Kernel Mode Switching (15 hours)**
   - Switch page table on mode switch (write CR3)
   - Handle segment selectors (CS, DS, SS for user/kernel)
   - Validate user pointers before dereferencing in kernel:
     ```c
     bool is_user_pointer_valid(const void *ptr, size_t len);
     ssize_t copy_from_user(void *kernel_dst, const void *user_src, size_t len);
     ssize_t copy_to_user(void *user_dst, const void *kernel_src, size_t len);
     ```

3. **Testing (10 hours)**
   - Create simple test program in assembly:
     ```asm
     ; User space test program
     _start:
         mov rax, 1      ; sys_write
         mov rdi, 1      ; stdout
         lea rsi, [msg]
         mov rdx, msg_len
         syscall
         
         mov rax, 60     ; sys_exit
         mov rdi, 0
         syscall
     
     msg: db "Hello from user space!", 10
     msg_len equ $ - msg
     ```
   - Load and execute from kernel

**Success Criteria:**
- Can switch to user mode and back
- User space can make syscalls
- User space can't access kernel memory (page fault if attempted)

---

#### Week 14: ELF Loader (40 hours)

**Deliverables:**

1. **ELF Parser (20 hours)**
   - Parse ELF64 headers (ELF header, program headers, section headers)
   - Validate ELF:
     - Magic number (0x7F 'E' 'L' 'F')
     - Class (64-bit)
     - Machine (x86-64)
     - Type (executable or shared library)
   - File: `kernel/elf.c`
   - Functions:
     ```c
     bool elf_validate(const void *elf_data, size_t size);
     int elf_load(struct task *task, const void *elf_data, size_t size);
     ```

2. **ELF Loading (15 hours)**
   - Allocate pages for each LOAD segment
   - Copy segment data to allocated pages
   - Set up initial stack (with argv, envp)
   - Set entry point in task context

3. **Testing (5 hours)**
   - Create simple C user program:
     ```c
     #include <stdio.h>
     int main(int argc, char **argv) {
         printf("Hello from user space!\n");
         return 0;
     }
     ```
   - Compile with user space toolchain
   - Load and execute from kernel

**Success Criteria:**
- Can parse and load ELF64 executables
- User program runs and exits cleanly
- Can pass arguments and environment to user program

**Milestone: User Space Programs Running** 🎉

---

### Phase 5: Capabilities and Secure IPC (Weeks 15-17, 120 hours)

**Goal:** Implement Cerberus capability system and Pulse IPC.

#### Week 15: Capability System (Cerberus) (40 hours)

**Deliverables:**

1. **Capability Data Structures (10 hours)**
   - Implement capability structure:
     ```c
     struct capability {
         uint128_t id;          // XChaCha20 nonce (cryptographic)
         uint32_t rights;       // Permission bitflags
         uint32_t object_type;  // What this capability refers to
         uint64_t object_id;    // Object identifier
         uint64_t expires;      // Expiration time (0 = never)
         struct capability *parent;  // Delegation chain
     };
     ```
   - Capability table per process
   - File: `cap/capability.c`

2. **Capability Operations (20 hours)**
   - Functions:
     ```c
     cap_handle_t cap_create(uint32_t object_type, uint64_t object_id, uint32_t rights);
     int cap_grant(pid_t target, cap_handle_t cap);
     int cap_derive(cap_handle_t cap, uint32_t subset_rights);
     int cap_revoke(cap_handle_t cap);
     bool cap_check(cap_handle_t cap, uint32_t required_rights);
     ```
   - Use XChaCha20 to generate unpredictable capability IDs
   - Track delegation chains

3. **TLB Shootdown for Revocation (10 hours)**
   - When capability revoked, invalidate TLB on all CPUs
   - Use IPI (Inter-Processor Interrupt) for remote TLB flush
   - Ensure no stale mappings remain

**Success Criteria:**
- Capabilities can be created, granted, derived, revoked
- Revocation immediately prevents further access
- Capability checks are fast (<100ns)

---

#### Week 16-17: IPC (Pulse) (80 hours)

**Deliverables:**

1. **Message Queue Infrastructure (Week 16, 40 hours)**
   - Per-process message queue (bounded)
   - Message structure:
     ```c
     struct ipc_msg {
         pid_t sender;
         size_t size;
         uint8_t data[IPC_MAX_MSG_SIZE];  // 128KB max
         struct list_head list;
     };
     ```
   - Functions:
     ```c
     int ipc_send(pid_t dest, const void *buf, size_t size);
     ssize_t ipc_receive(void *buf, size_t size);
     int ipc_register_port(const char *name);
     int ipc_connect(const char *name);
     ```
   - File: `ipc/msg.c`
   - Blocking semantics: sender blocks if queue full, receiver blocks if queue empty

2. **Small Message Optimization (<128B) (Week 16, cont.)**
   - For small messages, copy directly via registers when possible
   - Avoid dynamic allocation
   - Inline small messages in syscall

3. **Shared Memory IPC (128B-4KB) (Week 17, 20 hours)**
   - For medium messages, use shared memory
   - Map same physical pages in sender and receiver
   - Copy-on-write semantics to prevent modification after send
   - File: `ipc/shm.c`

4. **Page Remapping IPC (>4KB) (Week 17, 20 hours)**
   - For large messages, remap pages from sender to receiver
   - Zero-copy transfer
   - Unmaps pages from sender's address space

**Success Criteria:**
- IPC round-trip latency <10μs (optimize to <1μs in Phase 7)
- Messages delivered reliably (no loss, no corruption)
- Large messages transferred efficiently (zero-copy)

**Milestone: Secure IPC Working** 🔐

---

### Phase 6: Storage and Filesystem (Weeks 18-20, 120 hours)

**Goal:** Implement VirtIO block driver and Chroma VFS.

#### Week 18: VirtIO Infrastructure (40 hours)

**Deliverables:**

1. **VirtIO Transport (25 hours)**
   - Detect VirtIO devices (MMIO or PCI)
   - Implement virtqueue management:
     ```c
     struct virtqueue {
         uint16_t queue_size;
         void *desc_table;    // Descriptor table
         void *avail_ring;    // Available ring
         void *used_ring;     // Used ring
         uint16_t last_used_idx;
     };
     ```
   - Functions:
     ```c
     void virtio_init(void);
     struct virtqueue *virtqueue_create(size_t size);
     int virtqueue_add_buf(struct virtqueue *vq, struct virtio_sg *sg, uint16_t out_num, uint16_t in_num);
     void virtqueue_kick(struct virtqueue *vq);
     void *virtqueue_get_buf(struct virtqueue *vq, uint32_t *len);
     ```
   - File: `drivers/virtio/virtio.c`

2. **VirtIO Block Driver (15 hours)**
   - Initialize VirtIO block device
   - Implement read/write operations:
     ```c
     ssize_t virtio_blk_read(void *buf, size_t sector, size_t count);
     ssize_t virtio_blk_write(const void *buf, size_t sector, size_t count);
     ```
   - File: `drivers/virtio/virtio_blk.c`
   - Handle asynchronous I/O (interrupts when I/O completes)

**Success Criteria:**
- Can detect VirtIO block device in QEMU
- Can read and write sectors reliably
- No data corruption after 1000 I/O operations

---

#### Week 19-20: Filesystem (Chroma VFS) (80 hours)

**Deliverables:**

1. **VFS Layer (Week 19, 30 hours)**
   - Implement VFS abstractions:
     ```c
     struct inode {
         uint64_t ino;
         mode_t mode;        // File type and permissions
         uint64_t size;
         struct inode_ops *ops;
         void *private_data;
     };
     
     struct inode_ops {
         ssize_t (*read)(struct inode *, void *buf, size_t size, off_t offset);
         ssize_t (*write)(struct inode *, const void *buf, size_t size, off_t offset);
         struct inode *(*lookup)(struct inode *dir, const char *name);
         int (*create)(struct inode *dir, const char *name, mode_t mode);
         int (*unlink)(struct inode *dir, const char *name);
     };
     ```
   - File descriptor table per process
   - File: `fs/vfs.c`

2. **RAM Filesystem (Week 19, 10 hours)**
   - In-memory filesystem (no persistence)
   - Support: files, directories (flat, no deep hierarchy yet)
   - File: `fs/ramfs.c`

3. **/dev Filesystem (Week 20, 20 hours)**
   - Special filesystem for device nodes
   - Implement:
     - `/dev/null` - discard writes, return EOF on read
     - `/dev/zero` - infinite zeros
     - `/dev/console` - console I/O
     - `/dev/vda` - block device
   - File: `fs/devfs.c`

4. **Syscall Integration (Week 20, 20 hours)**
   - Update syscalls to use VFS:
     - `sys_open()` opens real files via VFS
     - `sys_read()` reads from file via inode ops
     - `sys_write()` writes to file via inode ops
     - `sys_close()` closes file descriptor

**Success Criteria:**
- User program can create, write, read, delete files
- `/dev/null` works correctly
- `/dev/console` allows user program to write to console
- Files persist for duration of kernel run (in RAM)

**Milestone: Persistent Storage and Filesystem Working** 💾

---

### Phase 7: Performance Optimization (Weeks 21-23, 120 hours)

**Goal:** Meet specification performance targets.

#### Week 21: Scheduler Optimization (40 hours)

**Deliverables:**

1. **Context Switch Optimization (20 hours)**
   - Profile current context switch (measure with TSC)
   - Identify hotspots
   - Optimize assembly:
     - Minimize register saves (save only callee-saved)
     - Optimize stack operations
     - Align code to cache lines
   - Target: <500ns per context switch

2. **Cache Warmth Tracking (10 hours)**
   - Use PMU (Performance Monitoring Unit) to track cache misses
   - Prefer scheduling task on CPU where it recently ran

3. **Fair Scheduler Optimization (10 hours)**
   - Optimize virtual time calculation
   - Use red-black tree for O(log n) task selection
   - Minimize lock contention

**Success Criteria:**
- Context switch <500ns (measured with TSC)
- Fair scheduler provides proportional CPU time
- No performance regression on microbenchmarks

---

#### Week 22: Syscall and IPC Optimization (40 hours)

**Deliverables:**

1. **Syscall Fast Path (20 hours)**
   - Profile syscall entry/exit
   - Minimize instruction count in fast path
   - Optimize argument passing (use registers, not stack)
   - Target: <80ns for null syscall

2. **IPC Fast Path (20 hours)**
   - Eliminate locks from small message path (use lock-free queue)
   - Optimize page remapping for large messages
   - Use futex for blocking (avoid spinlocks)
   - Target: <1μs round-trip latency

**Success Criteria:**
- Syscall overhead <100ns (stretch: <80ns)
- IPC latency <1μs
- Throughput >1M messages/second

---

#### Week 23: Memory Allocator Optimization (40 hours)

**Deliverables:**

1. **SLAB Allocator Optimization (20 hours)**
   - Use per-CPU caches (avoid locks in fast path)
   - Pre-allocate slabs to avoid slow path
   - Optimize cache coloring for better cache utilization
   - Target: <50ns for 4KB allocation

2. **PMM/VMM Optimization (20 hours)**
   - Optimize bitmap search (use SIMD or `tzcnt` instruction)
   - Batch TLB flushes
   - Optimize page table walks

**Success Criteria:**
- Memory allocation <50ns for common sizes
- No performance degradation under high allocation rate

**Milestone: Performance Targets Met** 🚀

---

### Phase 8: Production Readiness (Weeks 24-26, 120 hours)

**Goal:** Testing, debugging, documentation, release.

#### Week 24: Testing Infrastructure (40 hours)

**Deliverables:**

1. **Unit Test Framework (15 hours)**
   - Implement kernel test runner
   - Boot in test mode, run tests, report results
   - Example tests: memory allocator, scheduler, IPC

2. **Integration Tests (15 hours)**
   - Syscall tests (test each syscall from user space)
   - IPC tests (message passing between processes)
   - Filesystem tests (create, read, write, delete)

3. **Stress Tests (10 hours)**
   - Run kernel under load for extended periods
   - Tests: 1000 processes, high IPC rate, memory pressure

**Success Criteria:**
- 100+ unit tests passing
- 50+ integration tests passing
- Kernel stable for 24+ hours under stress

---

#### Week 25: Debugging Tools (40 hours)

**Deliverables:**

1. **GDB Stub (20 hours)**
   - Implement GDB remote serial protocol
   - Support: breakpoints, memory inspection, register dumps
   - File: `kernel/gdbstub.c`

2. **Kernel Profiler (10 hours)**
   - Sample-based profiling using PMU
   - Generate flamegraphs

3. **Crash Dumps (10 hours)**
   - Save core dump on kernel panic
   - Include: register state, stack trace, memory contents

**Success Criteria:**
- GDB can connect and debug kernel
- Profiler identifies performance hotspots
- Crash dumps useful for debugging

---

#### Week 26: Documentation and Release (40 hours)

**Deliverables:**

1. **API Documentation (15 hours)**
   - Document all public kernel APIs
   - Document syscall interface
   - Generate with Doxygen

2. **Getting Started Guide (10 hours)**
   - How to build kernel
   - How to run in QEMU
   - How to write user programs

3. **Release v0.1.0 (15 hours)**
   - Fix critical bugs
   - Tag release
   - Write release notes
   - Demo applications: shell, HTTP server

**Success Criteria:**
- Documentation complete and accurate
- v0.1.0 released and tagged
- Demo applications work

**Milestone: v0.1.0 Release** 🎊

---

## Effort Estimation and Timeline

### Total Effort: 1040 hours (26 weeks × 40 hours/week)

**Phase Breakdown:**
- Phase 0: 80 hours (2 weeks) - Boot infrastructure
- Phase 1: 120 hours (3 weeks) - Memory management
- Phase 2: 120 hours (3 weeks) - Interrupts and drivers
- Phase 3: 120 hours (3 weeks) - Scheduler
- Phase 4: 120 hours (3 weeks) - Syscalls and user space
- Phase 5: 120 hours (3 weeks) - Capabilities and IPC
- Phase 6: 120 hours (3 weeks) - Storage and filesystem
- Phase 7: 120 hours (3 weeks) - Performance optimization
- Phase 8: 120 hours (3 weeks) - Testing and release

**Assumptions:**
- Full-time work (40 hours/week)
- Experienced C/OS developer
- Access to x86_64 hardware or QEMU

**Risk Buffer:** ~15% buffer included for unexpected issues

---

## Technical Challenges and Risks

### High-Priority Risks

1. **Performance Targets Very Aggressive**
   - Context switch <500ns requires careful optimization
   - Syscall <80ns may require hardware assist
   - **Mitigation:** Profile extensively, optimize incrementally, accept compromise if needed

2. **C Memory Safety**
   - No compiler guarantees like Rust
   - Potential for buffer overflows, use-after-free, etc.
   - **Mitigation:** Extensive validation, static analysis tools (Clang analyzer), fuzzing

3. **SMP Synchronization** (Future)
   - Not in Phase 1-8, but will be needed
   - **Mitigation:** Minimize shared state, use atomic operations, test extensively

### Medium-Priority Risks

4. **Hardware Compatibility**
   - May not work on all x86_64 systems
   - **Mitigation:** Test on QEMU primarily, document real hardware support

5. **Schedule Slip**
   - 1040 hours is substantial
   - **Mitigation:** Prioritize critical features, cut non-essential if needed

---

## Success Metrics

### Functional
- Boots reliably (100% on QEMU, >95% on real hardware)
- 87/112 POSIX syscalls implemented
- User programs run successfully
- Stable for >24 hours

### Performance
- Context switch: <500ns (target)
- Syscall: <80ns (target)
- IPC: <1μs (target)
- Memory allocation: <50ns (target)

### Quality
- Zero critical bugs
- >70% code coverage in tests
- Static analysis clean (no warnings)

---

## Post-v0.1.0 Roadmap

### Phase 9: SMP Support (Weeks 27-30)
- Multi-processor initialization
- Per-CPU data structures
- SMP-safe scheduler
- TLB shootdown

### Phase 10: Networking (Weeks 31-34)
- VirtIO network driver
- TCP/IP stack
- Socket API

### Phase 11: RISC-V Port (Weeks 35-38)
- RISC-V HAL
- RISC-V interrupt handling
- RISC-V scheduler

---

## Conclusion

This plan provides a realistic, achievable roadmap to build the Serix microkernel from scratch in C, based purely on the specification in ADD.md. The 26-week timeline is ambitious but feasible for an experienced developer.

**Key Success Factors:**
- Follow the plan sequentially (each phase builds on previous)
- Test extensively at each phase (don't defer testing)
- Profile early and often (performance is a core goal)
- Document as you go (easier than doing it at the end)

**Next Steps:**
1. Set up development environment (toolchain, QEMU, build system)
2. Begin Phase 0, Week 1: Build system and boot infrastructure
3. Commit code frequently, tag milestones

**Timeline to v0.1.0:** 26 weeks from start (~6.5 months full-time)

---

**End of Bootstrap Plan**
