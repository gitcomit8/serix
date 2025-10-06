KERNEL_TARGET=target/x86_64-unknown-none/release/kernel
ISO=serix.iso
ISO_ROOT=iso_root
MULTIBOOT_HEADER=multiboot_header.o
BOOT=boot.o

.PHONY: all run iso clean kernel multiboot

all: iso

# Assemble multiboot header
$(MULTIBOOT_HEADER): multiboot_header.s
	nasm -f elf64 multiboot_header.s -o $(MULTIBOOT_HEADER)

# Assemble boot entry
$(BOOT): boot.s
	nasm -f elf64 boot.s -o $(BOOT)

# Build Rust kernel in release mode
kernel: $(MULTIBOOT_HEADER) $(BOOT)
	cargo build --release --target x86_64-unknown-none

iso: kernel $(MULTIBOOT_HEADER) $(BOOT)
	# Ensure ISO root exists
	mkdir -p $(ISO_ROOT)/boot/grub
	# Copy kernel binary
	cp $(KERNEL_TARGET) $(ISO_ROOT)/boot/kernel
	# Copy GRUB config
	cp grub.cfg $(ISO_ROOT)/boot/grub/grub.cfg
	# Create bootable ISO with GRUB
	grub-mkrescue -o $(ISO) $(ISO_ROOT)

run: iso
	qemu-system-x86_64 -cdrom $(ISO) -m 512 -serial stdio

clean:
	rm -rf $(ISO_ROOT) $(ISO) $(MULTIBOOT_HEADER) $(BOOT)
	cargo clean
