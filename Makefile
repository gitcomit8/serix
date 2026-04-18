KERNEL = target/x86_64-unknown-none/release/kernel
ISO = serix.iso
ISO_ROOT = iso_root
LIMINE_DIR = limine

LIMINE_BRANCH = v10.x-binary
LIMINE_URL = https://github.com/limine-bootloader/limine.git

.PHONY: all run iso disk clean limine kernel ext4d ext4disk

all: iso

ext4d:
	RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" \
	  cargo build -p ext4d --release --target x86_64-unknown-none

kernel: ext4d
	cargo build --manifest-path kernel/Cargo.toml --release --target x86_64-unknown-none

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

QEMU_COMMON = -m 4G -boot d -cdrom $(ISO) \
	-drive file=disk.img,if=none,format=raw,id=x0 \
	-device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off

run: iso disk
	qemu-system-x86_64 $(QEMU_COMMON) -serial stdio

run-nographic: iso disk
	qemu-system-x86_64 $(QEMU_COMMON) -nographic

run-debug: iso disk
	qemu-system-x86_64 $(QEMU_COMMON) -serial stdio \
		-d int,cpu_reset -no-reboot

run-gdb: iso disk
	qemu-system-x86_64 $(QEMU_COMMON) -serial stdio \
		-s -S

clean:
	rm -rf $(ISO_ROOT) $(ISO)
	cargo clean --manifest-path kernel/Cargo.toml
