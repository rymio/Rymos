# MOROS Adoption Plan

MOROS is the clearest reference for the kind of command-line OS RYMOS can grow
into. The first safe adoption stage is command vocabulary and small drivers
that do not require heap allocation or interrupts.

## Done

- Vendor MOROS under `vendor/moros`.
- Add MIT attribution in `THIRD_PARTY.md`.
- Add MOROS-style shell aliases:
  `list`, `read`, `print`, `copy`, `move`, `delete`, and `goto`.
- Add RAMFS operations:
  `append`, `copy`, and `move`.
- Add pseudo-device inventory with `dev`.
- Add PCI config-space scanner with `pci`.
- Add read-only `INITRD.RFS` boot filesystem generated from `bootfs/`.
- Add `bootls` and `bootcat` shell commands.
- Add Rust-first program ABI v1 and `run <program>` loader.
- Add standard BootFS layout with `config.sys`, `autoexec.bat`, `programs/`,
  `config/`, `logs/`, `shared/`, and `build/`.
- Complete ABI milestone 1: args, console write/read-line, BootFS file reads,
  and program exit codes.
- Complete milestone 2: persistent RYMFS over ATA PIO in QEMU.
- Complete milestone 3: synchronous process table with PID, state, exit code,
  `ps`, `wait`, and ABI `pid`.
- Complete milestone 4: `rymos-user` core runtime crate with `_start`
  trampoline, panic handler, and Rust helpers for ABI v1.
- Complete milestone 5: custom `x86_64-rymos` target spec and SDK-style
  program build/install wrapper.
- Complete milestone 6: local package manifest, package listing, install-all,
  and BootFS installed package index.
- Complete milestone 7: `rysh` tiny interpreted language packaged and run as a
  normal RYMOS program.
- Complete milestone 8: Rust self-hosting readiness manifest and BootFS status
  report for the future compiler port.

## Next

- Pass the UEFI memory map to the kernel and expose real `mem` data.
- Add IDT/PIC/APIC setup so keyboard input can move from polling to interrupts.
- Add a small allocator so filesystem and driver tables can grow dynamically.
- Add block-device abstraction.
- Add a FAT32 reader for the boot disk, or port a MOROS-like custom filesystem.
- Add ATA/AHCI or VirtIO block support.
- Add xHCI and USB HID keyboard support for real USB keyboards.
- Add RTC and timer devices.
- Add NIC driver and network stack.
- Add process/syscall boundaries after the shell stops being purely in-kernel.
