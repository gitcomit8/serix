# Phase 3 & Phase 4 Completion Plan

**Current State:** v0.0.6
**Target:** Complete Phase 3 (Preemptive Scheduling & IPC Hardening) and Phase 4 (Storage & Filesystem Stack)

---

## Phase 3: Preemptive Scheduling & IPC Hardening

### Current Implementation Status

**Completed:**
- ✅ `SchedClass` enum (Realtime, Fair, Batch, Iso)
- ✅ LAPIC timer-driven preemption at ~625 Hz
- ✅ SLUB-allocated 1 MiB per-task kernel stacks with guard pages
- ✅ Callee-saved GPR + CR3 context switch; `block_current_and_switch()`
- ✅ Port-based message passing with blocking `receive_blocking()`
- ✅ `SYS_RECV_BLOCK (22)` syscall
- ✅ `send()` wakes first blocked receiver
- ✅ Capability validation on `send()` (implemented in `ipc/src/lib.rs:102-110`)

**Remaining:**
- ⏳ Per-CPU run queues with `GS_BASE` MSR
- ⏳ `TSS.RSP0` swap on context switch (per-task kernel stacks)
- ⏳ Weighted Fair Queueing (WFQ) for `Fair` class
- ⏳ Priority inheritance protocol
- ⏳ SMP Bring-Up (AP bootstrap via INIT-SIPI-SIPI)
- ⏳ IPC fastpath: direct register transfer
- ⏳ Asynchronous notification ports

---

### Task 3.1: Per-CPU Run Queues with GS_BASE

**Why:** Current global `RunQueue` is a single point of contention. Per-CPU queues eliminate lock contention and enable true SMP.

**Current State:**
- Single global `RunQueue` in `task/src/scheduler.rs:48`
- `PER_CPU_DATA` in `kernel/src/gdt.rs:127-137` is a flat struct with `kernel_stack` field
- `GS_BASE` MSR set to `&PER_CPU_DATA` at boot (`gdt.rs:141`)

**Implementation Plan:**

1. **Expand `PerCpuData` struct** (`kernel/src/gdt.rs`):
   ```rust
   #[repr(C)]
   pub struct PerCpuData {
       pub kernel_stack: u64,
       pub run_queue: *const RunQueue,  // Per-CPU run queue pointer
       pub current_task: u64,           // TaskId of current task
       pub idle_task_stack: u64,        // Idle task kernel stack
       pub lapic_id: u8,                // LAPIC ID for this CPU
       pub ap_ready: bool,              // True when AP has finished init
       pub padding: [u8; 16],           // Align to cache line
   }
   ```

2. **Create per-CPU run queue array** (`task/src/scheduler.rs`):
   ```rust
   pub static PER_CPU_RUN_QUEUES: Once<[Once<Mutex<RunQueue>>; MAX_CPUS]> = Once::new();
   ```

3. **Update `RunQueue`** to support per-CPU:
   - Add `cpu_id: u8` field to `RunQueue`
   - Remove global `RUN_QUEUE` static
   - Add `init_per_cpu(cpu_id: u8)` function

4. **Update context switch** to load per-CPU data:
   - In `context_switch()`, update `PER_CPU_DATA.current_task`
   - Update `GS_BASE` MSR with per-CPU pointer if switching CPUs (SMP only)

5. **Update `schedule()`** to use per-CPU run queue:
   ```rust
   pub fn schedule() {
       let cpu_id = current_cpu_id();  // Read from APIC ID or GS_BASE
       let rq = get_per_cpu_run_queue(cpu_id);
       // ... use rq instead of global
   }
   ```

**Testing:**
- Boot on multi-core QEMU (`-smp 4`)
- Verify both cores schedule tasks
- Check `PER_CPU_DATA.kernel_stack` is set correctly on AP

---

### Task 3.2: TSS.RSP0 Swap (Per-Task Kernel Stacks)

**Why:** Currently all tasks share one TSS with a single RSP0. Per-task kernel stacks are required for interrupt safety on SMP.

**Current State:**
- Single TSS in `kernel/src/gdt.rs:18`
- `set_kernel_stack()` updates TSS.RSP0 on every context switch (`gdt.rs:119-124`)
- `SWITCH_HOOK` registered in `task/src/lib.rs:38-48`

**Implementation Plan:**

1. **Remove TSS.RSP0 dependency** from context switch:
   - In `context_switch()`, save/restore TSS.RSP0 explicitly
   - Or: keep TSS.RSP0 as "last used" and swap on interrupt entry

2. **Per-task TSS (SMP-only):**
   - Allocate TSS per CPU (not per task)
   - Each CPU's TSS.RSP0 = current task's kernel stack
   - On context switch, update TSS.RSP0 to new task's kstack

3. **Update `register_switch_hook`** (`task/src/lib.rs:46-48`):
   ```rust
   pub fn register_switch_hook(f: fn(VirtAddr)) {
       SWITCH_HOOK.call_once(|| f);
   }
   // Change to:
   pub fn register_switch_hook(f: fn(VirtAddr)) {
       // Called on every context switch, not once
   }
   ```

4. **Update `context_switch()`** (`task/src/context_switch.rs`):
   ```asm
   /* Save old TSS.RSP0 */
   "mov rax, [gs:PER_CPU_DATA_OFFSET + TSS_RSP0_OFFSET]"
   "mov [rdi + TSS_RSP0_SAVE_OFFSET], rax"
   
   /* Load new TSS.RSP0 */
   "mov rax, [rsi + TSS_RSP0_OFFSET]"
   "mov [gs:PER_CPU_DATA_OFFSET + TSS_RSP0_OFFSET], rax"
   ```

**Testing:**
- Verify interrupts work correctly on AP
- Check TSS.RSP0 matches current task's kstack
- Test SMP boot with multiple tasks

---

### Task 3.3: Weighted Fair Queueing (WFQ)

**Why:** Round-robin treats all tasks equally. WFQ gives priority to RT tasks and weights Fair tasks by priority.

**Current State:**
- `SchedClass::Fair(u8)` with priority 100-139 in `task/src/lib.rs:164-169`
- `pick_next_task()` does linear scan in `task/src/scheduler.rs:580-588`

**Implementation Plan:**

1. **Add virtual runtime tracking** to `TaskCB`:
   ```rust
   pub struct TaskCB {
       // ... existing fields
       pub virtual_runtime: u64,  // Nanoseconds of CPU time
       pub weight: u64,           // Scheduling weight (100-139 → 1-39)
   }
   ```

2. **Implement WFQ scheduler** (`task/src/wfq.rs`):
   ```rust
   pub fn pick_next_wfq(run_queue: &RunQueue, now: u64) -> Option<Arc<Mutex<TaskCB>>> {
       let mut best: Option<Arc<Mutex<TaskCB>>> = None;
       let mut min_vruntime = u64::MAX;
       
       for task in run_queue.queue.iter() {
           let t = task.lock();
           /* RT tasks always run first */
           if let SchedClass::Realtime(_) = t.sched_class {
               return Some(Arc::clone(task));
           }
           /* Fair tasks: pick lowest virtual runtime */
           if t.virtual_runtime < min_vruntime {
               min_vruntime = t.virtual_runtime;
               best = Some(Arc::clone(task));
           }
       }
       best
   }
   ```

3. **Update virtual runtime on each tick**:
   ```rust
   pub fn update_virtual_runtime(task: &mut TaskCB, tick_duration_ns: u64) {
       match task.sched_class {
           SchedClass::Fair(_) => {
               let weight = match task.priority() {
                   100 => 1,
                   110 => 2,
                   120 => 3,
                   130 => 4,
                   _ => 5,
               };
               task.virtual_runtime += tick_duration_ns * 100 / weight;
           }
           _ => {}
       }
   }
   ```

4. **Integrate with timer interrupt**:
   - In LAPIC timer handler, call `update_virtual_runtime()` for running task
   - Call `pick_next_wfq()` instead of `pick_next_task()`

**Testing:**
- Create tasks with different priorities
- Verify RT tasks preempt Fair tasks
- Measure scheduling latency under load

---

### Task 3.4: Priority Inheritance Protocol

**Why:** Priority inversion can cause RT tasks to block on low-priority tasks holding shared locks.

**Current State:**
- No priority inheritance in current scheduler
- `Mutex` in `spin` crate has no priority awareness

**Implementation Plan:**

1. **Add priority inheritance to `TaskCB`**:
   ```rust
   pub struct TaskCB {
       // ... existing fields
       pub inherited_priority: Option<u8>,  // Temporarily boosted priority
       pub blocked_on: Option<Arc<Mutex<TaskCB>>>,  // Task we're blocking
   }
   ```

2. **Implement `acquire_lock_with_pi()`** (priority inheritance):
   ```rust
   pub fn acquire_lock_with_pi(lock: &Mutex<()>, task: &TaskCB) {
       lock.lock();
       /* If lock holder has lower priority, boost it */
       let holder = find_lock_holder(lock);
       if let Some(h) = holder {
           if h.priority() < task.priority() {
               h.inherited_priority = Some(h.priority());
               h.sched_class = SchedClass::Realtime(task.priority());
           }
       }
   }
   ```

3. **Implement `release_lock_with_pi()`**:
   ```rust
   pub fn release_lock_with_pi(lock: &Mutex<()>, task: &TaskCB) {
       /* Restore holder's original priority */
       let holder = find_lock_holder(lock);
       if let Some(h) = holder {
           if let Some(orig) = h.inherited_priority {
               h.sched_class = SchedClass::Fair(orig);
               h.inherited_priority = None;
           }
       }
       lock.unlock();
   }
   ```

4. **Integrate with IPC receive**:
   - When task blocks on `receive_blocking()`, check if sender has lower priority
   - If so, boost sender's priority

**Testing:**
- Create 3 tasks: low, medium, high priority
- Low holds lock, medium blocks on lock, high blocks on IPC from low
- Verify low gets boosted to high priority

---

### Task 3.5: SMP Bring-Up (AP Bootstrap)

**Why:** Current kernel only runs on BSP (Bootstrap Processor). SMP enables true parallel execution.

**Current State:**
- Single CPU only (`task/src/scheduler.rs:25-26` has SMP TODOs)
- No AP bootstrap code
- LAPIC IDs not enumerated

**Implementation Plan:**

1. **Enumerate APs via LAPIC** (`apic/src/topology.rs`):
   ```rust
   pub fn enumerate_apics() -> Vec<u8> {
       let mut aps = Vec::new();
       /* Read LAPIC version and count from MP configuration table */
       /* ACPI MADT not implemented yet — use fixed APIC IDs 1..=max */
       let max_id = read_lapic_max_id();
       for id in 1..=max_id {
           if is_ap_alive(id) {
               aps.push(id);
           }
       }
       aps
   }
   ```

2. **Write AP bootstrap stub** (`boot/ap_bootstrap.S`):
   ```asm
   .section .text
   .global ap_bootstrap
   ap_bootstrap:
       /* Switch to 32-bit protected mode */
       /* Load GDT with kernel segments */
       /* Setup IDT */
       /* Enable APIC */
       /* Jump to per_cpu_init */
       jmp per_cpu_init
   ```

3. **Send INIT-SIPI-SIPI sequence** (`apic/src/smp.rs`):
   ```rust
   pub unsafe fn wakeup_ap(lapic_id: u8, bootstrap_addr: u64) {
       /* 1. Send INIT IPI */
       write_icr(0xF5, lapic_id);  /* INIT, level=deassert */
       usleep(10_000);
       
       /* 2. Send first SIPI */
       let vector = (bootstrap_addr / 4096) as u8;
       write_icr(0x00 | (vector << 8), lapic_id);  /* SIPI, vector */
       usleep(200);
       
       /* 3. Send second SIPI (required) */
       write_icr(0x00 | (vector << 8), lapic_id);
       usleep(200);
   }
   ```

4. **Per-AP initialization** (`kernel/src/smp.rs`):
   ```rust
   pub unsafe fn per_cpu_init() {
       /* Setup per-CPU data */
       let cpu_id = read_lapic_id();
       PER_CPU_DATA = PerCpuData {
           kernel_stack: allocate_kernel_stack(),
           run_queue: &per_cpu_run_queues[cpu_id],
           current_task: 0,
           lapic_id: cpu_id,
           ap_ready: true,
       };
       
       /* Load per-CPU GDT/IDT */
       gdt::init_per_cpu();
       idt::init_idt_per_cpu();
       
       /* Enable interrupts */
       x86_64::instructions::interrupts::enable();
       
       /* Enter scheduler */
       task::scheduler::start();
   }
   ```

5. **Boot sequence** (`kernel/src/main.rs`):
   ```rust
   /* After BSP init, wake APs */
   let aps = apic::topology::enumerate_apics();
   for ap_id in aps {
       unsafe {
           apic::smp::wakeup_ap(ap_id, ap_bootstrap_addr);
       }
   }
   ```

**Testing:**
- Boot with `-smp 4` in QEMU
- Verify all 4 CPUs reach scheduler
- Run tasks on multiple CPUs, verify parallelism

---

### Task 3.6: IPC Fastpath

**Why:** Current IPC goes through message queue + wake_task, which requires scheduler re-entry. Direct register transfer is faster when receiver is blocked.

**Current State:**
- `send()` enqueues message, pops waiter from `waiting_receivers`, calls `wake_task()` (`ipc/src/lib.rs:102-127`)
- `wake_task()` re-enqueues task on RunQueue (`task/src/scheduler.rs:164-166`)

**Implementation Plan:**

1. **Add direct transfer flag** to `Message`:
   ```rust
   pub struct Message {
       // ... existing fields
       pub direct: bool,  // True if receiver is blocked at receive()
   }
   ```

2. **Detect blocked receiver** in `send()`:
   ```rust
   pub fn send(&self, msg: Message) -> Result<(), &'static str> {
       /* Check if receiver is blocked */
       let waiter = self.waiting_receivers.lock().pop_front();
       if let Some(receiver) = waiter {
           let is_blocked = receiver.lock().state == TaskState::Blocked;
           if is_blocked {
               /* Fastpath: direct register transfer */
               return self.direct_transfer(msg, receiver);
           }
       }
       /* Slowpath: enqueue message */
       // ... existing code
   }
   ```

3. **Implement direct transfer**:
   ```rust
   fn direct_transfer(&self, msg: Message, receiver: Arc<Mutex<TaskCB>>) {
       /* Copy message directly into receiver's context */
       let mut r = receiver.lock();
       r.context.rdi = msg as *const Message as u64;  /* Pass via register */
       r.state = TaskState::Ready;
       drop(r);
       
       /* Mark sender as interrupted, receiver as ready */
       /* Context switch will resume receiver immediately */
   }
   ```

4. **Receiver detects direct message**:
   ```rust
   pub fn receive_blocking(&self) -> Message {
       loop {
           /* Check for direct message in context */
           if let Some(msg) = check_direct_message() {
               return msg;
           }
           /* Slowpath: enqueue, block, etc. */
       }
   }
   ```

**Testing:**
- Microbenchmark: 1M IPC sends/receives
- Measure latency with/without fastpath

---

### Task 3.7: Asynchronous Notification Ports

**Why:** Some devices (network, disk) need to notify Ring 3 tasks without queuing messages.

**Current State:**
- No async notification support
- All IPC is message-queue based

**Implementation Plan:**

1. **Add notification port type** to `CapabilityType`:
   ```rust
   pub enum CapabilityType {
       // ... existing variants
       AsyncNotification {
           port_id: u64,
           bitmask: u64,  /* Which events to notify */
       },
   }
   ```

2. **Create async notification port**:
   ```rust
   pub fn create_notification_port(id: u64, bitmask: u64) -> Arc<Port> {
       let port = Port::new(id, owner_id);
       port.notification_bitmask = bitmask;
       /* Register in capability store */
       port
   }
   ```

3. **Send notification** (no queue, just set bitmask):
   ```rust
   pub fn notify(&self, event_mask: u64) {
       self.notification_bitmask.fetch_or(event_mask, Ordering::Relaxed);
       /* Wake task if blocked in select/poll */
   }
   ```

4. **Receiver checks notification**:
   ```rust
   pub fn check_notification(&self) -> u64 {
       self.notification_bitmask.swap(0, Ordering::Relaxed)
   }
   ```

**Testing:**
- Create notification port, send events, verify wake-up

---

## Phase 4: Storage & Filesystem Stack

### Current Implementation Status

**Completed:**
- ✅ FAT32 driver (BPB, cluster chains, LFN, mkdir, unlink)
- ✅ Ext4 daemon (superblock, extent tree, file read/write)
- ✅ Mount table (longest-prefix matching in `vfs/src/lib.rs:102-133`)
- ✅ File descriptor table (global, keyed by `(task_id, fd)`)
- ✅ `SYS_OPEN`, `SYS_CLOSE`, `SYS_SEEK` syscalls

**Remaining:**
- ⏳ Ext4: HTree directory indexing
- ⏳ Ext4: `rmdir()` semantics + link-count checks
- ⏳ Ext4: Superblock generation / formatting (mkfs)
- ⏳ Ext4: Journal (JBD2)
- ⏳ Unified Page Cache (radix tree, demand paging, writeback)
- ⏳ `mmap()` file-backed mapping

---

### Task 4.1: Ext4 HTree Directory Indexing

**Why:** Current ext4 dir implementation uses linear scan. HTree provides O(log n) lookup for large directories.

**Current State:**
- `fs/src/ext4/dir.rs` implements linear `for_each_entry` loop
- No HTree/dx_root support

**Implementation Plan:**

1. **Add HTree structures** (`fs/src/ext4/htree.rs`):
   ```rust
   #[repr(C)]
   pub struct dx_root {
       pub inode: u32,
       pub reserved_zero: u32,
       pub hash_version: u8,
       pub info_len: u8,
       pub entries_count: u16,
       pub flevel: u16,
       pub dir_info: [u8; 12],  /* dx_dir_info */
   }
   
   #[repr(C)]
   pub struct dx_entry {
       pub hash: u32,
       pub block: u32,
   }
   
   #[repr(C)]
   pub struct dx_frame {
       pub inode: u32,
       pub level: u16,
       pub next: u16,
       pub limit: u16,
       pub count: u16,
       pub reserved_zero: u16,
       pub entries: [dx_entry; 0],  /* Variable length */
   }
   ```

2. **Implement hash function** (Jenkins one-at-a-time):
   ```rust
   pub fn hash_name(name: &[u8], sb: &Superblock) -> u32 {
       let mut hash: u32 = 5381;
       for &b in name {
           hash = hash.wrapping_mul(33).wrapping_add(b as u32);
       }
       /* Apply mask based on hash format (2/4/6/8 bit) */
       let mask = match sb.hash_format {
           2 => 0x3,
           4 => 0xF,
           6 => 0x3F,
           8 => 0xFF,
           _ => 0xFF,
       };
       hash & mask
   }
   ```

3. **Implement lookup with HTree**:
   ```rust
   pub fn lookup_htree(dev: &dyn BlockDev, sb: &Superblock, dir_ino: &Inode, name: &str) -> Option<u32> {
       let hash = hash_name(name.as_bytes(), sb);
       /* Traverse hash tree to find leaf block */
       let leaf_blk = traverse_tree(dev, sb, dir_ino, hash)?;
       /* Linear scan leaf for matching name */
       lookup_in_dir(dev, sb, leaf_blk, name)
   }
   ```

4. **Integrate with existing dir ops**:
   - In `lookup_in_dir()`, check if directory uses HTree (`dir_info.hash_version != 0`)
   - If so, use `lookup_htree()` instead of linear scan

**Testing:**
- Create directory with 1000+ files
- Measure lookup time vs linear scan
- Verify correct inode returned

---

### Task 4.2: Ext4 rmdir() and Link-Count Checks

**Why:** Current ext4 doesn't support `rmdir()` or enforce link counts. Directories can be deleted while still linked.

**Current State:**
- No `rmdir()` implementation
- Link count not checked on unlink

**Implementation Plan:**

1. **Add `rmdir` syscall** (`kernel/src/syscall.rs`):
   ```rust
   pub const SYS_RMDIR: u64 = 22;
   
   SYS_RMDIR => {
       let path = /* parse from arg1 */;
       match vfs::lookup_path(parent_path) {
           Some(dir) => match dir.rmdir(name) {
               Ok(()) => 0,
               Err(_) => ERRNO_EINVAL,
           },
           None => ERRNO_ENOENT,
       }
   }
   ```

2. **Implement `rmdir` in VFS trait** (`vfs/src/lib.rs`):
   ```rust
   fn rmdir(&self, _name: &str) -> Result<(), &'static str> {
       Err("not a directory")
   }
   ```

3. **Implement ext4 rmdir**:
   ```rust
   pub fn rmdir(
       dev: &dyn BlockDev,
       sb: &mut Superblock,
       bgdt: &mut BgDescTable,
       dir_ino: &mut Inode,
       name: &str,
   ) -> Result<(), &'static str> {
       /* 1. Lookup child inode */
       let child_ino = lookup_in_dir(dev, sb, bgdt, dir_ino, name)?;
       let mut child = Inode::read(dev, sb, bgdt, child_ino)?;
       
       /* 2. Check if directory is empty (only . and ..) */
       let entries = readdir(dev, sb, bgdt, &child);
       if entries.len() > 2 {
           return Err("Directory not empty");
       }
       
       /* 3. Decrement parent's link count */
       let mut parent = Inode::read(dev, sb, bgdt, dir_ino.ino)?;
       parent.links_count -= 1;
       parent.write(dev, sb, bgdt);
       
       /* 4. Remove directory entry from parent */
       remove_entry(dev, sb, bgdt, dir_ino, name);
       
       /* 5. Decrement child's link count (. entry) */
       child.links_count -= 1;
       child.write(dev, sb, bgdt);
       
       /* 6. Mark inode as free */
       bitmap_alloc::free_inode(dev, sb, bgdt, child_ino);
       
       Ok(())
   }
   ```

4. **Add link count check to unlink**:
   ```rust
   pub fn unlink(...) -> bool {
       /* Check link count > 0 */
       if inode.links_count == 0 {
           return false;  /* Already unlinked */
       }
       /* ... existing unlink logic ... */
       /* Decrement link count */
       inode.links_count -= 1;
       inode.write(dev, sb, bgdt);
   }
   ```

**Testing:**
- Create dir with files, try rmdir (should fail)
- Remove all files, rmdir (should succeed)
- Verify link counts match actual directory structure

---

### Task 4.3: Ext4 Superblock Generation / Formatting

**Why:** Current ext4 expects pre-formatted disk. Need mkfs equivalent for blank VirtIO-blk devices.

**Current State:**
- No formatting support
- `Superblock::read()` expects valid superblock at offset `0x400`

**Implementation Plan:**

1. **Add `format` function** (`fs/src/ext4/format.rs`):
   ```rust
   pub fn format_device(
       dev: &mut dyn BlockDev,
       block_size: u32,
       inode_count: u32,
       block_count: u64,
       label: &[u8],
   ) -> Result<(), &'static str> {
       /* 1. Write superblock */
       write_superblock(dev, block_size, inode_count, block_count, label)?;
       
       /* 2. Write block group descriptors */
       write_bgdt(dev, block_size, inode_count, block_count)?;
       
       /* 3. Allocate root inode (inode 2) */
       let root_ino = alloc_inode(dev, &mut sb, bgdt)?;
       root_ino.mode = EXT4_S_IFDIR | 0o755;
       root_ino.links_count = 2;  /* . and .. */
       root_ino.flags |= EXT4_EXTENTS_FL;
       root_ino.write(dev, &mut sb, bgdt);
       
       /* 4. Create root directory entries */
       create_dir_entry(dev, &mut sb, bgdt, &root_ino, ".", root_ino.ino, EXT4_FT_DIR);
       create_dir_entry(dev, &mut sb, bgdt, &root_ino, "..", root_ino.ino, EXT4_FT_DIR);
       
       Ok(())
   }
   ```

2. **Implement `write_superblock`**:
   ```rust
   fn write_superblock(dev: &mut dyn BlockDev, block_size: u32, 
                       inode_count: u32, block_count: u64, label: &[u8]) {
       let mut sb = Superblock::new();
       sb.s_magic = 0xEF53;
       sb.s_block_size = block_size;
       sb.s_inode_count = inode_count;
       sb.s_block_count = block_count;
       sb.s_inodes_per_group = inode_count / num_groups;
       sb.s_first_ino = 11;  /* Reserved inodes */
       sb.s_feature_compat = 0;
       sb.s_feature_incompat = INCOMPAT_EXTENTS;
       sb.s_feature_ro_compat = 0;
       
       /* Write to disk offset 0x400 */
       let mut buf = [0u8; 1024];
       sb.serialize(&mut buf);
       dev.write_block(0x400 / 512, &buf);
   }
   ```

3. **Add `format` syscall**:
   ```rust
   pub const SYS_FORMAT: u64 = 23;
   
   SYS_FORMAT => {
       /* arg1: device path, arg2: block_size, arg3: flags */
       let dev_path = /* parse path */;
       let block_size = arg2;
       
       let dev = vfs::lookup_path(dev_path)?;
       format_device(dev, block_size, ...)?;
       0
   }
   ```

**Testing:**
- Format blank disk image
- Mount and verify root directory
- Create files, verify on Linux host

---

### Task 4.4: Ext4 Journal (JBD2)

**Why:** Metadata consistency requires journaling. Without it, crashes can corrupt filesystem.

**Current State:**
- No journaling
- All writes go directly to disk

**Implementation Plan:**

1. **Add journal structures** (`fs/src/ext4/journal.rs`):
   ```rust
   #[repr(C)]
   pub struct journal_superblock {
       pub j_magic: u32,  /* 0xFF0FBDFF */
       pub j_blocksize: u32,
       pub j_journal_inum: u32,  /* Inode of journal file */
       pub j_max_size: u32,      /* Max journal size in blocks */
       pub j_start: u32,         /* First block of journal */
       pub j_tail: u32,          /* First unused block */
       pub j_sequence: u32,      /* Next transaction ID */
   }
   
   #[repr(C)]
   pub struct journal_transaction {
       pub t_id: u32,
       pub t_state: JournalState,
       pub t_buffers: Vec<JournalBuffer>,
   }
   
   #[repr(C)]
   pub struct journal_buffer {
       pub block: u32,
       pub data: Vec<u8>,
       pub checksum: u32,
   }
   ```

2. **Implement transaction commit** (ordered mode):
   ```rust
   pub fn commit_transaction(dev: &mut dyn BlockDev, sb: &mut Superblock, tx: &JournalTransaction) {
       /* 1. Write commit record */
       write_commit_record(dev, sb, tx);
       
       /* 2. Write all data blocks */
       for buf in &tx.buffers {
           dev.write_block(buf.block, &buf.data);
       }
       
       /* 3. Write superblock (update journal tail) */
       sb.j_journal_tail = tx.t_id;
       write_superblock(dev, sb);
   }
   ```

3. **Add `begin_transaction` to VFS ops**:
   ```rust
   pub fn begin_transaction() -> Option<JournalTransaction> {
       /* Allocate transaction, return handle */
   }
   
   pub fn journal_write(dev: &mut dyn BlockDev, tx: &mut JournalTransaction, block: u32, data: &[u8]) {
       /* Buffer write, don't flush yet */
       tx.buffers.push(JournalBuffer { block, data: data.to_vec(), checksum: 0 });
   }
   ```

4. **Integrate with ext4 write ops**:
   ```rust
   pub fn write(...) -> usize {
       let tx = begin_transaction().expect("Journal full");
       let n = write_data(dev, sb, bgdt, inode, offset, buf);
       journal_write(dev, &mut tx, inode_block, inode_bytes);
       commit_transaction(dev, sb, tx);
       n
   }
   ```

**Testing:**
- Write file, crash kernel, reboot
- Verify file contents intact
- Measure journal overhead (~10-20% slower)

---

### Task 4.5: Unified Page Cache

**Why:** Disk I/O is slow. Caching frequent reads/writes in memory reduces latency.

**Current State:**
- No page cache
- Every read/write goes to disk

**Implementation Plan:**

1. **Add page cache data structure** (`vfs/src/page_cache.rs`):
   ```rust
   pub struct PageCache {
       /* Radix tree: (InodeId, page_offset) -> Page */
       tree: Mutex<BTreeMap<(u32, u32), Arc<Mutex<Page>>>>,
       /* LRU list for eviction */
       lru: Mutex<VecDeque<(u32, u32)>>,
   }
   
   pub struct Page {
       data: [u8; PAGE_SIZE],  /* 4096 bytes */
       inode_id: u32,
       offset: u32,
       dirty: bool,
       accessed_at: u64,  /* LAPIC ticks */
   }
   ```

2. **Implement page lookup**:
   ```rust
   pub fn get_page(inode_id: u32, offset: u32) -> Option<Arc<Mutex<Page>>> {
       let key = (inode_id, offset);
       let cache = CACHE.get()?;
       if let Some(page) = cache.tree.lock().get(&key).cloned() {
           page.lock().accessed_at = current_lapic_ticks();
           Some(page)
       } else {
           None
       }
   }
   ```

3. **Implement page insertion**:
   ```rust
   pub fn insert_page(page: Page) {
       let key = (page.inode_id, page.offset);
       let cache = CACHE.get().expect("Page cache not initialized");
       cache.tree.lock().insert(key, Arc::new(Mutex::new(page)));
       cache.lru.lock().push_back(key);
   }
   ```

4. **Integrate with VFS read**:
   ```rust
   pub fn read(inode: &Arc<dyn INode>, offset: usize, buf: &mut [u8]) -> usize {
       let page_offset = offset / PAGE_SIZE;
       if let Some(page) = get_page(inode.ino(), page_offset as u32) {
           /* Hit: copy from cache */
           let p = page.lock();
           let start = offset % PAGE_SIZE;
           let len = core::cmp::min(buf.len(), PAGE_SIZE - start);
           buf[..len].copy_from_slice(&p.data[start..start + len]);
           return len;
       }
       /* Miss: read from disk, cache it */
       let data = read_from_disk(inode, offset, buf.len());
       let page = Page {
           data: data.clone(),
           inode_id: inode.ino(),
           offset: page_offset as u32,
           dirty: false,
           accessed_at: current_lapic_ticks(),
       };
       insert_page(page);
       buf[..data.len()].copy_from_slice(&data);
       data.len()
   }
   ```

5. **Implement writeback**:
   ```rust
   pub fn writeback_dirty_pages() {
       let cache = CACHE.get().expect("Page cache not initialized");
       let mut lru = cache.lru.lock();
       while let Some(key) = lru.pop_front() {
           if let Some(page) = cache.tree.lock().get(&key).cloned() {
               let mut p = page.lock();
               if p.dirty {
                   write_to_disk(p.inode_id, p.offset, &p.data);
                   p.dirty = false;
               }
           }
       }
   }
   ```

**Testing:**
- Read same file multiple times, verify cache hits
- Measure latency improvement
- Test eviction under memory pressure

---

### Task 4.6: mmap() File-Backed Mapping

**Why:** mmap() allows zero-copy file I/O and is required by Linux ABI (Phase 5).

**Current State:**
- No mmap support
- All I/O goes through read()/write()

**Implementation Plan:**

1. **Add mmap syscall** (`kernel/src/syscall.rs`):
   ```rust
   pub const SYS_MMAP: u64 = 90;
   
   SYS_MMAP => {
       /* arg1: file descriptor, arg2: offset, arg3: length */
       let fd = arg1;
       let offset = arg2;
       let length = arg3;
       
       let task_id = task::scheduler::current_task_id();
       let file = crate::fd::get(task_id, fd)?;
       
       /* 1. Allocate user VMA */
       let vaddr = allocate_user_vma(length)?;
       
       /* 2. Map file pages into VMA */
       map_file_pages(file.inode(), offset, length, vaddr)?;
       
       vaddr
   }
   ```

2. **Implement VMA allocation** (`memory/src/vma.rs`):
   ```rust
   pub fn allocate_user_vma(size: u64) -> Result<VirtAddr, &'static str> {
       /* Find free region in userspace (0x0000_0000_0000_0000 - 0x0000_8000_0000_0000) */
       /* Align to 4KiB */
       /* Map pages as WRITABLE | USER_ACCESSIBLE */
   }
   ```

3. **Implement file page mapping**:
   ```rust
   pub fn map_file_pages(inode: &Arc<dyn INode>, offset: u64, length: u64, vaddr: VirtAddr) {
       let n_pages = (length + PAGE_SIZE - 1) / PAGE_SIZE;
       for i in 0..n_pages {
           let page_offset = offset + i * PAGE_SIZE;
           let page_vaddr = vaddr + i * PAGE_SIZE;
           
           /* Read file page */
           let mut data = [0u8; PAGE_SIZE];
           inode.read(page_offset as usize, &mut data);
           
           /* Map page frame */
           let frame = allocate_frame()?;
           map_page(page_vaddr, frame, data)?;
       }
   }
   ```

4. **Handle page faults for mmap'd pages**:
   - In page fault handler, check if faulting address is in a mapped VMA
   - If so, read from file and populate page
   - Mark page as dirty if write

**Testing:**
- mmap() a file, read/write via pointer
- Verify changes persist to disk
- Test with large files (multiple pages)

---

## Implementation Order & Dependencies

```
Phase 3:
  3.1 Per-CPU Run Queues ──────────→ 3.2 TSS.RSP0 (SMP requires both)
  3.3 WFQ ──────────────────────────→ 3.4 Priority Inheritance (builds on WFQ)
  3.5 SMP Bring-Up ─────────────────→ 3.1 (per-CPU queues)
  3.6 IPC Fastpath ─────────────────→ Independent (can do anytime)
  3.7 Async Notification ───────────→ Independent

Phase 4:
  4.1 HTree ────────────────────────→ 4.2 rmdir (uses HTree for large dirs)
  4.2 rmdir ────────────────────────→ 4.3 Formatting (need rmdir for cleanup)
  4.3 Formatting ───────────────────→ 4.4 Journal (journal needs format to init)
  4.4 Journal ──────────────────────→ 4.5 Page Cache (writeback uses cache)
  4.5 Page Cache ───────────────────→ 4.6 mmap (mmap reads from cache)
  4.6 mmap ─────────────────────────→ Phase 5 (Linux ABI)
```

**Recommended Order:**
1. 3.6 IPC Fastpath (quick win, no SMP needed)
2. 3.3 WFQ (improves scheduling, no SMP needed)
3. 3.5 SMP Bring-Up (enables parallelism)
4. 3.1 Per-CPU Run Queues (requires SMP)
5. 3.2 TSS.RSP0 (requires SMP)
6. 3.4 Priority Inheritance (builds on WFQ)
7. 3.7 Async Notification (optional, for devices)
8. 4.3 Formatting (prerequisite for testing)
9. 4.1 HTree (performance improvement)
10. 4.2 rmdir (completes directory ops)
11. 4.4 Journal (data safety)
12. 4.5 Page Cache (performance)
13. 4.6 mmap (Linux ABI compatibility)

---

## Testing Strategy

**Unit Tests (in-crate):**
- `task/`: scheduler selection, WFQ priority, priority inheritance
- `ipc/`: direct transfer, async notifications
- `fs/ext4/`: HTree hash, rmdir semantics, journal commit

**Integration Tests (QEMU boot):**
- SMP: boot with `-smp 4`, verify all CPUs schedule
- IPC: producer/consumer with fastpath
- Filesystem: format disk, create directory tree, rmdir, verify on Linux

**Performance Benchmarks:**
- Context switch latency (target: <500ns on P-core)
- IPC throughput (messages/sec with/without fastpath)
- File I/O: sequential read/write MB/s with/without page cache

---

## Risk & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| SMP boot fails on real hardware | Blocks Phase 3 | Test in QEMU first, use ACPI MADT when available |
| JBD2 adds 20%+ overhead | Performance regression | Profile before/after, optimize commit path |
| Page cache evicts too aggressively | Miss rate high | Tune LRU, add clock algorithm |
| HTree complexity | Bug-prone | Start with linear scan, add HTree as optimization |

---

## Next Steps

1. **Immediate:** Implement 3.6 IPC Fastpath (1-2 days)
2. **Short-term:** 3.3 WFQ + 3.5 SMP (1 week)
3. **Medium-term:** 4.3 Formatting + 4.1 HTree (1 week)
4. **Long-term:** 4.4 Journal + 4.5 Page Cache (2 weeks)

**Total estimated effort:** 4-5 weeks for full Phase 3/4 completion.
