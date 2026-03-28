# Drivers Module

## Overview

The drivers module provides hardware device drivers for the Serix kernel. It currently includes PCI bus enumeration, a VirtIO 1.0 block device driver with interrupt-driven I/O, a virtqueue implementation with DMA-safe memory allocation, a block device VFS wrapper, and a console device. The driver subsystem bridges hardware access and the VFS layer, allowing disk I/O through the standard `INode` interface.

## Architecture

### Module Structure

| Module       | Description                                            |
|-------------|--------------------------------------------------------|
| `pci`       | PCI bus enumeration and configuration space access     |
| `virtio`    | VirtIO block device driver (PCI transport)             |
| `virtqueue` | Generic virtqueue descriptor ring implementation       |
| `block`     | VFS `INode` wrapper over VirtIO block device           |
| `console`   | Console device for VFS                                 |

## PCI Module

### PciDevice Struct

Represents a device on the PCI bus:

```rust
pub struct PciDevice {
	pub bus: u8,
	pub device: u8,
	pub function: u8,
	pub vendor_id: u16,
	pub device_id: u16,
}
```

Provides methods for configuration space access:

- `read_u8`, `read_u16`, `read_u32` -- Read from PCI configuration registers
- `get_bar` -- Read a Base Address Register and its size
- `enable_bus_master` -- Set the bus master bit in the PCI command register
- `interrupt_line`, `interrupt_pin` -- Read interrupt routing information
- `find_capability` -- Walk the PCI capability list to find a specific capability ID

### enumerate_pci

```rust
pub fn enumerate_pci() -> Vec<PciDevice>;
```

Scans all 256 PCI buses, 32 slots per bus, checking for present devices via the vendor ID register (`0xFFFF` means no device). Multi-function devices are detected via the header type register and all functions are enumerated. Returns a `Vec<PciDevice>` of all discovered devices.

## VirtIO Module

### VirtioBlock

The VirtIO block device driver implements the VirtIO 1.0 specification over PCI transport. It supports sector-level read and write operations with interrupt-driven completion.

### Two-Phase Initialization

Initialization is split into two phases to handle the dependency on the SLUB (heap) allocator:

**Phase 1 -- `VirtioBlock::init()`** (before SLUB):

1. Verify VirtIO vendor ID (`0x1AF4`) and modern device ID (`>= 0x1040`)
2. Walk PCI capabilities to locate MMIO regions (common config, notify, ISR, device config)
3. Map MMIO BARs via a caller-provided `map_mmio` callback
4. Negotiate device features
5. Set device status to `FEATURES_OK`
6. Store the partially-initialized device globally via `store_global()`

**Phase 2 -- `setup_queues()` / `setup_queues_global()`** (after SLUB):

1. Allocate virtqueue descriptor table, available ring, and used ring (requires heap/frame allocator)
2. Program queue addresses into the device via common config MMIO
3. Enable the queue
4. Set device status to `DRIVER_OK`, making the device fully operational

This split exists because virtqueue memory must be allocated from the physical frame allocator, which is not available until after the heap is initialized. Phase 1 stores the device in a global `Once<Mutex<VirtioBlock>>`, and Phase 2 is called later via `setup_queues_global()`.

### Sector I/O

```rust
pub fn read_sector(&mut self, sector: u64, buf: &mut [u8; 512]) -> Result<(), &'static str>;
pub fn write_sector(&mut self, sector: u64, buf: &[u8; 512]) -> Result<(), &'static str>;
pub fn capacity(&self) -> u64;
```

Both `read_sector` and `write_sector` submit a three-descriptor chain to the virtqueue:

1. **Header descriptor** (device-readable): VirtIO block request header with operation type and sector number
2. **Data descriptor**: 512-byte sector buffer (device-writable for reads, device-readable for writes)
3. **Status descriptor** (device-writable): Single-byte completion status

After submitting the chain and notifying the device, the driver spins on an `AtomicBool` flag (`VIRTIO_BLK_COMPLETE`) that is set by the interrupt handler when the device signals completion.

### Global Accessors

```rust
pub fn virtio_blk() -> Option<&'static Mutex<VirtioBlock>>;
pub fn store_global(blk: VirtioBlock);
pub fn setup_queues_global();
pub fn register_interrupt();
```

- `virtio_blk()` returns a reference to the global device instance
- `store_global()` stores a Phase 1 initialized device in the global `Once`
- `setup_queues_global()` runs Phase 2 on the stored device
- `register_interrupt()` reads the PCI interrupt line, maps the IRQ via IOAPIC, and registers the IDT handler

### Interrupt Handling

The VirtIO block device uses **interrupt vector 34** (`VIRTIO_BLK_VECTOR`), mapped from **IRQ 11** via the IOAPIC. The interrupt handler (`virtio_blk_interrupt_handler`) reads the ISR status register to acknowledge the interrupt, sets the `VIRTIO_BLK_COMPLETE` atomic flag, and signals EOI to the APIC.

```
PCI IRQ 11 --> IOAPIC --> Vector 34 --> virtio_blk_interrupt_handler
                                          |-> Read ISR status
                                          |-> Set VIRTIO_BLK_COMPLETE
                                          |-> EOI
```

## Virtqueue Module

### Virtqueue Struct

Implements the VirtIO split virtqueue with three DMA-accessible ring regions:

- **Descriptor table** (`VirtqDesc`): Array of buffer descriptors, each with a physical address, length, flags, and next-descriptor index
- **Available ring** (`VirtqAvail`): Driver-to-device ring of descriptor chain head indices
- **Used ring** (`VirtqUsed`): Device-to-driver ring of completed descriptor indices with byte counts

```rust
pub struct Virtqueue {
	// Pointers to DMA-mapped descriptor table, available ring, and used ring
	// Free list management, queue size, last-seen used index
}
```

**Key methods**:

- `allocate(queue_size, hhdm_offset)` -- Allocate and initialize all three ring regions
- `push_chain(descs)` -- Submit a descriptor chain (returns head index)
- `pop_used()` -- Retrieve completed entries from the used ring
- `free_chain(head)` -- Return descriptors to the free list

### DMA Address Model

Virtqueue memory must be allocated as physical page frames accessed through the HHDM (Higher Half Direct Map), not from the SLUB heap allocator. This is critical because:

1. **VirtIO devices perform DMA** using physical addresses. The device reads descriptor table entries containing physical buffer addresses.
2. **HHDM frames have trivial virtual-to-physical translation**: `phys = virt - hhdm_offset`. This makes it straightforward to provide the device with correct physical addresses.
3. **SLUB allocations** do not guarantee contiguous physical memory or simple address translation, making them unsuitable for DMA buffers.

The `alloc_dma_page()` function encapsulates this pattern:

```rust
fn alloc_dma_page(hhdm_offset: u64) -> Option<*mut u8>;
```

It allocates a physical frame from the page allocator, computes the HHDM virtual address (`hhdm_offset + phys`), zeros the page, and returns the virtual pointer. The corresponding physical address needed by the device is simply `virt - hhdm_offset`.

## Block Module

### BlockDevice

Implements the `vfs::INode` trait to provide byte-oriented access to the VirtIO block device. Translates arbitrary byte offsets and lengths into 512-byte sector operations.

```rust
pub struct BlockDevice;

impl INode for BlockDevice {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;
	fn write(&self, offset: usize, buf: &[u8]) -> usize;
	fn metadata(&self) -> FileType;  // Returns FileType::Device
	fn size(&self) -> usize;         // Returns capacity in bytes
}
```

**Read behavior**: Computes the starting sector and intra-sector offset, reads full sectors into a temporary 512-byte buffer, and copies only the requested byte range into the caller's buffer. Handles multi-sector reads in a loop.

**Write behavior**: For partial-sector writes, performs a read-modify-write cycle: reads the existing sector, overwrites the affected bytes, and writes the full sector back. Full-sector writes skip the initial read.

**Bounds checking**: Both operations clamp to the device capacity and return 0 if the offset is beyond the end of the device.

## Dependencies

### Internal Crates

- **hal**: I/O port access (`inl`, `outl`) for PCI configuration space, serial debug output
- **vfs**: `INode` trait and `FileType` enum for the block device wrapper
- **graphics**: Console device support
- **memory**: Page allocator for DMA frame allocation
- **idt**: Interrupt handler registration
- **apic**: IOAPIC IRQ mapping and EOI signaling

### External Crates

- **x86_64** (0.15.2): Interrupt stack frame types, paging structures, frame allocator trait
- **spin** (0.10.0): `Mutex` and `Once` for global device state

## License

GPL-3.0 (see LICENSE file in repository root)
