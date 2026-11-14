KERNEL = target/x86_64-unknown-none/release/kernel
ISO = serix.iso
ISO_ROOT = iso_root
LIMINE_DIR = limine

# Second QEMU disk: a GPT image with two partitions, so every boot exercises
# the multi-device and partition paths (/dev/sda whole-disk, /dev/sdb1-2).
GPT_IMG = gpt.img

LIMINE_BRANCH = v10.x-binary
LIMINE_URL = https://github.com/limine-bootloader/limine.git

.PHONY: all run run-host iso disk gptdisk clean limine kernel ext4d ext4disk policyd

all: iso

ext4d:
	RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" \
	  cargo build -Zbuild-std=core,alloc,compiler_builtins -Zbuild-std-features=compiler-builtins-mem -p ext4d --release --target x86_64-unknown-none

policyd:
	RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" \
	  cargo build -Zbuild-std=core,alloc,compiler_builtins -Zbuild-std-features=compiler-builtins-mem -p policyd --release --target x86_64-unknown-none

kernel: ext4d policyd
	cargo build -Zbuild-std=core,alloc,compiler_builtins -Zbuild-std-features=compiler-builtins-mem --manifest-path kernel/Cargo.toml --release --target x86_64-unknown-none

limine:
	if [ ! -d $(LIMINE_DIR) ]; then \
		git clone $(LIMINE_URL) --branch=$(LIMINE_BRANCH) --depth=1 $(LIMINE_DIR); \
	fi
	make -C $(LIMINE_DIR)

$(ISO_ROOT)/boot:
	mkdir -p $@

$(ISO_ROOT)/boot/kernel: kernel ext4d | $(ISO_ROOT)/boot
	cp $(KERNEL) $@

ext4disk:
	@if [ ! -f ext4.img ]; then \
		dd if=/dev/zero of=ext4.img bs=1M count=128 2>/dev/null; \
		mkfs.ext4 -O ^has_journal,^64bit,^metadata_csum,^dir_index ext4.img; \
		echo "Formatted ext4.img"; \
	else \
		echo "ext4.img exists, skipping"; \
	fi

$(ISO_ROOT)/boot/limine-bios.sys $(ISO_ROOT)/boot/limine-bios-cd.bin $(ISO_ROOT)/boot/limine-uefi-cd.bin: limine | $(ISO_ROOT)/boot
	cp $(LIMINE_DIR)/limine-bios.sys $(ISO_ROOT)/boot/
	cp $(LIMINE_DIR)/limine-bios-cd.bin $(ISO_ROOT)/boot/
	cp $(LIMINE_DIR)/limine-uefi-cd.bin $(ISO_ROOT)/boot/

$(ISO_ROOT)/limine.conf:
	cp ./limine.conf $@

init:
	RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" cargo build \
	    -p ulib \
	    --example init \
	    --release \
	    --target x86_64-unknown-none

rsh:
	RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" cargo build \
	    --manifest-path ../rsh/Cargo.toml \
	    --release \
	    --target x86_64-unknown-none

iso: $(ISO_ROOT)/boot/kernel $(ISO_ROOT)/boot/limine-bios.sys $(ISO_ROOT)/boot/limine-bios-cd.bin $(ISO_ROOT)/boot/limine-uefi-cd.bin $(ISO_ROOT)/limine.conf
	xorriso -as mkisofs -b boot/limine-bios-cd.bin \
	  -no-emul-boot -boot-load-size 4 -boot-info-table \
	  --efi-boot boot/limine-uefi-cd.bin \
	  -efi-boot-part --efi-boot-image --protective-msdos-label \
	  $(ISO_ROOT) -o $(ISO)
	./limine/limine bios-install $(ISO)

disk:
	@if [ ! -f disk.img ] || [ "$$(file disk.img | grep -c ext2)" -eq 0 ]; then \
		dd if=/dev/zero of=disk.img bs=1M count=64 2>/dev/null; \
		mkfs.ext2 -O ^resize_inode disk.img; \
		echo "Formatted disk.img as ext2"; \
	else \
		echo "disk.img already ext2, skipping format"; \
	fi

gptdisk:
	@if [ ! -f $(GPT_IMG) ]; then \
		dd if=/dev/zero of=$(GPT_IMG) bs=1M count=64 2>/dev/null; \
		printf 'label: gpt\nstart=2048, size=16384, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="serix-boot"\nstart=18432, size=110592, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="serix-root"\n' \
			| sfdisk --quiet $(GPT_IMG); \
		echo "Created $(GPT_IMG) (GPT, 2 partitions)"; \
	else \
		echo "$(GPT_IMG) exists, skipping"; \
	fi

QEMU_ACCEL ?= tcg
QEMU_CPU ?= max
QEMU_SMP_CPUS ?= 4
QEMU_SOCKETS ?= 1
QEMU_CORES ?= $(QEMU_SMP_CPUS)
QEMU_THREADS ?= 1
QEMU_MAXCPUS ?= $(shell expr $(QEMU_SOCKETS) \* $(QEMU_CORES) \* $(QEMU_THREADS))
QEMU_IOMMU ?= -device intel-iommu

HOST_QEMU_CPUS ?= $(shell nproc | awk '{ print ($$1 > 16) ? 16 : $$1 }')
HOST_QEMU_SOCKETS ?= $(shell lscpu -b -p=SOCKET | awk -F, '!/^#/ { seen[$$1]=1 } END { print length(seen) }')
# QEMU -smp requires a rectangular topology. Present each available host
# logical CPU as one guest core; this avoids inventing SMT siblings on hybrid CPUs.
HOST_QEMU_CORES ?= $(shell expr $(HOST_QEMU_CPUS) / $(HOST_QEMU_SOCKETS))
HOST_QEMU_THREADS ?= 1
HOST_QEMU_MAXCPUS ?= $(shell expr $(HOST_QEMU_SOCKETS) \* $(HOST_QEMU_CORES) \* $(HOST_QEMU_THREADS))

QEMU_COMMON = -machine q35,accel=$(QEMU_ACCEL) -cpu $(QEMU_CPU) -m 4G \
	-smp cpus=$(QEMU_SMP_CPUS),sockets=$(QEMU_SOCKETS),cores=$(QEMU_CORES),threads=$(QEMU_THREADS),maxcpus=$(QEMU_MAXCPUS) \
	-boot d -cdrom $(ISO) \
	-drive file=disk.img,if=none,format=raw,id=x0 \
	-device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off \
	-drive file=$(GPT_IMG),if=none,format=raw,id=x1 \
	-device virtio-blk-pci,drive=x1,disable-legacy=on,disable-modern=off $(QEMU_IOMMU)

run: iso disk gptdisk
	qemu-system-x86_64 $(QEMU_COMMON) -serial stdio

# KVM host CPU passthrough with the host's socket/core/thread counts. The
# kernel currently supports 16 online CPUs; excess host logical CPUs are capped.
run-host: iso disk
	$(MAKE) run QEMU_ACCEL=kvm QEMU_CPU=host \
	  QEMU_SMP_CPUS=$(HOST_QEMU_CPUS) QEMU_SOCKETS=$(HOST_QEMU_SOCKETS) \
	  QEMU_CORES=$(HOST_QEMU_CORES) QEMU_THREADS=$(HOST_QEMU_THREADS) \
	  QEMU_MAXCPUS=$(HOST_QEMU_MAXCPUS)

run-nographic: iso disk gptdisk
	qemu-system-x86_64 $(QEMU_COMMON) -nographic

run-debug: iso disk gptdisk
	qemu-system-x86_64 $(QEMU_COMMON) -serial stdio \
		-d int,cpu_reset -no-reboot

run-gdb: iso disk gptdisk
	qemu-system-x86_64 $(QEMU_COMMON) -serial stdio \
		-s -S

clean:
	rm -rf $(ISO_ROOT) $(ISO)
	cargo clean --manifest-path kernel/Cargo.toml
