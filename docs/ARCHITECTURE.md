===================================
Serix Kernel Architecture
===================================

:Author: Serix Kernel Team
:Version: v0.0.5
:Architecture: x86_64
:Design: Microkernel-inspired monolithic kernel

.. contents::
    :depth: 3

Overview
========

Serix is a capability-based microkernel-inspired operating system written in
Rust for the x86_64 architecture. This document describes the architectural
design, subsystem organization, and implementation philosophy of the kernel.

Current Status (v0.0.5)
-----------------------

Serix v0.0.5 is a proof-of-concept kernel with these working features:

- x86_64 long mode, higher-half kernel
- APIC interrupt controller (Local APIC + I/O APIC)
- Timer interrupts (~625 Hz) and keyboard input
- Serial console and framebuffer graphics
- Memory management (paging, heap, frame allocator)
- VFS with ramdisk and ELF loader
- Basic syscalls and userspace init execution

See individual subsystem documentation for detailed information.
