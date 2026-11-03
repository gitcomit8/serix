# Changelog

## [v0.0.8] - 2026-11-04

This release summarizes changes since v0.0.6; it includes the contents of the previously published v0.0.7 release.

### Added

- Kernel-space TTY shell and expanded process, file, and filesystem syscall support.
- SMP startup and per-CPU scheduler queues, weighted fair scheduling, priority inheritance, and TSS kernel-stack switching.
- IPC notification ports and register-mediated fast-path message delivery.
- Ext4 HTree directory lookup, formatting, JBD2 journal support, and stricter directory handling.
- Unified page cache, deferred writeback, demand paging, copy-on-write, and file-backed `mmap` support.
- Per-task capability spaces, generation-checked references, syscall authorization, revocation, delegation, and capability audit logging.
- Ring 3 policy daemon with TOML policy loading and hot reload, integrated through the LES bridge.

### Changed

- Reworked VFS mounts and path canonicalization, and split kernel startup into subsystem initialization modules.
- Migrated userspace syscall wrappers and filesystem services to capability-referenced APIs.
- Updated build configuration and project documentation for the expanded kernel and userspace architecture.

### Fixed

- Corrected SMP per-CPU state setup, APIC handling, and GS-base invariants during syscall/context-switch paths.
- Fixed an audit-ring-related boot crash and preserved capability slot ordering.
- Fixed ext4 HTree hash selection and zero-link-count handling during directory removal.
- Preserved IPC direct-delivery ordering and corrected several filesystem, build, and boot issues.

