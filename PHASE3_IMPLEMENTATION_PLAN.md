# Serix Phase 3 Implementation Plan

> **Target**: Complete Phase 3 - Preemptive Scheduling & IPC Hardening
> 
> **Architecture**: Single-CPU with SMP placeholders
> 
> **Approach**: Sprint-based, atomic commits, feature-by-feature implementation

---

## Overview

This plan implements the four critical features needed to complete Phase 3 of the Serix kernel:

1. **Preemptive Task Switching** - Wire LAPIC timer to scheduler with proper context switching
2. **Stack Allocation Infrastructure** - SLAB/SLUB allocator for kernel stacks (1 MB per task)
3. **Blocking IPC** - Add task state transitions and wake-up mechanism for receive()
4. **VirtIO Block I/O** - Complete virtqueue operations with async interrupt-driven I/O

Each feature is broken into small sprints (~2-4 commits each) for incremental progress.

---

## Feature 1: Preemptive Task Switching

**Goal**: Enable timer-driven preemptive multitasking on single CPU with SMP placeholders

### Sprint 1.1: Task State Management (2 commits)

**Objective**: Add task state transitions and run queue infrastructure

#### Commit 1.1.1: `feat(task): add task state transitions and scheduler infrastructure`
**Files**: `task/src/lib.rs`, `task/src/scheduler.rs` (new)

**Changes**:
- Add `TaskState` variants: `Ready`, `Running`, `Blocked`, `Sleeping`, `Zombie`
- Implement state transition methods: `set_state()`, `is_runnable()`
- Add `CURRENT_TASK: AtomicU64` to track running task globally
- Create `struct RunQueue` with `VecDeque<Arc<TaskCB>>` for single-CPU
- Add `// TODO(SMP): Per-CPU run queues with GS_BASE` comment
- Implement `RunQueue::enqueue()`, `RunQueue::dequeue()`, `RunQueue::peek()`

**Code Style**:
```rust
/*
 * set_state - Transition task to new state
 * @new_state: Target state
 *
 * Atomically updates task state. Should be called with scheduler lock held.
 */
pub fn set_state(&self, new_state: TaskState) {
	self.state.store(new_state as u8, Ordering::Release);
}
```

**Testing**: Compile check, verify enqueue/dequeue logic

---

#### Commit 1.1.2: `feat(task): implement round-robin scheduler with task selection`
**Files**: `task/src/scheduler.rs`

**Changes**:
- Implement `schedule()` function: select next runnable task
- Add `pick_next_task()`: dequeue from run queue, check `is_runnable()`
- Add `reschedule_current()`: re-enqueue current task if still `Ready`
- Add quantum tracking: `const TIME_SLICE_TICKS: u64 = 10`
- Add `// TODO(SMP): Load-balancing across CPUs` comment

**Code Style**:
```rust
/*
 * schedule - Select and switch to next runnable task
 *
 * Called from timer interrupt or voluntary yield. Performs task selection
 * and context switch. Must be called with interrupts disabled.
 *
 * TODO(SMP): Acquire per-CPU run queue lock instead of global
 */
pub fn schedule() {
	// Implementation
}
```

**Testing**: Verify task selection logic with mock tasks

---

### Sprint 1.2: Timer Integration (2 commits)

**Objective**: Wire LAPIC timer to invoke scheduler

#### Commit 1.2.1: `feat(apic): refactor timer handler to call scheduler`
**Files**: `apic/src/timer.rs`

**Changes**:
- Modify `timer_interrupt()` to call `task::schedule()` every `TIME_SLICE_TICKS`
- Keep tick counter incrementation
- Add `unsafe { send_eoi(); }` AFTER scheduler call (important ordering)
- Remove `timer_interrupt_handler()` (redundant with `timer_interrupt()`)
- Update `register_handler()` call

**Code Style**:
```rust
/*
 * timer_interrupt - Timer interrupt handler with preemption
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Increments tick counter and invokes scheduler every TIME_SLICE_TICKS.
 * EOI must be sent after scheduler to prevent lost interrupts.
 */
extern "x86-interrupt" fn timer_interrupt(_stack_frame: InterruptStackFrame) {
	unsafe {
		TICKS += 1;
	}
	
	/* Invoke scheduler every time slice */
	if unsafe { TICKS } % task::TIME_SLICE_TICKS == 0 {
		task::schedule();
	}
	
	unsafe {
		send_eoi();
	}
}
```

**Testing**: Boot kernel, verify timer interrupts fire, check `schedule()` is called

---

#### Commit 1.2.2: `feat(task): integrate context switching in scheduler`
**Files**: `task/src/scheduler.rs`, `task/src/context_switch.rs`

**Changes**:
- Call `context_switch()` in `schedule()` when switching tasks
- Save current task context before switch
- Restore next task context after switch
- Update `CURRENT_TASK` atomic
- Handle case where next task == current task (no switch needed)

**Code Style**:
```rust
/*
 * schedule - Perform task scheduling and context switch
 *
 * Saves current task context, selects next runnable task, and performs
 * context switch. If no other task is ready, continues current task.
 *
 * Safety: Must be called with interrupts disabled to prevent races.
 */
pub fn schedule() {
	let current = get_current_task();
	let next = pick_next_task();
	
	if next.task_id == current.task_id {
		return; /* Same task, no switch needed */
	}
	
	/* Update current task pointer */
	CURRENT_TASK.store(next.task_id, Ordering::Release);
	
	/* Perform context switch */
	unsafe {
		context_switch(&mut *current.context.lock(), &*next.context.lock());
	}
}
```

**Testing**: Create 2 dummy tasks, verify context switch happens

---

### Sprint 1.3: Multi-Task Testing (1 commit)

**Objective**: Validate preemptive multitasking with real tasks

#### Commit 1.3.1: `test(task): add multi-task test in kernel main`
**Files**: `kernel/src/main.rs`

**Changes**:
- Create 3 test tasks with different priorities
- Each task prints its ID and yields
- Verify round-robin scheduling via serial output
- Add `// TODO: Remove test tasks before production` comment

**Code Style**:
```rust
/*
 * test_task_a - Test task A (priority 120)
 *
 * Prints message and yields CPU. Used to validate preemptive scheduling.
 * TODO: Remove before production.
 */
async fn test_task_a() {
	loop {
		serial_println!("[TASK A] Running");
		task::yield_now().await;
	}
}
```

**Testing**: Run in QEMU, verify task interleaving in serial output

---

## Feature 2: Stack Allocation Infrastructure

**Goal**: Implement SLAB/SLUB allocator for kernel stacks (1 MB per task)

### Sprint 2.1: Memory Allocator Foundation (3 commits)

**Objective**: Create SLUB allocator for fixed-size kernel objects

#### Commit 2.1.1: `feat(memory): add SLUB allocator infrastructure`
**Files**: `memory/src/slub.rs` (new), `memory/src/lib.rs`

**Changes**:
- Create `struct SlubAllocator` with free list per size class
- Implement size classes: 4K, 8K, 16K, 32K, 64K, 128K, 256K, 512K, 1M
- Add `struct SlubCache` for each size class
- Implement `alloc_slab()`: allocate pages from frame allocator
- Add `free_slab()`: return pages to frame allocator
- Add `// TODO: Add per-CPU caches for lock-free fast path` comment

**Code Style**:
```rust
/*
 * struct SlubAllocator - SLUB allocator for kernel objects
 * @caches: Array of slab caches for different size classes
 * @lock: Protects allocator data structures
 *
 * Provides efficient allocation of fixed-size kernel objects.
 * Currently uses global lock; TODO: per-CPU caches for scalability.
 */
pub struct SlubAllocator {
	caches: [SlubCache; NUM_SIZE_CLASSES],
	lock: Mutex<()>,
}
```

**Testing**: Allocate/free objects, verify free list integrity

---

#### Commit 2.1.2: `feat(memory): implement slab allocation and deallocation`
**Files**: `memory/src/slub.rs`

**Changes**:
- Implement `SlubAllocator::alloc(&self, size: usize) -> *mut u8`
- Select appropriate cache based on size
- Pop from free list or allocate new slab
- Implement `SlubAllocator::free(&self, ptr: *mut u8, size: usize)`
- Push to free list
- Add debug assertions for pointer alignment

**Code Style**:
```rust
/*
 * alloc - Allocate object from SLUB cache
 * @size: Size of object in bytes
 *
 * Return: Pointer to allocated memory, or null on failure
 *
 * Selects appropriate size class and returns object from free list.
 * If free list is empty, allocates new slab from frame allocator.
 */
pub fn alloc(&self, size: usize) -> *mut u8 {
	let cache_idx = Self::size_to_cache_idx(size);
	// Implementation
}
```

**Testing**: Stress test with many alloc/free cycles

---

#### Commit 2.1.3: `feat(memory): integrate SLUB allocator with global memory subsystem`
**Files**: `memory/src/lib.rs`, `memory/src/heap.rs`

**Changes**:
- Initialize `SLUB_ALLOCATOR: Once<SlubAllocator>` in `init_heap()`
- Export `alloc_kernel_object(size)` and `free_kernel_object(ptr, size)`
- Add wrapper functions for type safety
- Keep existing heap allocator for small allocations

**Code Style**:
```rust
/*
 * alloc_kernel_object - Allocate fixed-size kernel object
 * @size: Size in bytes (must be power of 2, <= 1 MiB)
 *
 * Return: Pointer to allocated memory
 *
 * Uses SLUB allocator for efficient fixed-size allocation.
 * For sizes < 4 KiB, falls back to heap allocator.
 */
pub fn alloc_kernel_object(size: usize) -> *mut u8 {
	// Implementation
}
```

**Testing**: Verify integration, benchmark allocation speed

---

### Sprint 2.2: Task Stack Allocation (2 commits)

**Objective**: Use SLUB allocator for task kernel stacks

#### Commit 2.2.1: `feat(task): implement kernel stack allocation using SLUB`
**Files**: `task/src/lib.rs`

**Changes**:
- Remove `// TODO: Allocate stack memory properly` comment
- Add `const KERNEL_STACK_SIZE: usize = 1024 * 1024; /* 1 MiB */`
- Implement `TaskCB::alloc_kernel_stack() -> VirtAddr`
- Allocate 1 MiB stack using `memory::alloc_kernel_object()`
- Map stack pages with `PRESENT | WRITABLE | NO_EXECUTE` flags
- Store stack base in `TaskCB`

**Code Style**:
```rust
/*
 * alloc_kernel_stack - Allocate kernel stack for task
 *
 * Return: Virtual address of stack top (grows downward)
 *
 * Allocates 1 MiB kernel stack from SLUB allocator and maps it
 * into kernel address space. Stack is used during syscalls and
 * interrupt handling for this task.
 */
fn alloc_kernel_stack() -> VirtAddr {
	let stack_bottom = memory::alloc_kernel_object(KERNEL_STACK_SIZE);
	assert!(!stack_bottom.is_null());
	
	/* Stack grows downward, return top address */
	VirtAddr::new(stack_bottom as u64 + KERNEL_STACK_SIZE as u64)
}
```

**Testing**: Create task, verify stack is allocated and accessible

---

#### Commit 2.2.2: `feat(task): add stack guard pages and overflow detection`
**Files**: `task/src/lib.rs`

**Changes**:
- Allocate stack size + 4 KiB (guard page at bottom)
- Unmap guard page or map as `NOT_PRESENT`
- Add stack canary value at base of actual stack
- Check canary on context switch (debug builds)
- Add `// TODO: Add red zones for KASAN integration` comment

**Code Style**:
```rust
/*
 * alloc_kernel_stack - Allocate kernel stack with guard page
 *
 * Return: Virtual address of stack top
 *
 * Allocates 1 MiB + 4 KiB (guard page) from SLUB allocator.
 * Guard page is mapped as NOT_PRESENT to catch stack overflows.
 * Stack canary is written at base and checked on context switch.
 *
 * TODO: Add red zones for KASAN integration
 */
fn alloc_kernel_stack() -> VirtAddr {
	const GUARD_SIZE: usize = 4096;
	let total_size = KERNEL_STACK_SIZE + GUARD_SIZE;
	
	// Implementation with guard page
}
```

**Testing**: Trigger stack overflow intentionally, verify guard page catches it

---

## Feature 3: Blocking IPC with Task State Transitions

**Goal**: Add blocking receive() that puts tasks to sleep and wakes them on message arrival

### Sprint 3.1: IPC Infrastructure (2 commits)

**Objective**: Add wait queue to ports and blocking receive()

#### Commit 3.1.1: `feat(ipc): add wait queue to ports for blocking receive`
**Files**: `ipc/src/lib.rs`

**Changes**:
- Remove `// TODO: waiting_tasks` comment
- Add `waiting_tasks: Mutex<VecDeque<TaskId>>` to `struct Port`
- Implement `Port::block_on_receive(task_id: TaskId)`
- Add task to wait queue
- Set task state to `Blocked`
- Call `task::schedule()` to yield CPU

**Code Style**:
```rust
/*
 * struct Port - IPC communication port with wait queue
 * @id: Port identifier
 * @queue: Message queue (bounded by PORT_QUEUE_LEN)
 * @waiting_tasks: Tasks blocked waiting for messages
 *
 * Port provides message-passing IPC with blocking receive semantics.
 * When receive() is called on empty port, calling task is added to
 * wait queue and rescheduled.
 */
pub struct Port {
	id: u64,
	queue: Mutex<VecDeque<Message>>,
	waiting_tasks: Mutex<VecDeque<TaskId>>,
}
```

**Testing**: Block task on empty port, verify state is `Blocked`

---

#### Commit 3.1.2: `feat(ipc): implement task wake-up on message send`
**Files**: `ipc/src/lib.rs`

**Changes**:
- Remove `// TODO: Wake up waiting tasks` comment in `send()`
- After pushing message, check if any tasks are waiting
- Pop task from wait queue
- Set task state to `Ready`
- Enqueue task to scheduler run queue
- Handle case where multiple tasks are waiting (wake oldest first)

**Code Style**:
```rust
/*
 * send - Send message to port and wake waiting task
 * @msg: Message to send
 *
 * Return: true if successful, false if queue full
 *
 * Pushes message to port queue. If any task is blocked waiting for
 * messages, wakes the oldest waiting task by transitioning it to
 * Ready state and enqueueing it to the scheduler.
 */
pub fn send(&self, msg: Message) -> bool {
	let mut q = self.queue.lock();
	if q.len() >= PORT_QUEUE_LEN {
		return false;
	}
	q.push_back(msg);
	drop(q); /* Release message queue lock */
	
	/* Wake up one waiting task if any */
	let mut waiting = self.waiting_tasks.lock();
	if let Some(task_id) = waiting.pop_front() {
		task::wake_task(task_id);
	}
	
	true
}
```

**Testing**: Send message to port with blocked task, verify task wakes up

---

### Sprint 3.2: Blocking Syscall Integration (2 commits)

**Objective**: Wire blocking receive into syscall handler

#### Commit 3.2.1: `feat(ipc): add blocking receive() method to IpcSpace`
**Files**: `ipc/src/lib.rs`

**Changes**:
- Add `IpcSpace::receive_blocking(port_id: u64) -> Result<Message, IpcError>`
- Loop: try non-blocking receive
- If message available, return immediately
- If queue empty, call `Port::block_on_receive(current_task_id())`
- After wake-up, retry receive

**Code Style**:
```rust
/*
 * receive_blocking - Blocking receive from port
 * @port_id: Port identifier
 *
 * Return: Message on success, IpcError on failure
 *
 * Attempts to receive message from port. If port queue is empty,
 * blocks calling task until message arrives. Task is woken when
 * another task sends to this port.
 */
pub fn receive_blocking(&self, port_id: u64) -> Result<Message, IpcError> {
	let ports = self.ports.read();
	let port = ports.get(&port_id).ok_or(IpcError::InvalidPort)?;
	
	loop {
		/* Try non-blocking receive */
		if let Some(msg) = port.receive() {
			return Ok(msg);
		}
		
		/* No message available, block until one arrives */
		port.block_on_receive(task::current_task_id());
		
		/* Task resumed, retry receive */
	}
}
```

**Testing**: Call blocking receive from syscall, verify task blocks and wakes

---

#### Commit 3.2.2: `feat(kernel): add SYS_RECV_BLOCKING syscall variant`
**Files**: `kernel/src/syscall.rs`

**Changes**:
- Add `SYS_RECV_BLOCKING = 22` constant
- Add `sys_recv_blocking()` handler
- Update syscall dispatcher to handle new syscall
- Keep existing `SYS_RECV` for non-blocking behavior
- Document both variants in comments

**Code Style**:
```rust
/*
 * sys_recv_blocking - Blocking IPC receive syscall
 * @port_id: Port identifier
 * @buf: Userspace buffer for message data
 * @len: Buffer length
 *
 * Return: Message ID on success, negative error code on failure
 *
 * Blocks calling task until message is available on port. Task is
 * rescheduled when message arrives.
 */
fn sys_recv_blocking(port_id: u64, buf: u64, len: u64) -> i64 {
	/* Validate userspace pointer */
	if !is_valid_user_ptr(buf, len) {
		return ERRNO_EFAULT as i64;
	}
	
	/* Perform blocking receive */
	match ipc::IPC_GLOBAL.receive_blocking(port_id) {
		Ok(msg) => {
			/* Copy message data to userspace */
			// Implementation
		}
		Err(_) => ERRNO_EINVAL as i64,
	}
}
```

**Testing**: Test IPC ping-pong between two tasks

---

## Feature 4: VirtIO Block I/O Operations

**Goal**: Complete virtqueue read/write with interrupt-driven async I/O

### Sprint 4.1: Virtqueue Infrastructure (3 commits)

**Objective**: Set up descriptor table, available ring, used ring

#### Commit 4.1.1: `feat(drivers): define virtqueue data structures`
**Files**: `drivers/src/virtio.rs`, `drivers/src/virtqueue.rs` (new)

**Changes**:
- Create `struct VirtqDesc` (descriptor table entry)
- Create `struct VirtqAvail` (available ring)
- Create `struct VirtqUsed` (used ring)
- Add `struct Virtqueue` wrapper with DMA-safe allocation
- Add `const VIRTQ_DESC_F_NEXT: u16 = 1`, `VIRTQ_DESC_F_WRITE: u16 = 2`

**Code Style**:
```rust
/*
 * struct VirtqDesc - VirtIO descriptor table entry
 * @addr: Physical address of buffer
 * @len: Length of buffer in bytes
 * @flags: Descriptor flags (NEXT, WRITE, INDIRECT)
 * @next: Index of next descriptor (if NEXT flag set)
 *
 * Describes a DMA buffer for VirtIO device. Descriptors can be
 * chained using NEXT flag for scatter-gather operations.
 */
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtqDesc {
	addr: u64,
	len: u32,
	flags: u16,
	next: u16,
}
```

**Testing**: Verify struct sizes match VirtIO spec

---

#### Commit 4.1.2: `feat(drivers): implement virtqueue allocation and initialization`
**Files**: `drivers/src/virtqueue.rs`

**Changes**:
- Implement `Virtqueue::new(queue_size: u16) -> Self`
- Allocate physically contiguous memory for descriptor table
- Allocate available ring and used ring
- Initialize all entries to zero
- Store physical addresses for device programming

**Code Style**:
```rust
/*
 * new - Allocate and initialize virtqueue
 * @queue_size: Number of descriptors (must be power of 2)
 *
 * Return: Initialized Virtqueue
 *
 * Allocates physically contiguous memory for virtqueue structures:
 * - Descriptor table: 16 bytes * queue_size
 * - Available ring: 6 + 2 * queue_size bytes
 * - Used ring: 6 + 8 * queue_size bytes
 *
 * Memory must be DMA-accessible by device. Alignment requirements:
 * - Descriptor table: 16-byte aligned
 * - Available ring: 2-byte aligned
 * - Used ring: 4-byte aligned
 */
pub fn new(queue_size: u16) -> Self {
	// Implementation with proper alignment
}
```

**Testing**: Allocate virtqueue, verify alignment and sizes

---

#### Commit 4.1.3: `feat(drivers): implement virtqueue descriptor chain builder`
**Files**: `drivers/src/virtqueue.rs`

**Changes**:
- Add `Virtqueue::alloc_desc_chain(&mut self, num_descs: u16) -> Option<u16>`
- Return index of first descriptor
- Mark descriptors as used in free list bitmap
- Add `Virtqueue::free_desc_chain(&mut self, head_idx: u16)`
- Walk chain using NEXT flags and mark descriptors as free

**Code Style**:
```rust
/*
 * alloc_desc_chain - Allocate chain of descriptors
 * @num_descs: Number of descriptors needed
 *
 * Return: Index of first descriptor, or None if insufficient descriptors
 *
 * Allocates contiguous descriptors from free list and chains them
 * using NEXT flags. Used for scatter-gather I/O operations where
 * request spans multiple buffers.
 */
pub fn alloc_desc_chain(&mut self, num_descs: u16) -> Option<u16> {
	// Implementation with free list management
}
```

**Testing**: Allocate/free descriptor chains, verify free list integrity

---

### Sprint 4.2: Block Device Driver (3 commits)

**Objective**: Implement read/write operations with virtqueue

#### Commit 4.2.1: `feat(drivers): implement VirtIO block request structures`
**Files**: `drivers/src/virtio.rs`

**Changes**:
- Create `struct VirtioBlkReq` (request header)
- Add `const VIRTIO_BLK_T_IN: u32 = 0` (read)
- Add `const VIRTIO_BLK_T_OUT: u32 = 1` (write)
- Create `struct VirtioBlkReqStatus` (response status)
- Add `const VIRTIO_BLK_S_OK: u8 = 0`, `VIRTIO_BLK_S_IOERR: u8 = 1`

**Code Style**:
```rust
/*
 * struct VirtioBlkReq - Block device request header
 * @type_: Request type (IN=read, OUT=write)
 * @reserved: Reserved, must be zero
 * @sector: First sector to read/write (512-byte units)
 *
 * Request header is always first descriptor in chain.
 * Format: [header desc] -> [data desc(s)] -> [status desc]
 */
#[repr(C)]
pub struct VirtioBlkReq {
	type_: u32,
	reserved: u32,
	sector: u64,
}
```

**Testing**: Verify struct layout matches VirtIO spec

---

#### Commit 4.2.2: `feat(drivers): implement synchronous block read operation`
**Files**: `drivers/src/virtio.rs`

**Changes**:
- Add `VirtioBlock::read_sync(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlkError>`
- Allocate 3-descriptor chain: header, data, status
- Fill header with sector number and VIRTIO_BLK_T_IN
- Map data buffer to descriptor
- Add status descriptor with WRITE flag
- Add to available ring and kick device
- Poll used ring until completion (busy-wait for now)
- Add `// TODO: Replace polling with interrupt-driven completion` comment

**Code Style**:
```rust
/*
 * read_sync - Synchronous block read operation
 * @sector: Starting sector number (512-byte units)
 * @buf: Buffer to read data into
 *
 * Return: Ok on success, BlkError on failure
 *
 * Submits read request to VirtIO block device and busy-waits for
 * completion. Buffer size must be multiple of 512 bytes.
 *
 * TODO: Replace polling with interrupt-driven completion for efficiency.
 */
pub fn read_sync(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlkError> {
	assert_eq!(buf.len() % 512, 0, "Buffer size must be multiple of 512");
	
	/* Build 3-descriptor chain: header -> data -> status */
	// Implementation
}
```

**Testing**: Read sector 0 from disk, verify data

---

#### Commit 4.2.3: `feat(drivers): implement synchronous block write operation`
**Files**: `drivers/src/virtio.rs`

**Changes**:
- Add `VirtioBlock::write_sync(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlkError>`
- Similar to read but use VIRTIO_BLK_T_OUT
- Data descriptor does NOT have WRITE flag (device reads from it)
- Verify status is VIRTIO_BLK_S_OK after completion

**Code Style**:
```rust
/*
 * write_sync - Synchronous block write operation
 * @sector: Starting sector number (512-byte units)
 * @buf: Buffer to write data from
 *
 * Return: Ok on success, BlkError on failure
 *
 * Submits write request to VirtIO block device and busy-waits for
 * completion. Buffer size must be multiple of 512 bytes.
 *
 * TODO: Replace polling with interrupt-driven completion for efficiency.
 */
pub fn write_sync(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlkError> {
	assert_eq!(buf.len() % 512, 0, "Buffer size must be multiple of 512");
	
	/* Build 3-descriptor chain: header -> data -> status */
	// Implementation
}
```

**Testing**: Write data to sector, read back and verify

---

### Sprint 4.3: Interrupt-Driven I/O (3 commits)

**Objective**: Replace polling with async interrupt completion

#### Commit 4.3.1: `feat(drivers): add VirtIO interrupt handler registration`
**Files**: `drivers/src/virtio.rs`, `kernel/src/main.rs`

**Changes**:
- Allocate MSI-X vector for VirtIO block device
- Register interrupt handler with IDT
- Add `extern "x86-interrupt" fn virtio_blk_interrupt()`
- Read ISR status register to acknowledge interrupt
- Add `// TODO: Wake up waiting I/O tasks` comment

**Code Style**:
```rust
/*
 * virtio_blk_interrupt - VirtIO block device interrupt handler
 * @_stack_frame: Interrupt stack frame (unused)
 *
 * Called when VirtIO block device completes I/O request.
 * Reads ISR status register to acknowledge interrupt and processes
 * completed requests from used ring.
 *
 * TODO: Wake up waiting I/O tasks instead of polling
 */
extern "x86-interrupt" fn virtio_blk_interrupt(_stack_frame: InterruptStackFrame) {
	/* Read ISR status to acknowledge interrupt */
	let isr_status = VIRTIO_DEVICE.lock().read_isr_status();
	
	if isr_status & 0x1 != 0 {
		/* Queue interrupt - process used ring */
		process_completed_requests();
	}
	
	apic::send_eoi();
}
```

**Testing**: Trigger I/O, verify interrupt fires

---

#### Commit 4.3.2: `feat(drivers): implement async I/O with completion tracking`
**Files**: `drivers/src/virtio.rs`, `drivers/src/blk_async.rs` (new)

**Changes**:
- Create `struct BlkRequest` with completion status and waker
- Add `PENDING_REQUESTS: Mutex<BTreeMap<u16, BlkRequest>>`
- Implement `read_async()` and `write_async()` returning `Future`
- Store request in map before submitting to device
- In interrupt handler, look up request and call waker

**Code Style**:
```rust
/*
 * struct BlkRequest - Async block I/O request
 * @status: Completion status (None=pending, Some(Ok/Err)=complete)
 * @waker: Task waker to notify on completion
 *
 * Tracks in-flight block I/O request. Stored in global map indexed
 * by descriptor chain head index. Interrupt handler updates status
 * and wakes task when device completes request.
 */
pub struct BlkRequest {
	status: Mutex<Option<Result<(), BlkError>>>,
	waker: Mutex<Option<Waker>>,
}

/*
 * read_async - Asynchronous block read operation
 * @sector: Starting sector number
 * @buf: Buffer to read data into
 *
 * Return: Future that resolves to Result<(), BlkError>
 *
 * Submits read request to device and returns immediately. Calling
 * task can await the future; it will be woken by interrupt handler
 * when I/O completes.
 */
pub async fn read_async(sector: u64, buf: &mut [u8]) -> Result<(), BlkError> {
	// Implementation with future/waker
}
```

**Testing**: Submit multiple async reads, verify all complete

---

#### Commit 4.3.3: `feat(drivers): integrate async VirtIO with VFS layer`
**Files**: `vfs/src/lib.rs`, `drivers/src/virtio.rs`

**Changes**:
- Add `async fn INode::read_async()` and `write_async()` to trait
- Implement for ramdisk (no-op async wrapper)
- Add `struct BlockDevice` INode implementation using VirtIO
- Wire VirtIO block device to VFS at `/dev/vda`
- Update syscall `read()`/`write()` to use async I/O

**Code Style**:
```rust
/*
 * struct BlockDevice - Block device INode backed by VirtIO
 * @device: VirtIO block device driver
 * @sector_size: Sector size in bytes (typically 512)
 *
 * Implements INode trait for block device. File offset is translated
 * to sector numbers for VirtIO driver. All I/O is asynchronous.
 */
pub struct BlockDevice {
	device: Arc<Mutex<VirtioBlock>>,
	sector_size: usize,
}

impl INode for BlockDevice {
	/*
	 * read_async - Read from block device
	 * @offset: Byte offset within device
	 * @buf: Buffer to read into
	 *
	 * Return: Number of bytes read
	 *
	 * Translates offset to sector numbers and issues async read to
	 * VirtIO driver. Handles partial sector reads at start/end.
	 */
	async fn read_async(&self, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
		// Implementation
	}
}
```

**Testing**: Read/write file via VFS, verify data persists across reboots

---

## Commit Strategy

### Atomic Commit Guidelines

1. **One logical change per commit**
   - Each commit should implement exactly one feature or fix one bug
   - If you can't describe the commit in one sentence, split it

2. **Use `git add -p` for selective staging**
   ```bash
   git add -p file.rs
   # Review each hunk, press 'y' to stage, 'n' to skip, 's' to split
   ```

3. **Commit message format**
   ```
   <type>(<scope>): <subject>
   
   <body>
   
   <footer>
   ```
   
   **Types**: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`
   
   **Example**:
   ```
   feat(task): add task state transitions and scheduler infrastructure
   
   Implements TaskState enum with Ready, Running, Blocked, Sleeping, and
   Zombie variants. Adds RunQueue for single-CPU scheduling with dequeue/
   enqueue operations. Includes placeholders for future per-CPU queues.
   
   - Add CURRENT_TASK atomic for tracking running task
   - Implement state transition methods with proper ordering
   - Create RunQueue with VecDeque backing
   ```

4. **Never include co-authors** (as requested)

5. **Push after each sprint** (~2-4 commits)
   ```bash
   git push origin main
   ```

---

## Code Style Requirements

### Comment Style (C-style for functions)

```rust
/*
 * function_name - Brief description
 * @param1: Parameter description
 * @param2: Parameter description
 *
 * Detailed explanation of what the function does.
 * Can span multiple lines.
 *
 * Return: Description of return value
 *
 * Safety: If unsafe, explain why it's safe to call
 */
pub fn function_name(param1: Type1, param2: Type2) -> ReturnType {
	// Implementation
}
```

### Comment Style (Single-line for inline)

```rust
/* Increment tick counter */
TICKS += 1;

/* TODO(SMP): Add per-CPU run queue support */
```

### Struct Documentation

```rust
/*
 * struct Name - Brief description
 * @field1: Field description
 * @field2: Field description
 *
 * Detailed explanation of struct purpose and usage.
 */
pub struct Name {
	field1: Type1,
	field2: Type2,
}
```

### Code Formatting

- **Tabs**: Use tabs for indentation (8 spaces wide)
- **Line width**: Max 100 characters
- **Braces**: Opening brace on same line (K&R style)
- Run `cargo fmt` before each commit

---

## Testing Strategy

### Per-Sprint Testing

After each commit:
1. **Compile check**: `cargo build --release`
2. **Run in QEMU**: `make run`
3. **Check serial output**: Look for `[CHECKPOINT]` markers
4. **Functional test**: Verify specific feature works

### Integration Testing

After each feature (all sprints complete):
1. **Multi-task test**: Run 3+ concurrent tasks
2. **IPC ping-pong test**: Two tasks exchanging messages
3. **Block I/O test**: Write and read back data
4. **Stress test**: Many tasks, heavy I/O load

### Validation Criteria

Feature 1 (Task Switching):
- ✅ Multiple tasks print messages in round-robin order
- ✅ Context switches happen every ~10 ticks
- ✅ No crashes or hangs

Feature 2 (Stack Allocation):
- ✅ Each task has 1 MiB kernel stack
- ✅ Guard page catches stack overflow
- ✅ No memory leaks

Feature 3 (Blocking IPC):
- ✅ Task blocks when receiving from empty port
- ✅ Task wakes when message arrives
- ✅ Multiple tasks can wait on same port

Feature 4 (VirtIO I/O):
- ✅ Can read sector 0 from disk
- ✅ Can write data and read back
- ✅ Data persists across reboot
- ✅ Async I/O completes via interrupts

---

## Sprint Execution Order

Execute sprints in order:

1. **Feature 1 (Task Switching)**: Sprints 1.1 → 1.2 → 1.3
2. **Feature 2 (Stack Allocation)**: Sprints 2.1 → 2.2
3. **Feature 3 (Blocking IPC)**: Sprints 3.1 → 3.2
4. **Feature 4 (VirtIO I/O)**: Sprints 4.1 → 4.2 → 4.3

Each sprint is independent within a feature, but features depend on each other:
- Feature 3 depends on Feature 1 (need working scheduler)
- Feature 4 can be implemented in parallel with Features 1-3

---

## Progress Tracking

Use SQL database to track todo items:

```sql
CREATE TABLE todos (
	id TEXT PRIMARY KEY,
	title TEXT NOT NULL,
	description TEXT,
	status TEXT DEFAULT 'pending',
	feature TEXT,
	sprint TEXT
);

CREATE TABLE todo_deps (
	todo_id TEXT,
	depends_on TEXT,
	PRIMARY KEY (todo_id, depends_on)
);
```

Update status as you work:
- `pending` → `in_progress` → `done`
- Check dependencies before starting new todo

---

## Final Deliverables

When all features are complete:

1. **Phase 3 completion commit**:
   ```
   milestone: complete Phase 3 - preemptive scheduling and IPC hardening
   
   All Phase 3 features implemented:
   - Preemptive task switching with timer-driven scheduling
   - SLUB allocator for kernel stacks (1 MiB per task)
   - Blocking IPC with task wake-up mechanism
   - VirtIO block I/O with async interrupt-driven completion
   
   Kernel now supports multi-tasking with proper isolation and
   persistent storage. Ready for Phase 4 (filesystem stack).
   ```

2. **Update ROADMAP.md**:
   - Mark Phase 3 as ✅ Complete
   - Update "Current Status" section

3. **Update CHANGELOG.md** (if exists):
   - Document all new features
   - List breaking changes (if any)

4. **Final push**:
   ```bash
   git push origin main
   ```

---

## Estimated Effort

- **Feature 1**: 5 commits, ~800 LOC
- **Feature 2**: 5 commits, ~600 LOC
- **Feature 3**: 4 commits, ~400 LOC
- **Feature 4**: 9 commits, ~1200 LOC

**Total**: 23 commits, ~3000 LOC

This plan is designed for incremental implementation with frequent validation.
