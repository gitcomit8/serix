# Phase 7 implementation testing guide

This guide covers the Phase 7 code that is currently implemented in this branch: the ACPI byte parsers and early-boot RSDP/table discovery. It also gives the boot smoke test. It does not imply that the remaining Phase 7 hardware features are already present.

## 1. Run the ACPI parser tests

Run the parser tests on the host target so Rust's test harness is available:

```sh
cargo test -p acpi --target x86_64-unknown-linux-gnu
```

The tests cover SDT signature/checksum validation, malformed MADT entries, duplicate enabled APIC IDs, sparse/high APIC IDs, ISA IRQ override lookup, and short DMAR structures. Expected result: all tests pass.

Check that the no-std parser also compiles for the kernel target:

```sh
cargo check -p acpi --target x86_64-unknown-none
cargo check -p drivers --target x86_64-unknown-none
```

## 2. Build the kernel

From the repository root:

```sh
cargo check --manifest-path kernel/Cargo.toml \
  -Zbuild-std=core,alloc,compiler_builtins \
  -Zbuild-std-features=compiler-builtins-mem \
  --release --target x86_64-unknown-none
```

The repository's pinned toolchain is nightly. Existing warnings in other crates are expected; the command must finish successfully.

## 3. Boot smoke test under QEMU

The normal `make run` target builds the ISO and starts QEMU with four CPUs and the VirtIO block device:

```sh
make run
```

On serial output, look for ACPI messages after `Phase 1: init_memory...`, including one of:

- `[ACPI] MADT topology: ...` followed by logical CPU/APIC ID entries
- `[ACPI] no valid MADT; retain degraded legacy APIC setup`
- `[ACPI] Limine did not provide an RSDP; firmware features disabled`

The first is the normal expected path on QEMU. The remaining messages are deliberate degraded-mode diagnostics. A malformed ACPI table must not prevent the kernel from continuing into its existing boot path.

If the QEMU machine configuration includes NVMe or xHCI hardware, PCI discovery logs lines beginning with `PCI Nvme` or `PCI Xhci`, including decoded BAR0 and MSI-X table/PBA locations. These are read-only inventory values; the kernel does not map the BAR for these devices or enable their interrupts.

For a one-CPU baseline, use the same build artifacts and run:

```sh
qemu-system-x86_64 -m 4G -smp 1 -boot d -cdrom serix.iso \
  -drive file=disk.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off \
  -serial stdio
```

For a four-CPU topology check, use `-smp 4` in place of `-smp 1`. Compare the ACPI logical/APIC entries with QEMU's CPU topology and verify the system reaches `Serix OS ... ready.` in both runs.

## 4. Current implementation boundary

ACPI table discovery currently validates RSDP, root SDT, child SDT checksums and bounds, and parses MADT topology, DMAR remapping-unit descriptors, and selected FADT fields. DMAR/FADT are diagnostic only. MADT data is not yet the source of I/O APIC routing or AP startup. The kernel still uses its existing APIC setup and Limine MP callback path.

Do not use this branch to test IOMMU isolation, Ring 3 NVMe/XHCI drivers, ACPI power transitions, x2APIC, AP startup via INIT/SIPI, or TLB shootdown; those Phase 7 deliverables are not implemented yet. The roadmap remains unchecked for those items.
