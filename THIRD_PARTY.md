# Third-Party References

## MOROS

Repository: https://github.com/vinc/moros

MOROS is included under `vendor/moros` as an MIT-licensed reference for RYMOS
development.

Copyright (c) 2019-2025 Vincent Ollivier.

Current RYMOS adaptations inspired by MOROS:

- command naming and aliases such as `read`, `write`, `copy`, `move`, `delete`,
  `print`, `list`, and `goto`
- pseudo-device roadmap exposed through the `dev` command
- PCI config-space scanning approach using ports `0xCF8` and `0xCFC`

RYMOS does not currently copy MOROS subsystems wholesale. MOROS targets a
BIOS/bootloader stack with allocator-backed modules, interrupts, block devices,
and userspace. RYMOS currently uses a UEFI/FAT32 bootloader and a small
freestanding kernel, so driver adoption is staged.
