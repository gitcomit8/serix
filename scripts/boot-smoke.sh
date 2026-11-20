#!/usr/bin/env bash
#
# boot-smoke.sh - Boot the Serix ISO headless and fail unless the kernel
# reaches its post-init checkpoints without reporting a fault.
#
# Usage: scripts/boot-smoke.sh [iso] [log] [timeout-seconds] [disk] [gpt-disk]
#
# The kernel never exits, so the run is not driven by QEMU's exit status: the
# script polls the serial log for expected checkpoints, then stops QEMU and
# scans the log for panic output. Exit status is 0 only when every checkpoint
# appeared and the log is fault-free.

set -euo pipefail

ISO=${1:-serix.iso}
LOG=${2:-boot-smoke.log}
TIMEOUT=${3:-120}
DISK=${4:-disk.img}
GPT_DISK=${5:-gpt.img}

for f in "$ISO" "$DISK" "$GPT_DISK"; do
	if [[ ! -f "$f" ]]; then
		echo "boot-smoke: missing input file: $f" >&2
		exit 2
	fi
done

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
	echo "boot-smoke: qemu-system-x86_64 not found in PATH" >&2
	exit 2
fi

QEMU_ERR="$LOG.qemu"
: > "$LOG"
: > "$QEMU_ERR"

# Boot private copies of the disks: the smoke run must not modify the images,
# and QEMU's image lock would otherwise collide with a developer's own boot.
WORK_DIR=$(mktemp -d)
cp "$DISK" "$WORK_DIR/disk.img"
cp "$GPT_DISK" "$WORK_DIR/gpt.img"

# Two disks so the boot exercises the multi-device and GPT partition paths.
qemu-system-x86_64 \
	-machine q35,accel=tcg \
	-cpu max \
	-m 4G \
	-smp 4 \
	-boot d \
	-cdrom "$ISO" \
	-drive file="$WORK_DIR/disk.img",if=none,format=raw,id=x0 \
	-device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off \
	-drive file="$WORK_DIR/gpt.img",if=none,format=raw,id=x1 \
	-device virtio-blk-pci,drive=x1,disable-legacy=on,disable-modern=off \
	-display none \
	-monitor none \
	-no-reboot \
	-serial "file:$LOG" \
	>"$QEMU_ERR" 2>&1 &
QEMU_PID=$!

cleanup() {
	kill "$QEMU_PID" 2>/dev/null || true
	wait "$QEMU_PID" 2>/dev/null || true
	rm -rf "$WORK_DIR"
}
trap cleanup EXIT

# Boot stages that must be reached. These are the last kernel checkpoints
# before the idle loop: VFS up, both boot daemons spawned from disk images,
# and the Ring 0 shell running.
expect=(
	"Phase 1 complete"
	"VFS: mount table initialized"
	"ext4d: daemon spawned"
	"Kernel shell spawned"
)

deadline=$((SECONDS + TIMEOUT))
reached=0
while ((SECONDS < deadline)); do
	if ! kill -0 "$QEMU_PID" 2>/dev/null; then
		echo "boot-smoke: QEMU exited before all checkpoints were reached" >&2
		tail -n 40 "$QEMU_ERR" >&2 || true
		tail -n 40 "$LOG" >&2 || true
		exit 1
	fi

	reached=1
	for checkpoint in "${expect[@]}"; do
		if ! grep -qF -- "$checkpoint" "$LOG"; then
			reached=0
			break
		fi
	done
	if ((reached)); then
		break
	fi
	sleep 1
done

if ((!reached)); then
	echo "boot-smoke: timed out after ${TIMEOUT}s; checkpoints never reached:" >&2
	for checkpoint in "${expect[@]}"; do
		if ! grep -qF -- "$checkpoint" "$LOG"; then
			echo "  - $checkpoint" >&2
		fi
	done
	tail -n 40 "$LOG" >&2 || true
	exit 1
fi

# A checkpoint can be printed before a later fault, so scan the whole log.
fault_pattern='panic|triple fault|cpu exception|page fault|general protection'
if grep -qiE "$fault_pattern" "$LOG"; then
	echo "boot-smoke: kernel reported a fault" >&2
	grep -inE "$fault_pattern" "$LOG" >&2 || true
	exit 1
fi

echo "boot-smoke: OK - reached all checkpoints in ${SECONDS}s"
grep -nE "Kernel shell spawned|daemon spawned|VFS: auto-mounted" "$LOG" || true
