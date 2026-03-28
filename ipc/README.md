# IPC Module

## Overview

The IPC (Inter-Process Communication) module implements a port-based message passing system for the Serix kernel, internally named "Pulse." It provides both synchronous (blocking) and asynchronous (non-blocking) communication between tasks. Each port maintains an independent message queue, and tasks can send messages to any port or block waiting for incoming messages on a specific port.

## Architecture

### Core Responsibilities

1. **Message Passing**: Fixed-size message transfer between tasks via named ports
2. **Port Management**: Creation and lookup of communication endpoints in a global namespace
3. **Blocking Semantics**: Wait queue support for tasks that block on empty ports
4. **Wake Coordination**: Automatic wake-up of blocked receivers when messages arrive

### Design Philosophy

The IPC subsystem follows a simple, lock-based model suitable for a uniprocessor kernel:

- **Fixed-size messages**: Avoids heap allocation in the hot path; messages are stack-copyable
- **Bounded queues**: Backpressure via a hard queue depth limit prevents unbounded memory growth
- **Spin locks**: All internal synchronization uses `spin::Mutex`, safe in interrupt-disabled contexts
- **Global namespace**: A single `IpcSpace` serves as the system-wide port registry

## Message Format

```rust
pub const MAX_MSG_SIZE: usize = 128;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Message {
	pub sender_id: u64,
	pub id: u64,
	pub len: u64,
	pub data: [u8; MAX_MSG_SIZE],
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `sender_id` | `u64` | Task ID of the sending task |
| `id` | `u64` | Application-defined message type or identifier |
| `len` | `u64` | Number of valid bytes in `data` (0..128) |
| `data` | `[u8; 128]` | Payload buffer; only the first `len` bytes are meaningful |

The `Message` struct is `#[repr(C)]` and implements `Clone` + `Copy`, making it suitable for register-based transfer and stack allocation. The `Default` implementation zero-initializes all fields.

Total struct size: 24 bytes (header) + 128 bytes (payload) = 152 bytes.

## Port Model

```rust
pub const PORT_QUEUE_LEN: usize = 32;

pub struct Port {
	id: u64,
	queue: Mutex<VecDeque<Message>>,
	waiting_receivers: Mutex<VecDeque<Arc<Mutex<TaskCB>>>>,
}
```

Each `Port` represents a communication endpoint with:

- **id**: A `u64` identifier used for lookup in the global IPC space
- **queue**: A bounded `VecDeque<Message>` holding up to `PORT_QUEUE_LEN` (32) pending messages, protected by a spinlock
- **waiting_receivers**: A FIFO queue of task references (`Arc<Mutex<TaskCB>>`) representing tasks blocked waiting for messages on this port

### Queue Behavior

- Messages are enqueued at the back and dequeued from the front (FIFO ordering)
- The queue is pre-allocated with `VecDeque::with_capacity(PORT_QUEUE_LEN)`
- When the queue reaches 32 messages, `send()` returns `false` and the message is dropped

## IPC Space

```rust
pub struct IpcSpace {
	ports: RwLock<BTreeMap<u64, Arc<Port>>>,
}
```

The `IpcSpace` is the system-wide port registry. It maps port IDs to `Arc<Port>` references using a `BTreeMap` protected by a read-write lock.

### Operations

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `const fn new() -> Self` | Creates an empty IPC namespace (const-constructible) |
| `create_port(id)` | `fn create_port(&self, id: u64) -> Arc<Port>` | Creates a new port with the given ID, inserts it into the registry, and returns a shared reference |
| `get_port(id)` | `fn get_port(&self, id: u64) -> Option<Arc<Port>>` | Looks up a port by ID; returns `None` if not found |

Port creation uses a write lock; port lookup uses a read lock, allowing concurrent readers.

## Global Instance

```rust
pub static IPC_GLOBAL: IpcSpace = IpcSpace::new();
```

A single static `IpcSpace` instance serves as the kernel's global IPC registry. Because `IpcSpace::new()` is a `const fn`, this is initialized at compile time with no runtime setup required.

All kernel subsystems and syscall handlers access ports through `IPC_GLOBAL`.

## Operations

### send()

```rust
pub fn send(&self, msg: Message) -> bool
```

Enqueues a message on the port's queue. If the queue is full (>= 32 messages), the message is rejected and the function returns `false`.

After successfully enqueuing a message, `send()` checks the `waiting_receivers` queue. If any task is blocked waiting for a message on this port, the first waiter is popped and woken via `task::wake_task()`, which transitions the task from `Blocked` state back to `Ready` and places it on the scheduler's run queue.

**Flow**:

```
1. Lock message queue
2. If queue full -> return false
3. Push message to back of queue
4. Unlock message queue
5. Lock waiting_receivers
6. Pop first waiter (if any)
7. Unlock waiting_receivers
8. Call task::wake_task() on waiter
9. Return true
```

### receive()

```rust
pub fn receive(&self) -> Option<Message>
```

Non-blocking dequeue. Locks the message queue, pops the front message, and returns it. Returns `None` immediately if the queue is empty. This operation never blocks the calling task.

### receive_blocking()

```rust
pub fn receive_blocking(&self) -> Message
```

Blocking receive. If a message is available, it is returned immediately (fast path). Otherwise, the calling task is placed on the port's `waiting_receivers` queue and blocked via `task::block_current_and_switch()`, which sets the task's state to `Blocked` and triggers a context switch to the next runnable task.

When a `send()` on this port later wakes the blocked task, execution resumes in the loop body, and the task retries the receive. The loop handles spurious wakes: if the queue is still empty after being woken (e.g., another task consumed the message first), the task blocks again.

**Flow**:

```
loop {
    1. Lock queue, try pop_front
    2. If message available -> return it
    3. Get current task Arc via task::current_task_arc()
    4. Push Arc onto waiting_receivers queue
    5. Call task::block_current_and_switch()
       (task is now blocked; execution resumes here after wake)
    6. Loop back to retry
}
```

**Safety**: Must be called with interrupts disabled. Must not be called from interrupt context.

## Blocking Semantics

The blocking model uses per-port wait queues to avoid busy-waiting:

1. **Blocking**: When `receive_blocking()` finds an empty queue, it obtains the current task's `Arc<Mutex<TaskCB>>` via `task::current_task_arc()` and pushes it onto the port's `waiting_receivers` deque. It then calls `task::block_current_and_switch()`, which sets the task state to `Blocked` and performs a context switch to the next ready task.

2. **Waking**: When `send()` enqueues a message, it pops the first entry from `waiting_receivers` (if any) and calls `task::wake_task()`. This transitions the task back to `Ready` state and places it on the scheduler's `RunQueue` for execution.

3. **Spurious Wake Handling**: After being woken, the task loops back and re-checks the queue. If another task consumed the message between the wake and the retry, the task blocks again. This ensures correctness under concurrent access.

The `waiting_receivers` queue is FIFO, so tasks are woken in the order they blocked --- providing fair scheduling of receivers.

## Syscall Interface

The kernel exposes three IPC-related syscalls, dispatched in `kernel/src/syscall.rs`:

| Number | Name | Arguments | Description |
|--------|------|-----------|-------------|
| 20 | `SYS_SEND` | `RDI`: port ID, `RSI`: pointer to `Message` | Send a message to the specified port |
| 21 | `SYS_RECV` | `RDI`: port ID, `RSI`: pointer to `Message` buffer | Non-blocking receive; returns 0 on success, error if empty |
| 22 | `SYS_RECV_BLOCK` | `RDI`: port ID, `RSI`: pointer to `Message` buffer | Blocking receive; task sleeps until a message arrives |

Userspace wrappers are provided in the `ulib` crate (e.g., `serix_send()`, `serix_recv()`). The syscall ABI follows Linux conventions: `RAX` = syscall number, arguments in `RDI`, `RSI`, `RDX`, `R10`, `R8`, `R9`.

## Dependencies

### Internal Crates

- **task**: Provides `TaskCB`, `current_task_arc()`, `wake_task()`, and `block_current_and_switch()` for blocking/waking coordination

### External Crates

- **spin** (0.10.0): Spinlock (`Mutex`) and `RwLock` synchronization primitives
- **alloc**: `BTreeMap`, `VecDeque`, `Arc` from Rust's `no_std`-compatible allocation library

## Example: Producer/Consumer Pattern

```rust
use ipc::{IPC_GLOBAL, Message, MAX_MSG_SIZE};

/* Producer task */
fn producer() {
	let port = IPC_GLOBAL.create_port(42);

	let mut msg = Message::default();
	msg.sender_id = 1;
	msg.id = 100;
	msg.len = 5;
	msg.data[..5].copy_from_slice(b"hello");

	let ok = port.send(msg);
	if !ok {
		serial_println!("send failed: port queue full");
	}
}

/* Consumer task (blocking) */
fn consumer() {
	let port = IPC_GLOBAL.get_port(42).expect("port not found");

	/* Blocks until a message is available */
	let msg = port.receive_blocking();
	serial_println!(
		"received msg id={} from task {} ({} bytes)",
		msg.id, msg.sender_id, msg.len
	);
}

/* Consumer task (non-blocking) */
fn consumer_poll() {
	let port = IPC_GLOBAL.get_port(42).expect("port not found");

	match port.receive() {
		Some(msg) => {
			serial_println!("got message: id={}", msg.id);
		}
		None => {
			serial_println!("no messages pending");
		}
	}
}
```

In a typical Serix deployment, the producer and consumer run as separate kernel tasks (or userspace processes via syscalls). The producer creates port 42 and sends messages; the consumer retrieves the port handle and either polls or blocks for incoming messages.

## Future Development

### Planned Features

1. **IPC Fastpath**: Direct register transfer when the receiver is already blocked at the call site, bypassing the queue entirely
2. **Per-process IPC Spaces**: Replace the global namespace with capability-scoped port registries
3. **Multi-message Batch Send**: Enqueue multiple messages atomically
4. **Port Destruction**: Clean teardown of ports with notification to blocked waiters
5. **Priority Messaging**: Support for priority levels within port queues

## License

GPL-3.0 (see LICENSE file in repository root)
