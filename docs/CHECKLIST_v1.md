
## v1.0 Release Checklist

### Foundation (must have, already partially done)
- [ ] Stable preemptive scheduler, no deadlocks under load
- [ ] Per-task kernel stacks with guard pages (done)
- [ ] Context switch correctness under stress
- [ ] TSS.RSP0 updated on every context switch
- [ ] Capability enforcement on all IPC send() calls
- [ ] Mount table for overlaying filesystems

### Memory
- [ ] `mmap(MAP_ANONYMOUS)` — anonymous page allocation
- [ ] `mmap(MAP_PRIVATE)` — file-backed private mapping
- [ ] `munmap` — unmapping regions
- [ ] `brk` / `sbrk` — heap growth for userspace
- [ ] Page fault handler that allocates on demand (demand paging)
- [ ] Copy-on-write fork semantics
- [ ] Stack growth on page fault (guard page expansion)

### Process Model
- [ ] `fork()` — duplicate process
- [ ] `execve()` — replace process image with ELF
- [ ] `waitpid()` — parent waits for child exit
- [ ] `exit_group()` — clean process termination
- [ ] `getpid()` / `getppid()`
- [ ] `clone()` → thread creation (CLONE_VM | CLONE_FS | CLONE_FILES)
- [ ] TLS setup via `arch_prctl(ARCH_SET_FS)`
- [ ] Process table (global PID → TaskCB mapping)
- [ ] Zombie reaping

### Syscall Coverage
- [ ] `read` / `write` / `open` / `close` (done)
- [ ] `lseek`
- [ ] `stat` / `fstat`
- [ ] `access`
- [ ] `getcwd` / `chdir`
- [ ] `mkdir` / `rmdir` (done)
- [ ] `unlink` (done)
- [ ] `rename`
- [ ] `dup` / `dup2`
- [ ] `pipe`
- [ ] `fcntl` (at minimum F_GETFL/F_SETFL)
- [ ] `ioctl` (TIOCGWINSZ, TCGETS minimum)
- [ ] `poll` or `select`
- [ ] `mmap` / `munmap` / `mprotect`
- [ ] `brk`
- [ ] `rt_sigaction` / `rt_sigprocmask` / `rt_sigreturn`
- [ ] `kill` / `tgkill`
- [ ] `nanosleep` or `clock_nanosleep`
- [ ] `gettimeofday` / `clock_gettime`
- [ ] `uname`
- [ ] `getdents64`
- [ ] `set_tid_address`

### Filesystem
- [ ] FAT32 stable (done, mostly)
- [ ] `/proc/self/maps` — minimum for dynamic linker
- [ ] `/proc/self/exe`
- [ ] `/dev/null` / `/dev/zero`
- [ ] `/dev/tty` backed by framebuffer console
- [ ] Proper path canonicalization
- [ ] Symlink support (at least readlink)
- [ ] File permissions (even fake/stub ones)

### Dynamic Linking
- [ ] PT_INTERP parsing in ELF loader (done partially)
- [ ] ld-linux-x86-64.so.2 loadable from VFS
- [ ] Auxiliary vector construction (AT_PHDR, AT_ENTRY, AT_BASE, AT_RANDOM, AT_PAGESZ)
- [ ] argv/envp/auxv stack layout correct

### Terminal / Shell
- [ ] PTY (pseudo-terminal) or at minimum /dev/tty
- [ ] VDSO page for clock_gettime fast path
- [ ] Port rsh (your existing shell) to no_std + ulib
- [ ] `ls` builtin working
- [ ] `cat` builtin working
- [ ] External command execution via fork+exec

### Stability
- [ ] No kernel panic on bad syscall arguments
- [ ] All userspace pointer accesses validated before use
- [ ] Double fault handler with separate IST stack (currently missing)
- [ ] Proper EOI on all interrupt paths
- [ ] Serial log of all panics with register dump

### Build / Developer Experience
- [ ] CI: cargo build passes on tag
- [ ] CI: QEMU boots to shell in automated test
- [ ] cargo clippy with zero warnings
- [ ] Version string in uname output

