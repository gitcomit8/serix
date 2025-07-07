# === CONFIGURATION ===
TARGET = x86_64-unknown-uefi
TARGET_DIR = target/$(TARGET)/release
KERNEL_NAME = kernel
EFI_PATH = esp/EFI/BOOT
EFI_FILE = $(EFI_PATH)/BOOTX64.EFI

REMOTE_BUILD_DIR = $(shell pwd)
LAPTOP_USER = amb
LAPTOP_HOST = ideapad
LAPTOP_DEST_DIR = ~/kernel

# === BUILD ===
all: esp

build:
	cargo +nightly build --release --target $(TARGET)

esp: build
	mkdir -p $(EFI_PATH)
	cp $(TARGET_DIR)/$(KERNEL_NAME).efi $(EFI_FILE)

copy: esp
	scp -r esp $(LAPTOP_USER)@$(LAPTOP_HOST):$(LAPTOP_DEST_DIR)/

clean:
	cargo clean
	rm -rf esp

help:
	@echo "make esp   - builds the UEFI kernel and populates esp/"
	@echo "make copy  - SCPs the esp/ directory to your laptop"
	@echo "make clean - cleans everything"
