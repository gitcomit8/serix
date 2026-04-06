KERNEL = target/x86_64-unknown-none/release/kernel
ISO = serix.iso
ISO_ROOT = iso_root
LIMINE_DIR = limine

LIMINE_BRANCH = v10.x-binary
LIMINE_URL = https://github.com/limine-bootloader/limine.git

.PHONY: all run iso disk clean limine kernel

all: iso

kernel:
	cargo build --manifest-path kernel/Cargo.toml --release --target x86_64-unknown-none

limine:
	if [ ! -d $(LIMINE_DIR) ]; then \
		git clone $(LIMINE_URL) --branch=$(LIMINE_BRANCH) --depth=1 $(LIMINE_DIR); \
	fi
	make -C $(LIMINE_DIR)

$(ISO_ROOT)/boot:
	mkdir -p $@

$(ISO_ROOT)/boot/kernel: kernel | $(ISO_ROOT)/boot
	cp $(KERNEL) $@

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

disk: init rsh
	@if [ ! -f disk.img ] || [ "$$(file disk.img | grep -c FAT)" -eq 0 ]; then \
		dd if=/dev/zero of=disk.img bs=1M count=32 2>/dev/null; \
		mkfs.vfat -F 32 -n SERIX disk.img; \
		echo "Formatted disk.img as FAT32 (SERIX)"; \
	else \
		echo "disk.img already FAT32, skipping format"; \
	fi
	@mkdir -p disk_mount && \
	sudo mount -o loop disk.img disk_mount && \
	sudo cp target/x86_64-unknown-none/release/examples/init disk_mount/init && \
	sudo cp ../rsh/target/x86_64-unknown-none/release/rsh disk_mount/rsh && \
	sudo umount disk_mount && \
	rmdir disk_mount && \
	echo "Copied init and rsh to disk.img" || echo "Warning: Failed to copy to disk.img"

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
