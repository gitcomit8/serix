
## v1.0 Release Checklist

### Foundation (must have, already partially done)
- [x] Stable preemptive scheduler with WFQ, no deadlocks under load
- [x] Per-task kernel stacks with guard pages (done)
- [ ] Context switch correctness under stress testing
- [ ] TSS.RSP0 updated on every context switch (deferred — uses SWITCH_HOOK callback)
- [x] Capability enforcement on all IPC send() calls (done; extended to all resource-accessing syscalls)
- [x] Mount table for overlaying filesystems (done; longest-prefix BTreeMap matching)

### Memory
- [ ] `mmap(MAP_ANONYMOUS)` — anonymous page allocation
- [ ] `mmap(MAP_PRIVATE)` — file-backed private mapping
- [x] `munmap` — unmapping regions (SYS_MUNMAP(91))
- [ ] `brk` / `sbrk` — heap growth for userspace
- [ ] Page fault handler that allocates on demand (demand paging)
- [ ] Copy-on-write fork semantics
- [ ] Stack growth on page fault (guard page expansion)

### Process Model
- [ ] `fork()` — duplicate process
- [ ] `execve()` — replace process image with ELF
- [x] `waitpid()` — parent waits for child exit (SYS_WAIT(5); waits for any child, -1 = any)
- [x] `exit()` — process termination (SYS_EXIT(0))
- [x] `getpid()` / `getppid()` (SYS_GETPID(2), SYS_GETPPID(3))
- [ ] `clone()` → thread creation (CLONE_VM | CLONE_FS | CLONE_FILES)
- [ ] TLS setup via `arch_prctl(ARCH_SET_FS)`
- [ ] Process table (global PID → TaskCB mapping; per-CPU run queues exist)
- [x] Zombie reaping (SYS_WAIT reaps zombies; zombie list per run queue)

### Syscall Coverage
- [x] `read` / `write` / `open` / `close` (SYS_READ(12), SYS_WRITE(13), SYS_OPEN(10), SYS_CLOSE(11))
- [x] `lseek` (SYS_SEEK(14))
- [ ] `stat` / `fstat`
- [ ] `access`
- [ ] `getcwd` / `chdir`
- [x] `mkdir` / `rmdir` (SYS_MKDIR(20), SYS_RMDIR(22))
- [x] `unlink` (SYS_UNLINK(21))
- [ ] `rename`
- [x] `dup` / `dup2` (SYS_DUP(15), SYS_DUP2(16))
- [x] `pipe` (SYS_PIPE(17))
- [ ] `fcntl` (at minimum F_GETFL/F_SETFL)
- [ ] `ioctl` (TIOCGWINSZ, TCGETS minimum)
- [ ] `poll` or `select`
- [x] `mmap` / `munmap` (SYS_MMAP(90), SYS_MUNMAP(91))
- [ ] `brk`
- [ ] `rt_sigaction` / `rt_sigprocmask` / `rt_sigreturn`
- [ ] `kill` / `tgkill`
- [ ] `nanosleep` or `clock_nanosleep`
- [ ] `gettimeofday` / `clock_gettime`
- [ ] `uname`
- [x] `getdents64` (SYS_GETDENTS(18))
- [ ] `set_tid_address`

### Filesystem
- [x] FAT32 stable (done)
- [ ] `/proc/self/maps` — minimum for dynamic linker
- [ ] `/proc/self/exe`
- [ ] `/dev/null` / `/dev/zero`
- [ ] `/dev/tty` backed by framebuffer console
- [x] Proper path canonicalization (longest-prefix mount matching)
- [ ] Symlink support (at least readlink)
- [ ] File permissions (even fake/stub ones)

### Dynamic Linking
- [ ] PT_INTERP parsing in ELF loader (not yet implemented)
- [ ] ld-linux-x86-64.so.2 loadable from VFS
- [ ] Auxiliary vector construction (AT_PHDR, AT_ENTRY, AT_BASE, AT_RANDOM, AT_PAGESZ)
- [ ] argv/envp/auxv stack layout correct

### Terminal / Shell
- [x] Kernel shell (`kshell`) with builtins: `help`, `echo`, `ls`, `cat`, `write`, `mkdir`, `rm`, `mount`, `umount`, `halt`, `reboot`
- [ ] PTY (pseudo-terminal) or at minimum /dev/tty
- [ ] VDSO page for clock_gettime fast path
- [ ] Port rsh (existing shell) to no_std + ulib for Ring-3 userspace
- [x] `ls` builtin working (kshell)
- [x] `cat` builtin working (kshell)
- [ ] External command execution via fork+exec

### Stability
- [ ] No kernel panic on bad syscall arguments (partial: userspace pointers validated, argument ranges checked)
- [x] All userspace pointer accesses validated before use (`is_user_accessible()` checks)
- [ ] Double fault handler with separate IST stack (not yet implemented)
- [x] Proper EOI on all interrupt paths (`apic::send_eoi()` in handlers)
- [x] Serial log of all panics with register dump (location, file, line)

### Build / Developer Experience
- [ ] CI: cargo build passes on tag
- [ ] CI: QEMU boots to shell in automated test
- [ ] cargo clippy with zero warnings
- [ ] Version string in uname output

