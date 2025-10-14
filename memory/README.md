# Memory Management Module

## Overview

The memory module provides comprehensive memory management infrastructure for the Serix kernel, including virtual memory (paging), physical memory allocation (frame allocation), and dynamic memory allocation (heap). This module is fundamental to isolating processes, protecting kernel memory, and providing efficient memory allocation services.

## Architecture

### Components

1. **Page Table Management** (`lib.rs`): Virtual memory initialization and mapping
2. **Heap Allocator** (`heap.rs`): Dynamic memory allocation for kernel
3. **Frame Allocator** (`lib.rs`, `heap.rs`): Physical memory page allocation

### Memory Management Hierarchy

```
┌─────────────────────────────────────┐
│   Kernel Code (Vec, Box, etc.)      │
├─────────────────────────────────────┤
│   Heap Allocator                    │
│   (linked_list_allocator)           │
├─────────────────────────────────────┤
│   Page Table Mapper                 │
│   (OffsetPageTable)                 │
├─────────────────────────────────────┤
│   Frame Allocator                   │
│   (Physical page management)        │
├─────────────────────────────────────┤
│   Physical Memory                   │
│   (RAM provided by bootloader)      │
└─────────────────────────────────────┘
```

## Module Structure

```
memory/
├── src/
│   ├── lib.rs      # Page table management, frame allocator
│   └── heap.rs     # Heap allocator and initialization
└── Cargo.toml
```

## Virtual Memory (lib.rs)

### x86_64 Paging Overview

x86_64 uses a 4-level page table structure:

```
Virtual Address (48-bit used):
Bits 47-39: PML4 index (512 entries)
Bits 38-30: PDP  index (512 entries)
Bits 29-21: PD   index (512 entries)
Bits 20-12: PT   index (512 entries)
Bits 11-0:  Page offset (4096 bytes)

Total addressable: 256 TiB (with 4-level paging)
```

**Page Table Entry (PTE) Format (64-bit)**:
```
Bit 0:     Present (P)
Bit 1:     Writable (R/W)
Bit 2:     User/Supervisor (U/S)
Bit 3:     Page-level write-through (PWT)
Bit 4:     Page-level cache disable (PCD)
Bit 5:     Accessed (A)
Bit 6:     Dirty (D)
Bit 7:     Page size (PS) or Page attribute table (PAT)
Bit 8:     Global (G)
Bits 9-11: Available for software use
Bits 12-51: Physical page frame address (4KB aligned)
Bits 52-62: Available for software use
Bit 63:    Execute disable (XD/NX)
```

### Active Page Table Access

```rust
unsafe fn active_level_table(offset: VirtAddr) -> &'static mut PageTable
```

**Purpose**: Returns a mutable reference to the currently active level-4 (PML4) page table.

**Implementation**:
```rust
let (frame, _) = Cr3::read();  // Read CR3 register (page table base)
let phys = frame.start_address().as_u64();
let virt = offset.as_u64() + phys;  // Physical -> Virtual conversion
&mut *(virt as *mut PageTable)
```

**CR3 Register**: Contains the physical address of the PML4 page table.

**Physical Memory Offset**: Serix uses an "offset page table" approach where all physical memory is mapped at a fixed virtual offset:

```
Physical Address: 0x0000_0000_0000_0000
Virtual Address:  0xFFFF_8000_0000_0000 + 0x0000_0000_0000_0000
                = 0xFFFF_8000_0000_0000

Example:
Physical: 0x0000_0000_0010_0000 (1 MB)
Virtual:  0xFFFF_8000_0010_0000
```

**Why Offset Mapping?**
- Simple physical-to-virtual conversion (addition)
- Allows kernel to access any physical address
- No need for temporary mappings
- Fast page table manipulation

### Offset Page Table Initialization

```rust
pub unsafe fn init_offset_page_table(offset: VirtAddr) -> OffsetPageTable<'static>
```

**Purpose**: Initializes an `OffsetPageTable` with the active page table and physical memory offset.

**Usage**:
```rust
let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };
```

**OffsetPageTable Benefits**:
- Implements `Mapper` trait for page mapping operations
- Automatically translates physical addresses
- Provides safe interface for page table manipulation

### Frame Allocator (Boot-time)

```rust
pub struct BootFrameAllocator {
    frames: &'static [PhysFrame],
    next: usize,
}
```

**Purpose**: Allocates physical memory frames (4KB pages) from usable memory regions.

**Why Two Frame Allocators?**
1. **StaticBootFrameAllocator** (heap.rs): Used during boot before heap is initialized
2. **BootFrameAllocator** (lib.rs): Used after heap is initialized (can use Vec)

#### Initialization

```rust
pub fn new(memory_map: &[&Entry]) -> Self
```

**Process**:
1. Iterate through memory map entries
2. Filter for USABLE regions
3. Split regions into 4KB frames
4. Store frames in a Vec
5. Leak Vec to get `&'static [PhysFrame]`

**Implementation**:
```rust
let mut frames = alloc::vec::Vec::new();

for region in memory_map.iter()
    .filter(|r| r.entry_type == limine::memory_map::EntryType::USABLE)
{
    let start = region.base;
    let end = region.base + region.length;
    let start_frame = PhysFrame::containing_address(PhysAddr::new(start));
    let end_frame = PhysFrame::containing_address(PhysAddr::new(end - 1));
    
    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        frames.push(frame);
    }
}

let static_frames = Box::leak(frames.into_boxed_slice());
BootFrameAllocator {
    frames: static_frames,
    next: 0,
}
```

**Box::leak**: Converts `Box<[PhysFrame]>` to `&'static [PhysFrame]`, transferring ownership to the static lifetime.

#### Frame Allocation

```rust
unsafe impl FrameAllocator<Size4KiB> for BootFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame>
}
```

**Algorithm**: Simple bump allocator
```rust
if self.next < self.frames.len() {
    let frame = self.frames[self.next];
    self.next += 1;
    Some(frame)
} else {
    None
}
```

**Limitations**:
- No deallocation (frames can't be freed)
- Simple bump allocation (no best-fit, first-fit, etc.)
- Suitable only for boot-time allocation

**Future**: Replace with buddy allocator or slab allocator for production use.

## Heap Management (heap.rs)

### Heap Layout

```rust
const HEAP_START: usize = 0x4444_4444_0000;  // Virtual address
const HEAP_SIZE: usize = 1024 * 1024;        // 1 MB
```

**Virtual Address Space Layout**:
```
0x0000_0000_0000_0000: Null page (unmapped)
...
0x4444_4444_0000:      Heap start (1 MB)
0x4444_4454_0000:      Heap end
...
0xFFFF_8000_0000_0000: Physical memory mapping
...
0xFFFF_FFFF_FFFF_FFFF: End of address space
```

**Why 0x4444_4444_0000?**
- Arbitrary non-zero address
- Not conflicting with typical user/kernel boundaries
- Easy to recognize in debugging (repeating pattern)

### Static Boot Frame Storage

```rust
pub const MAX_BOOT_FRAMES: usize = 65536;  // ~256 MB of frames
pub static mut BOOT_FRAMES: [Option<PhysFrame>; MAX_BOOT_FRAMES] = [None; MAX_BOOT_FRAMES];
```

**Purpose**: Pre-heap storage for physical frames.

**Bootstrap Problem**:
1. Need heap to allocate memory
2. Need to allocate frames to map heap
3. Can't allocate without heap (circular dependency)

**Solution**: Use static array to store frames before heap initialization.

**Size**: 65536 frames × 4KB = 256 MB of trackable physical memory.

### Global Heap Allocator

```rust
#[global_allocator]
pub static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();
```

**Purpose**: Global allocator instance used by Rust's `alloc` crate.

**`#[global_allocator]`**: Tells Rust to use this allocator for:
- `Box<T>`
- `Vec<T>`
- `String`
- `Arc<T>`, `Rc<T>`
- All heap allocations

**LockedHeap**: Thread-safe heap allocator from `linked_list_allocator` crate.

**Linked List Allocator**:
- Maintains a linked list of free memory blocks
- Allocation: First-fit algorithm
- Deallocation: Merges adjacent free blocks
- Overhead: Small (pointer per free block)

### Heap Initialization

```rust
pub fn init_heap(
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
)
```

**Purpose**: Maps heap memory and initializes the global allocator.

**Process**:

#### Step 1: Calculate Page Range

```rust
let page_range = {
    let heap_start = VirtAddr::new(HEAP_START as u64);
    let heap_end = VirtAddr::new((HEAP_START + HEAP_SIZE - 1) as u64);
    let start_page = Page::containing_address(heap_start);
    let end_page = Page::containing_address(heap_end);
    Page::range_inclusive(start_page, end_page)
};
```

**Page Count**: For 1 MB heap, this is 256 pages (1024 * 1024 / 4096).

#### Step 2: Map Each Page

```rust
for page in page_range {
    // Allocate a physical frame
    let frame = frame_allocator
        .allocate_frame()
        .expect("No frames available");
    
    // Set page table flags
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    
    // Map the page to the frame
    unsafe {
        mapper
            .map_to(page, frame, flags, frame_allocator)
            .expect("Mapping failed")
            .flush();  // Flush TLB entry
    }
}
```

**Page Table Flags**:
- **PRESENT**: Page is present in memory (not swapped out)
- **WRITABLE**: Page is writable (read-write access)

**Additional Possible Flags**:
- `USER_ACCESSIBLE`: User mode can access (not set for kernel heap)
- `WRITE_THROUGH`: Write-through caching
- `NO_CACHE`: Disable caching
- `GLOBAL`: Don't flush from TLB on context switch
- `NO_EXECUTE`: Page is not executable (NX bit)

**TLB Flush**: After modifying page tables, must flush TLB (Translation Lookaside Buffer) to ensure CPU uses new mapping.

#### Step 3: Initialize Allocator

```rust
unsafe {
    HEAP_ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
}
```

**Effect**: Tells the linked list allocator:
- Where the heap starts (virtual address)
- How large it is
- Creates initial free block spanning entire heap

### Static Boot Frame Allocator

```rust
pub struct StaticBootFrameAllocator {
    next: usize,
    limit: usize,
}
```

**Purpose**: Pre-heap frame allocator that uses static `BOOT_FRAMES` array.

**Why Separate from BootFrameAllocator?**
- Can't use Vec before heap is initialized
- Static array available immediately after boot
- Simple, predictable behavior

#### Initialization

```rust
pub fn new(frame_count: usize) -> Self
```

**Parameters**: `frame_count` = number of frames stored in `BOOT_FRAMES` array.

#### Frame Allocation

```rust
unsafe impl FrameAllocator<Size4KiB> for StaticBootFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while self.next < self.limit {
            unsafe {
                if let Some(frame) = BOOT_FRAMES[self.next].take() {
                    self.next += 1;
                    return Some(frame);
                }
            }
            self.next += 1;
        }
        None
    }
}
```

**Algorithm**:
1. Iterate through `BOOT_FRAMES` starting at `next`
2. Take the first `Some(frame)` (removes it from array)
3. Return the frame
4. If all frames exhausted, return `None`

**Option::take()**: Replaces `Some(value)` with `None` and returns `Some(value)`.

## Memory Initialization Sequence

### Phase 1: Frame Pre-allocation (Kernel main.rs)

```rust
let mut frame_count = 0;
for region in memory_map_entries.iter()
    .filter(|r| r.entry_type == EntryType::USABLE)
{
    let start_frame = PhysFrame::containing_address(PhysAddr::new(region.base));
    let end_frame = PhysFrame::containing_address(PhysAddr::new(region.base + region.length - 1));
    
    for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
        if frame_count < memory::heap::MAX_BOOT_FRAMES {
            unsafe {
                memory::heap::BOOT_FRAMES[frame_count] = Some(frame);
            }
            frame_count += 1;
        }
    }
}
```

**Why in kernel?** Needs access to bootloader-provided memory map.

### Phase 2: Page Table Initialization

```rust
let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };
```

**Sets up**: Virtual memory infrastructure for subsequent operations.

### Phase 3: Frame Allocator Creation

```rust
let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);
```

**Wraps**: Static frame array in FrameAllocator interface.

### Phase 4: Heap Initialization

```rust
init_heap(&mut mapper, &mut frame_alloc);
```

**Result**: Kernel can now use `Vec`, `Box`, `String`, etc.

## Memory Layout

### Virtual Address Space

```
0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF: User space (128 TB)
0x0000_8000_0000_0000 - 0xFFFF_7FFF_FFFF_FFFF: Non-canonical (invalid)
0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF: Kernel space (128 TB)

Kernel Space Detail:
0xFFFF_8000_0000_0000 - Physical memory mapping base
0x4444_4444_0000       - Kernel heap (1 MB)
(Kernel code loaded by bootloader at various addresses)
```

### Physical Memory

```
0x0000_0000 - 0x0009_FFFF: Conventional memory (640 KB)
0x000A_0000 - 0x000F_FFFF: Video memory, BIOS ROM
0x0010_0000 - ???        : Extended memory (varies by system)

Memory Map Types:
- USABLE:                  Available for allocation
- RESERVED:                Hardware reserved (MMIO, ACPI)
- ACPI_RECLAIMABLE:        Can be reclaimed after parsing ACPI
- ACPI_NVS:                ACPI Non-Volatile Storage
- BAD_MEMORY:              Defective RAM
- BOOTLOADER_RECLAIMABLE:  Can be reclaimed after boot
- KERNEL_AND_MODULES:      Kernel image and modules
```

## Usage Examples

### Allocating Dynamic Memory

```rust
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;

// After heap is initialized...

// Vector
let mut v = Vec::new();
v.push(1);
v.push(2);
v.push(3);

// Boxed value
let boxed = Box::new(42);

// String
let s = String::from("Hello, Serix!");
```

### Manual Page Mapping

```rust
use x86_64::structures::paging::{Page, PhysFrame, PageTableFlags};
use x86_64::{VirtAddr, PhysAddr};

let page = Page::containing_address(VirtAddr::new(0x5000_0000));
let frame = PhysFrame::containing_address(PhysAddr::new(0x1000_0000));
let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

unsafe {
    mapper.map_to(page, frame, flags, &mut frame_allocator)
        .expect("Failed to map page")
        .flush();
}
```

### Unmapping Pages

```rust
let page = Page::containing_address(VirtAddr::new(0x5000_0000));

unsafe {
    let (frame, flush) = mapper.unmap(page).expect("Failed to unmap");
    flush.flush();
}
```

## Safety Considerations

### Memory Safety Invariants

1. **No Unmapped Access**: All accessed memory must be mapped
2. **No Double Mapping**: Physical frame should not be mapped to multiple virtual pages (except for read-only shared pages)
3. **No Use After Free**: Freed memory must not be accessed
4. **Correct Flags**: Pages must have appropriate access flags (writable, user, executable)

### Unsafe Operations

All memory management operations are inherently unsafe:

```rust
unsafe {
    // Initializing page table (relies on bootloader correctness)
    let mapper = memory::init_offset_page_table(phys_mem_offset);
    
    // Mapping pages (incorrect mapping can cause corruption)
    mapper.map_to(page, frame, flags, &mut frame_alloc)?;
    
    // Initializing heap (must be correctly mapped first)
    HEAP_ALLOCATOR.lock().init(heap_start, heap_size);
}
```

**Why Unsafe?**
- Relies on correctness of bootloader-provided information
- Incorrect mappings cause memory corruption, undefined behavior
- No way to verify correctness at compile-time

## Debugging

### Heap Allocation Failures

**Symptoms**: Allocation returns null pointer, panic in allocator.

**Causes**:
1. Heap not initialized
2. Heap exhausted (out of memory)
3. Heap corrupted (double free, buffer overflow)

**Debug Strategy**:
```rust
// Check if heap is initialized
serial_println!("Allocating...");
let v = Vec::new();  // Will panic if heap not initialized
serial_println!("Allocation successful");
```

### Page Fault Debugging

**Page Fault Causes**:
1. Accessing unmapped memory
2. Writing to read-only page
3. Executing non-executable page
4. User accessing kernel page

**Debug Information**:
```rust
// In page fault handler
serial_println!("Page Fault at {:#x}", cr2_value);
serial_println!("Instruction: {:#x}", rip);
serial_println!("Error code: {:?}", error_code);
```

### Frame Allocator Exhaustion

```rust
if let Some(frame) = frame_allocator.allocate_frame() {
    // Use frame
} else {
    serial_println!("WARNING: Out of physical memory!");
}
```

## Performance Considerations

### TLB Flush Overhead

**Problem**: TLB flush is expensive (100-1000 cycles per entry).

**Optimization**: Batch operations to minimize flushes.

```rust
// Bad: Flush after each map
for page in pages {
    mapper.map_to(page, frame, flags, alloc)?;  // Flushes TLB
}

// Good: Flush once at end
for page in pages {
    mapper.map_to(page, frame, flags, alloc)?;
    // Don't flush yet
}
mapper.flush_all();  // Single batch flush
```

### Heap Allocation Performance

**Linked List Allocator**:
- Allocation: O(n) in worst case (first-fit)
- Deallocation: O(1) if adjacent blocks merged

**Alternative Allocators** (future):
- **Buddy Allocator**: O(log n), less fragmentation
- **Slab Allocator**: O(1), optimized for fixed-size objects
- **SLUB**: Linux-style slab allocator

### Large Pages

**4KB Pages**: Standard, maximum flexibility
**2MB Pages** (huge pages): Reduce TLB pressure, faster
**1GB Pages**: Minimize TLB misses for large allocations

**Trade-off**: Large pages reduce flexibility, increase internal fragmentation.

## Future Enhancements

### Demand Paging

**Goal**: Allocate pages only when accessed (lazy allocation).

```rust
// Map page as not present
mapper.map_to(page, PhysFrame::null(), PageTableFlags::empty(), alloc)?;

// In page fault handler
if !page_is_present() {
    let frame = allocate_frame();
    mapper.update_flags(page, PageTableFlags::PRESENT | PageTableFlags::WRITABLE)?;
}
```

### Copy-on-Write

**Goal**: Share memory between processes until one writes.

```rust
// Fork: Share pages as read-only
for page in process_pages {
    mapper.update_flags(page, PageTableFlags::PRESENT)?;  // Remove WRITABLE
}

// In page fault handler (write to COW page)
if is_copy_on_write(page) {
    let new_frame = allocate_frame();
    copy_frame(old_frame, new_frame);
    mapper.map_to(page, new_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE, alloc)?;
}
```

### Memory Defragmentation

**Problem**: Heap becomes fragmented over time.

**Solution**: Compact heap by moving allocations.

```rust
pub fn compact_heap() {
    // Identify fragmented regions
    // Move allocations to consolidate free space
    // Update pointers
}
```

**Challenge**: Updating all pointers (requires conservative GC or cooperation from allocator).

### NUMA Awareness

**Goal**: Allocate memory on same NUMA node as CPU for better performance.

```rust
pub fn allocate_frame_on_node(node: u8) -> Option<PhysFrame>;
```

## Dependencies

### Internal Crates

None (memory is a foundational module)

### External Crates

- **linked_list_allocator** (0.10.5): Heap allocator implementation
- **x86_64** (0.15.2): Page table abstractions, CR3 register
- **limine** (0.5.0): Memory map structures

## Configuration

### Cargo.toml

```toml
[package]
name = "memory"
version = "0.1.0"
edition = "2024"

[dependencies]
linked_list_allocator = "0.10.5"
x86_64 = "0.15.2"
limine = "0.5.0"
```

### Compile-Time Configuration

Heap size can be adjusted:
```rust
const HEAP_SIZE: usize = 1024 * 1024;  // Change to 2MB, 4MB, etc.
```

Maximum boot frames:
```rust
pub const MAX_BOOT_FRAMES: usize = 65536;  // ~256 MB
```

## References

- [Intel 64 and IA-32 Architectures Software Developer's Manual, Volume 3A: System Programming Guide, Chapter 4 (Paging)](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev - Paging](https://wiki.osdev.org/Paging)
- [OSDev - Page Frame Allocation](https://wiki.osdev.org/Page_Frame_Allocation)
- [Writing an OS in Rust: Heap Allocation](https://os.phil-opp.com/heap-allocation/)

## License

GPL-3.0 (see LICENSE file in repository root)
