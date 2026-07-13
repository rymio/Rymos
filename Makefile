BOOT_TARGET := x86_64-unknown-uefi
KERNEL_TARGET := x86_64-unknown-none
BOOT_EFI := target/$(BOOT_TARGET)/release/rymos-bootloader.efi
KERNEL_ELF := target/$(KERNEL_TARGET)/release/rymos-kernel
INITRD := target/initrd.rfs
IMAGE := target/rymos-fat32.img
DATA_IMAGE := target/rymos-data.img
OVMF_CODE ?= /opt/homebrew/share/qemu/edk2-x86_64-code.fd
PROGRAM ?= hello
UPLOAD_FILE ?=
UPLOAD_DEST ?=

.PHONY: all build sdk-list pkg-list selfhost-status program programs image data-disk pfs-put run run-headless clean

all: image

build:
	cargo build -p rymos-bootloader --release --target $(BOOT_TARGET)
	cargo build -p rymos-kernel --release --target $(KERNEL_TARGET)

sdk-list:
	python3 scripts/rymos-sdk.py list

pkg-list:
	python3 scripts/rymos-sdk.py pkg-list

selfhost-status:
	python3 scripts/rymos-sdk.py selfhost-status

program:
	python3 scripts/rymos-sdk.py install $(PROGRAM)

programs:
	python3 scripts/rymos-sdk.py install-all

$(INITRD): bootfs rymos-packages.toml rymos-selfhost.toml scripts/make_initrd.py programs
	python3 scripts/make_initrd.py bootfs $(INITRD)

image: build $(INITRD)
	python3 scripts/make_fat32.py $(BOOT_EFI) $(KERNEL_ELF) $(INITRD) $(IMAGE)
	@echo "Wrote $(IMAGE)"

data-disk:
	python3 scripts/make_data_disk.py $(DATA_IMAGE)

pfs-put: data-disk
	python3 scripts/pfs_put.py $(DATA_IMAGE) $(UPLOAD_FILE) $(UPLOAD_DEST)

run: image data-disk
	qemu-system-x86_64 \
		-machine pc \
		-m 256M \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive format=raw,file=$(IMAGE),if=virtio \
		-drive format=raw,file=$(DATA_IMAGE),if=ide,index=1 \
		-serial stdio \
		-monitor none \
		-no-reboot

run-headless: image data-disk
	qemu-system-x86_64 \
		-machine pc \
		-m 256M \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive format=raw,file=$(IMAGE),if=virtio \
		-drive format=raw,file=$(DATA_IMAGE),if=ide,index=1 \
		-display none \
		-serial stdio \
		-monitor none \
		-no-reboot

clean:
	cargo clean
