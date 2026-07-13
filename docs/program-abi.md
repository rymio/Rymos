# RYMOS Program ABI

RYMOS programs are Rust-first, `no_std` ELF binaries.

Current ABI status:

- ABI version: `10`
- Architecture: `x86_64`
- Format: ELF64
- Runtime crate: `runtime/rymos-user`
- Target spec: `targets/x86_64-rymos.json`
- Load area: `0x200000..0x300000`
- Entry point: `_start`
- Calling convention: `extern "sysv64"`
- Arguments: raw byte slice copied through `args`
- Files: read-only BootFS access through `file_size` and `file_read`, plus
  descriptor IO through `open/read/write/seek/close`, plus `unlink` and
  `rename`
- Standard descriptors: fd `0` stdin, fd `1` stdout, fd `2` stderr
- Input: blocking line input through `read_line`
- Process: `pid`, `spawn`, and `wait`
- Metadata: `stat` and `list`
- Directories: nested `mkdir` for RYMFS within the compact path limit
- Environment: read-only `env_get` and `env_list`
- Memory: `mem_alloc_pages` maps zeroed pages for runtime heap growth
- Time: `time_ticks` returns monotonic CPU ticks
- Function signature:

```rust
extern "sysv64" fn _start(abi: *const RymosAbi) -> i32
```

The current loader runs trusted programs in kernel mode. This is enough to
prove the ABI and compile Rust apps, but not yet a protected userspace model.

## ABI Table

```rust
#[repr(C)]
pub struct RymosAbi {
    pub version: u32,
    pub write: extern "sysv64" fn(*const u8, usize),
    pub pid: extern "sysv64" fn() -> u32,
    pub args: extern "sysv64" fn(*mut u8, usize) -> usize,
    pub read_line: extern "sysv64" fn(*mut u8, usize) -> usize,
    pub file_size: extern "sysv64" fn(*const u8, usize) -> isize,
    pub file_read: extern "sysv64" fn(*const u8, usize, *mut u8, usize) -> isize,
    pub open: extern "sysv64" fn(*const u8, usize, u32) -> i32,
    pub read: extern "sysv64" fn(i32, *mut u8, usize) -> isize,
    pub write_fd: extern "sysv64" fn(i32, *const u8, usize) -> isize,
    pub seek: extern "sysv64" fn(i32, usize) -> isize,
    pub close: extern "sysv64" fn(i32) -> i32,
    pub stat: extern "sysv64" fn(*const u8, usize, *mut RymosStat) -> i32,
    pub list: extern "sysv64" fn(*const u8, usize, usize, *mut u8, usize, *mut RymosStat) -> isize,
    pub mkdir: extern "sysv64" fn(*const u8, usize) -> i32,
    pub env_get: extern "sysv64" fn(*const u8, usize, *mut u8, usize) -> isize,
    pub env_list: extern "sysv64" fn(usize, *mut u8, usize, *mut u8, usize) -> isize,
    pub spawn: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub wait: extern "sysv64" fn(u32, *mut RymosProcessStatus) -> i32,
    pub mem_alloc_pages: extern "sysv64" fn(usize) -> u64,
    pub time_ticks: extern "sysv64" fn() -> u64,
    pub unlink: extern "sysv64" fn(*const u8, usize) -> i32,
    pub rename: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
}
```

`write(ptr, len)` writes bytes to the active RYMOS console.

`pid()` returns the current process ID.

`args(ptr, len)` copies the raw command-line argument bytes into the caller
buffer and returns bytes copied. Passing a null or zero-length buffer returns
the total argument length.

`read_line(ptr, len)` reads a line from the active console and returns bytes
copied, excluding the newline.

`file_size(path_ptr, path_len)` returns the size of a read-only BootFS file, or
`-1` when missing.

`file_read(path_ptr, path_len, buffer_ptr, buffer_len)` copies bytes from a
read-only BootFS file and returns bytes copied, or `-1` on error.

`open(path_ptr, path_len, flags)` opens a small process-local file descriptor,
or `-1` on error. Plain paths open read-only BootFS files. Paths prefixed with
`pfs:` open RYMFS persistent files.

`read(fd, buffer_ptr, buffer_len)` reads from a descriptor and advances its
offset. It returns bytes copied, `0` at EOF, or `-1` on error.

`write_fd(fd, buffer_ptr, buffer_len)` writes to a writable descriptor and
advances its offset. Today this is supported for `pfs:` files only.

`seek(fd, offset)` sets an absolute descriptor offset and returns the new
offset, or `-1` on error.

`close(fd)` closes a descriptor and returns `0`, or `-1` on error.

`stat(path_ptr, path_len, stat_ptr)` fills file metadata for BootFS or `pfs:`
paths.

`list(namespace_ptr, namespace_len, index, name_ptr, name_len, stat_ptr)` lists
flat entries. Use an empty namespace for BootFS and `pfs:` for RYMFS.

`mkdir(path_ptr, path_len)` creates a RYMFS directory. Paths must use the
`pfs:` prefix, for example `pfs:src` or `pfs:src/bin`. Parent directories must
already exist.

`env_get(key_ptr, key_len, value_ptr, value_len)` copies a read-only
environment value and returns bytes copied, or `-1`.

`env_list(index, key_ptr, key_len, value_ptr, value_len)` enumerates read-only
environment entries.

`spawn(name_ptr, name_len, args_ptr, args_len)` reserves the ABI slot for
program launch and currently returns `-2` because programs are still loaded at
fixed physical addresses. A real child load would overwrite the caller until
RYMOS has isolated or relocatable app loading.

`wait(pid, status_ptr)` fills `RymosProcessStatus` for any known process in the
kernel process table and returns `0`, or `-1` if the PID is unknown.

`mem_alloc_pages(page_count)` maps zeroed pages into the current program's heap
virtual window and returns the base virtual address, or `0` on failure.

`time_ticks()` returns a monotonic CPU timestamp counter value. It is not
calibrated to wall-clock time yet.

`unlink(path_ptr, path_len)` removes a RYMFS file or empty directory. Paths
must use `pfs:`.

`rename(old_ptr, old_len, new_ptr, new_len)` renames a RYMFS file or empty
directory. Parent directories for the destination must already exist.

## Core Runtime

Milestone 4 adds `rymos-user`, the first user-program runtime crate. Programs
now implement `rymos_main()` and use safe wrappers for the ABI calls:

- `print`, `println`, and `write`
- `abi_version` and `pid`
- `args` and `read_line`
- `file_size` and `file_read`
- `File::open`, `File::read`, and `File::close`
- `env_get`, `env_list`, `spawn`, `wait`, `mem_alloc_pages`, `time_ticks`,
  `unlink`, and `rename`

The runtime owns the exported `_start(abi)` trampoline, stores the ABI pointer,
and provides the panic handler. Application code should depend on
`rymos-user` instead of defining the ABI table by hand.

## Example

See `programs/hello`.

```rust
#![no_std]
#![no_main]

use rymos_user as rt;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("hello from a Rust RYMOS program");
    rt::print("pid: ");
    rt::print_usize(rt::pid() as usize);
    rt::write(b"\n");
    0
}
```

## Build

Programs are linked with the shared linker script at `0x200000`:

```sh
make programs
```

For SDK details and custom target mode, see `docs/sdk.md`.

The build copies `target/x86_64-unknown-none/release/hello` to:

```text
bootfs/programs/hello.elf
```

Then `make image` packages it into `INITRD.RFS`.

## Run

Inside RYMOS:

```text
run hello
```

With an argument:

```text
run hello config.sys
```

The shell resolves that to:

```text
programs/hello.elf
```

## Milestone Status

Milestones 1 through 8 are complete:

- console write
- process ID
- program args
- blocking console line input
- read-only BootFS file size/read
- exit code by return value
- fixed process table with PID, state, and exit code
- `rymos-user` core runtime crate
- `x86_64-rymos` target spec and SDK build/install wrapper
- base package manifest and BootFS package index
- `rysh` tiny language interpreter running through the program ABI
- Rust self-hosting readiness report for the future compiler port
- ABI v5 RYMFS directory creation/listing
- ABI v6 read-only environment access
- ABI v7 process wait/status and a guarded spawn slot
- ABI v8 kernel-backed heap page allocation with per-process virtual windows
- ABI v9 standard descriptors and monotonic ticks
- ABI v10 RYMFS unlink/rename
- `liballoc` bump heap in `rymos-user` grows through mapped pages

## Next ABI Milestones

- Add larger-file dynamic persistent filesystem allocation.
- Add `exit` as a kernel service instead of returning directly.
- Reclaim page-table pages for exited process heap windows.
- Add safe process spawning after isolated or relocatable app loading.
- Add per-process environment.
- Move from trusted kernel-mode apps to ring-3 userspace plus syscalls.
