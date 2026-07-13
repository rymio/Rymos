# RYMOS

RYMOS is a Rust-based operating system experiment.

The current milestone follows the shape of Philipp Oppermann's minimal Rust
kernel chapter:

- a freestanding Rust kernel with `_start`
- `#![no_std]` and `#![no_main]`
- panic aborts into an infinite loop
- direct VGA text-buffer output at `0xb8000`
- UEFI GOP framebuffer text output at `1024x768`

RYMOS keeps the already-working UEFI/FAT32 boot path:

1. Build a UEFI bootloader.
2. Build a freestanding `x86_64-unknown-none` kernel ELF.
3. Place the bootloader at `EFI/BOOT/BOOTX64.EFI`.
4. Place the kernel at `KERNEL.ELF` in the FAT32 root.
5. Boot through UEFI, load the ELF, exit boot services, and jump to `_start`.

Expected kernel output:

```text
RYMOS minimal Rust kernel
_start reached
kernel shell online
rymos:/ $
```

## Kernel Shell

The kernel now includes a tiny command shell. It accepts input from COM1 serial
and from the basic PS/2 keyboard path in QEMU.

Current commands:

```text
help                 show commands
clear                clear screen
about                kernel summary
mem                  memory summary
video                show active video mode
df                   filesystem usage
fsformat             format persistent RYMFS disk
pls                  list persistent files
pread <file>         read persistent file
pwrite <file> <txt>  write persistent file
pdelete <file>       delete persistent file
bootls               list boot filesystem
bootcat <file>       read boot filesystem file
run <program>        run bootfs Rust program
ps                   list processes
wait <pid>           show process exit status
drivers              list kernel drivers
dev                  list pseudo devices
pci                  scan PCI config space
pwd                  print working directory
ls|list              list ramfs entries
cat|read <file>      print file
touch <file>         create empty file
write <file> <text>  replace file
append <file> <txt>  append to file
echo|print <text>    print text
cp|copy <src> <dst>  copy file
mv|move <src> <dst>  move file
rm|delete <file>     remove file
mkdir <name>         create ramfs directory entry
rmdir <name>         remove empty directory entry
cd|goto [/]          show/change current dir
reboot               reset via keyboard controller
halt                 stop the CPU
```

File commands are backed by a small fixed-size RAMFS inside the kernel. The
FAT32 volume is currently used by the UEFI bootloader to load `KERNEL.ELF` and
`INITRD.RFS`.

`INITRD.RFS` is a read-only boot filesystem generated from `bootfs/`, loaded by
the bootloader, and passed to the kernel. Use `bootls` and `bootcat <file>` to
inspect it.

An in-kernel FAT32 or block-device driver is a later milestone.

## Persistent Filesystem

Milestone 2 adds a tiny persistent filesystem named RYMFS.

The run targets create and attach this disk if missing:

```text
target/rymos-data.img
```

RYMFS is intentionally small:

- raw ATA disk, primary slave in QEMU
- RYMFS3 metadata header
- 96 entries max
- 256 MiB max per file
- contiguous dynamic extents inside the 4 GiB data disk
- compact nested paths, directories, rename, and delete
- persistent across emulator restarts

Commands:

```text
fsformat
pwrite note survives-reboot
pread note
pls
pdelete note
df
```

To upload a host file into the persistent disk image:

```sh
make pfs-put UPLOAD_FILE=/path/to/file.bin UPLOAD_DEST=uploads/file.bin
```

The data disk image is sparse by default and grows logically to 4 GiB.

Then boot RYMOS and read it as:

```text
pread uploads/file.bin
```

The first ATA driver uses legacy IDE ports, so QEMU currently runs with
`-machine pc`. AHCI/VirtIO support can bring us back to `q35` later.

## Standard BootFS Layout

RYMOS uses a simpler-than-Linux layout:

```text
autoexec.bat      startup commands executed after the kernel shell is ready
config.sys        driver and filesystem setup conventions
config/           system configuration files
programs/         RYMOS program ELF binaries
logs/             logs, once writable storage exists
shared/           shared libraries and reusable assets
build/            build metadata and packaged artifacts
```

At boot, the kernel reports `config.sys`, then runs commands from
`autoexec.bat`. The default autoexec script runs `hello` from
`programs/hello.elf`.

## Program ABI

RYMOS can now run a tiny Rust `no_std` program from BootFS:

```text
run hello
```

Programs are ELF64 binaries linked at `0x200000` and called with ABI v10:

```rust
extern "sysv64" fn _start(abi: *const RymosAbi) -> i32
```

Milestone 4 adds the first core runtime crate at `runtime/rymos-user`. User
programs now implement `rymos_main()` and call runtime helpers such as
`println`, `args`, `pid`, `file_size`, and `file_read`; the runtime owns the
`_start` trampoline and panic handler.

For details, see `docs/program-abi.md`.

ABI v10 currently supports console output, raw args, blocking line input,
read-only BootFS file reads, BootFS read handles, RYMFS read/write/seek
handles, file metadata/listing, compact nested RYMFS directories, RYMFS
unlink/rename, read-only environment variables, process wait/status, a guarded
spawn slot, kernel-backed heap page allocation, standard descriptors,
monotonic ticks, and exit codes by return value.

## SDK And Packages

Milestones 5 and 6 add a first SDK-style wrapper plus a tiny base package
manifest for RYMOS programs:

```sh
make sdk-list
make pkg-list
make program PROGRAM=hello
make programs
```

The canonical custom target spec is `targets/x86_64-rymos.json`. Since Cargo
JSON target builds still need nightly plus `rust-src`, the SDK defaults to a
stable-compatible `x86_64-unknown-none` fallback and applies the same linker
contract.

`rymos-packages.toml` lists enabled base-system programs. `make programs`
installs all enabled packages into `bootfs/programs` and writes
`bootfs/build/packages.txt`. See `docs/sdk.md`.

## Tiny Language

Milestone 7 adds `rysh`, a tiny interpreted language that runs as a normal
RYMOS program:

```text
run rysh build/demo.rym
```

The first language supports comments, `print`, `write`, `pid`, `args`, `set`,
`get`, `cat`, `writefile`, `fillfile`, `countfile`, `stat`, `listdir`,
`mkdir`, `rm`, `rename`, `env`, `getenv`, `spawn`, `wait`, and simple
`$variable` expansion. See `docs/rysh.md`.

## Rust Self-Hosting

Milestone 8 adds the self-hosting readiness track for eventually running a Rust
toolchain on RYMOS. It does not pretend `rustc` can run yet; instead it keeps a
machine-readable checklist in `rymos-selfhost.toml` and packages a generated
status report into:

```text
bootfs/build/selfhost.txt
```

Generate it directly with:

```sh
make selfhost-status
```

The `rysh` boot demo prints the report. See `docs/self-hosting.md`.

The runtime now includes a fixed 128 MiB per-program bump heap, and
`programs/allocdemo` verifies `alloc::vec::Vec` and `alloc::string::String`
inside RYMOS.

## Process Model

Milestone 3 adds the first process table.

Current behavior:

- every `run <program>` gets a PID
- process state is tracked as `ready`, `running`, `exited`, or `failed`
- exit code is stored
- `ps` lists process history
- `wait <pid>` reports the stored exit status
- programs can ask for their PID through the ABI
- programs can query process status through the ABI
- the spawn ABI exists, but returns `-2` until app loading no longer overwrites
  the caller

Processes still run synchronously and in trusted kernel mode. This is the
process model foundation before scheduling, memory isolation, and ring-3
syscalls.

## Hardware Status

Current:

- Serial 16550 output/input.
- PS/2 keyboard polling.
- VGA text buffer output.
- UEFI GOP framebuffer output at `1024x768`.
- PCI config-space scanner.
- UEFI memory-map reader and physical page allocator.
- Paging diagnostics, kernel-owned PML4 clone, zeroed page-table page allocation,
  and scratch virtual mapping.
- Volatile RAMFS.
- Read-only bootfs initrd.
- Persistent RYMFS over ATA PIO in QEMU.

Not yet:

- USB host controller driver.
- USB HID keyboard driver.
- Network driver or network stack.

On real hardware, a USB keyboard will only work today if firmware/chipset
legacy emulation exposes it as PS/2. Proper real-device keyboard support needs
xHCI plus USB HID.

## Prerequisites

Install the Rust targets:

```sh
rustup target add x86_64-unknown-uefi x86_64-unknown-none
```

Optional, for emulation:

```sh
brew install qemu
```

## Build

```sh
make image
```

This creates:

```text
target/rymos-fat32.img
```

## Run

For a QEMU display window:

```sh
make run
```

The graphical window uses the UEFI GOP framebuffer. Serial is also attached to
the terminal, so shell output appears in both places.

For terminal-only verification:

```sh
make run-headless
```

This exposes the shell over serial, so you can type commands directly in the
terminal. Stop QEMU with `Ctrl-C`.

If your OVMF firmware is installed somewhere else, pass it explicitly:

```sh
make run OVMF_CODE=/path/to/OVMF_CODE.fd
```

## Layout

```text
bootloader/src/main.rs       UEFI FAT32 ELF loader
kernel/src/main.rs           Minimal freestanding Rust kernel
kernel/linker.ld             Links the kernel at 1 MiB
runtime/rymos-user/          Core runtime crate for RYMOS programs
programs/hello/              Example Rust RYMOS program
programs/allocdemo/          User-program heap and liballoc smoke test
programs/rysh/               Tiny RYMOS script interpreter
targets/x86_64-rymos.json    Canonical custom target spec for programs
scripts/rymos-sdk.py         Program build/install wrapper
rymos-packages.toml          Base program package manifest
rymos-selfhost.toml          Rust compiler readiness manifest
docs/rust-port-roadmap.md    Cargo/rustc port readiness plan
bootfs/                      Files packaged into INITRD.RFS
scripts/make_initrd.py       Read-only boot filesystem builder
scripts/make_fat32.py        FAT32 image builder
target/rymos-fat32.img       Bootable FAT32 image
```

## Notes

The kernel is linked as a static ELF at `0x100000`. The bootloader parses
program headers, allocates the requested physical pages, copies `PT_LOAD`
segments, exits UEFI boot services, and jumps to the ELF entry point.

The bootloader requests UEFI GOP mode `1024x768` before jumping to the kernel.
The kernel renders an 8x16 bitmap font over that framebuffer, giving a
`128x48` text grid. Legacy VGA text memory is still mirrored for fallback.

MOROS is vendored in `vendor/moros` as an MIT-licensed reference. RYMOS has
started by adapting MOROS-style shell verbs, pseudo-device thinking, and a
small PCI config-space scanner. Larger MOROS subsystems like ATA, networking,
sound, and the custom filesystem need RYMOS milestones first: interrupts,
paging, block devices, and a mounted filesystem.
