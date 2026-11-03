# Phase 7 implementation testing guide

This guide covers the Phase 7 code currently implemented in this branch: ACPI parsers and boot discovery, xAPIC/x2APIC register setup, AP callbacks through Limine MP, bounded IPI mailboxes, and PCI inventory for NVMe/xHCI devices. It also gives the boot smoke test. The remaining Phase 7 hardware features are not present yet.

## 1. Run the ACPI parser tests

Run the parser tests on the host target so Rust's test harness is available:

```sh
cargo test -p acpi --target x86_64-unknown-linux-gnu
```

The tests cover legacy and extended RSDP checksums, SDT signature/checksum validation, malformed MADT entries, duplicate enabled APIC IDs, sparse/high APIC IDs, ISA IRQ override lookup, short DMAR structures, FADT reset GAS parsing, and null XSDT pointers. Expected result: all tests pass.

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

On serial output, first look for `x2APIC enabled` or `x2APIC unsupported; using xAPIC`, then ACPI messages after `Phase 1: init_memory...`, including one of:

- `[ACPI] MADT topology: ...` followed by logical CPU/APIC ID entries
- `[ACPI] no valid MADT; retain degraded legacy APIC setup`
- `[ACPI] Limine did not provide an RSDP; firmware features disabled`

The normal expected path includes `x2APIC enabled`, an RSDP virtual and physical address, a validated FADT, a validated DMAR table, and a MADT CPU list. A four-CPU QEMU boot should report four MADT CPU entries and three AP callback initialization messages. The no-RSDP or invalid-MADT messages are deliberate degraded-mode diagnostics; malformed ACPI must not prevent the existing boot path from continuing.

The `enabled=true` MADT flag means the processor is available now. `hotplug_capable=false` only means firmware does not advertise that disabled processor as a future hotplug target; it is normal for an enabled CPU. `[CPU]` lines probe CPUID leaf `0x1A` on the BSP and in each AP callback. `Unknown` with `leaf_1a=false` means the CPU model lacks the leaf; `leaf_1a=true raw=0x00` means the hypervisor exposed the leaf but did not provide a recognized core type.

The default `make run` uses TCG and `-cpu max`, so `Unknown` core types are expected. On a Linux host with KVM, `make run-host` uses `-cpu host`, the host socket count, and one guest core for each available host logical CPU (capped at 16). This avoids fabricating a uniform SMT layout for a hybrid host. QEMU's `-smp` topology describes guest vCPUs; it does not pin each guest vCPU to a corresponding host P-core or E-core. Host passthrough can still leave leaf `0x1A` unavailable or uniform across vCPUs.

If the QEMU machine configuration includes NVMe or xHCI hardware, PCI discovery logs lines beginning with `PCI Nvme` or `PCI Xhci`, including decoded BAR0 and MSI-X table/PBA locations. These are read-only inventory values; the kernel does not map the BAR for these devices or enable their interrupts.

The default `make run` adds QEMU's emulated Intel IOMMU, so ACPI should report a validated DMAR table. `QEMU_IOMMU=` disables the emulated device for a no-IOMMU comparison. Serix currently parses and reports DMAR only; detecting the table does not mean device DMA is isolated. `reset register false` means the FADT did not provide an extended reset GAS; it is a firmware capability report, not an ACPI parsing error.

Serial output records are serialized across CPUs, so AP messages should no longer be garbled together. The HAL retains the newest 32 KiB of serial output in a memory ring buffer. In the kernel shell, enter `logs` to display the newest 4 KiB, or `clear` to clear the framebuffer console and reset its cursor. Serial output is not mirrored to the framebuffer; use `logs` when you need to review boot messages from kshell.

For a one-CPU baseline, use the same build artifacts and run:

```sh
qemu-system-x86_64 -cpu max -m 4G -smp 1 -boot d -cdrom serix.iso \
  -drive file=disk.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off \
  -device intel-iommu \
  -serial stdio
```

For a four-CPU topology check, use `-smp 4` in place of `-smp 1`. Compare the ACPI logical/APIC entries with QEMU's CPU topology, confirm one `[CPU]` line for every vCPU, and verify the system reaches `Serix OS ... ready.` In kshell, run `logs` to review recent serial output and `clear` to clear the framebuffer. APs still start through Limine's MP callback; normal boot does not yet use the kernel INIT/SIPI helper.

To exercise the xAPIC fallback, use the same command with `-cpu max,-x2apic -smp 1`. Expected output starts with `xAPIC enabled` and continues through the ACPI diagnostics.

The APIC mode line verifies which register path was selected. AP callback startup is exercised on `-smp 4`. A dedicated cross-CPU IPI/TLB integration test is not wired to a user command yet; mailbox and shootdown APIs have compile coverage only. Do not infer a successful shootdown from seeing the APs start.

## 4. Current implementation boundary

ACPI table discovery currently validates RSDP, root SDT, child SDT checksums and bounds, and parses MADT topology, DMAR remapping-unit descriptors, and selected FADT fields. DMAR/FADT are diagnostic only. MADT data is not yet the source of I/O APIC routing or AP startup. The kernel still uses its existing APIC setup and Limine MP callback path.

Do not use this branch to test IOMMU isolation, Ring 3 NVMe/XHCI drivers, ACPI power transitions, kernel AP startup via INIT/SIPI, or end-to-end TLB shootdown; those Phase 7 deliverables are not implemented yet. x2APIC selection and AP callback startup are QEMU-tested; mailbox acknowledgement behavior is not yet exercised end to end. The roadmap remains unchecked for the end-to-end deliverables.
