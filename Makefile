KERNEL = target/x86_64-unknown-none/release/kernel
ISO = serix.iso
ISO_ROOT = iso_root
LIMINE_DIR = limine

LIMINE_BRANCH = v10.x-binary
LIMINE_URL = https://github.com/limine-bootloader/limine.git

.PHONY: all run iso clean limine kernel

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
	RUSTFLAGS="-C link-arg=-Tuser.ld" cargo build \
        -p ulib \
        --example init \
        --release \
        --target x86_64-unknown-none

iso: init $(ISO_ROOT)/boot/kernel $(ISO_ROOT)/boot/limine-bios.sys $(ISO_ROOT)/boot/limine-bios-cd.bin $(ISO_ROOT)/boot/limine-uefi-cd.bin $(ISO_ROOT)/limine.conf
	xorriso -as mkisofs -b boot/limine-bios-cd.bin \
	  -no-emul-boot -boot-load-size 4 -boot-info-table \
	  --efi-boot boot/limine-uefi-cd.bin \
	  -efi-boot-part --efi-boot-image --protective-msdos-label \
	  $(ISO_ROOT) -o $(ISO)
	./limine/limine bios-install $(ISO)

run: iso
	qemu-system-x86_64 -cdrom $(ISO) -m 4G -serial stdio

clean:
	rm -rf $(ISO_ROOT) $(ISO)
	cargo clean --manifest-path kernel/Cargo.toml
