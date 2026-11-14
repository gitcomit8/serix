# Payload Disk

`make payload` builds `payload.img` (96 MiB, GPT), the second disk that carries
the userland described in [docs/USERLAND.md](../docs/USERLAND.md). It is
generated, never committed: `*.img` is gitignored.

## Layout

| Partition | Sectors | Size | Filesystem | Contents |
|---|---|---|---|---|
| 1 | start 2048, 32768 | 16 MiB | ext2, 1 KiB blocks | `/SERIX.MRK`, `/sbin/init`, `/sbin/ext4d` |
| 2 | start 34816, 159744 | 78 MiB | ext4, 1 KiB blocks, no journal | `/bin`, `/usr/bin`, `/tmp`, `/etc/serix/policy.toml` |

Partition 1 is the bootstrap volume: it carries only what the kernel needs
before an ext4 daemon exists. Partition 2 is the userland that `init` mounts
read-only.

## Building

```bash
make payload        # builds ext4d and init first, then the image
```

Both partitions are populated with `mke2fs -d`, so the target needs no loop
device, no mount, and no root. It rebuilds on every invocation, so the image
always matches the current `ext4d` and `init` binaries.

## Bootstrap filesystem is ext2, not FAT32

`docs/USERLAND.md` §3 originally specified FAT32 for the bootstrap volume. It is
built as ext2 instead, for two reasons:

- The kernel already has an ext2 reader (`fs/src/ext2/`), so the volume is
  readable before `ext4d` starts, which is the only property the design needs.
- Building and inspecting a FAT32 image needs `mkfs.fat` (dosfstools) and
  `mtools`, neither of which the normal build requires. The ext2 path needs only
  `mke2fs` and `sfdisk`, which `make disk` and `make gptdisk` already use.

Switching back to FAT32 is confined to the `payload` target in the `Makefile`
plus the marker scan in `kernel/src/init/`, which probes partitions by
filesystem detection rather than by type GUID.

## Verifying without booting

```bash
sfdisk -l payload.img
dd if=payload.img of=/tmp/p1.img bs=512 skip=2048 count=32768 status=none
debugfs -R 'ls -l /sbin' /tmp/p1.img
debugfs -R 'cat /SERIX.MRK' /tmp/p1.img
```

## Source files

Checked in under `payload/`:

- `payload/bootstrap/SERIX.MRK` — the marker the kernel scans for; contents
  `serix-payload 1`.
- `payload/userland/etc/serix/policy.toml` — the policy `init` applies with
  `SYS_POLICY_RELOAD`.

Everything else in the image is copied from build output:
`target/x86_64-unknown-none/release/ext4d` and
`target/x86_64-unknown-none/release/examples/init`.
