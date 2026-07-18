# RYMOS Program ABI

RYMOS programs are Rust-first, `no_std` ELF binaries.

Current ABI status:

- ABI version: `21`
- Architecture: `x86_64`
- Format: ELF64
- Runtime crate: `runtime/rymos-user`
- Runtime helpers: descriptor IO, pipes, spawn/wait, `command_output`, and
  `Command`
- Target spec: `targets/x86_64-rymos.json`
- Load area: `0x200000..0x300000`
- Entry point: `_start`
- Calling convention: `extern "sysv64"`
- Arguments: legacy raw byte slice copied through `args`, plus real argv
  spawn/read support through `spawn_argv`, `argv_count`, and `argv_get`
- Files: read-only BootFS access through `file_size` and `file_read`, plus
  descriptor IO through `open/read/write/seek/close`, plus `unlink` and
  `rename`
- Standard descriptors: fd `0` stdin, fd `1` stdout, fd `2` stderr
- Pipes: in-memory fd pairs through `pipe`
- Descriptor redirection: `dup2` can map fd `0`, `1`, or `2` to another fd
- Input: blocking line input through `read_line`
- Process: `pid`, synchronous `spawn`, consuming `wait`, and child `wait_any`
- Metadata: `stat` and `list`
- Directories: nested `mkdir` for RYMFS within the compact path limit
- Paths: per-program current directory, PFS relative paths, and last-error code
- Environment: inherited defaults plus process-local `env_get`, `env_list`,
  `env_set`, and `env_remove`
- Memory: `mem_alloc_pages` maps zeroed pages for runtime heap growth;
  `mem_map_pages` and `mem_unmap_pages` provide mmap-like regions
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
    pub env_set: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub env_remove: extern "sysv64" fn(*const u8, usize) -> i32,
    pub spawn: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub spawn_argv: extern "sysv64" fn(*const u8, usize, *const RymosArgSlice, usize) -> i32,
    pub wait: extern "sysv64" fn(u32, *mut RymosProcessStatus) -> i32,
    pub wait_any: extern "sysv64" fn(*mut RymosProcessStatus) -> i32,
    pub mem_alloc_pages: extern "sysv64" fn(usize) -> u64,
    pub mem_map_pages: extern "sysv64" fn(usize, u32) -> u64,
    pub mem_unmap_pages: extern "sysv64" fn(u64, usize) -> i32,
    pub time_ticks: extern "sysv64" fn() -> u64,
    pub unlink: extern "sysv64" fn(*const u8, usize) -> i32,
    pub rename: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub cwd: extern "sysv64" fn(*mut u8, usize) -> isize,
    pub chdir: extern "sysv64" fn(*const u8, usize) -> i32,
    pub last_error: extern "sysv64" fn() -> i32,
    pub pipe: extern "sysv64" fn(*mut i32, *mut i32) -> i32,
    pub dup2: extern "sysv64" fn(i32, i32) -> i32,
    pub argv_count: extern "sysv64" fn() -> usize,
    pub argv_get: extern "sysv64" fn(usize, *mut u8, usize) -> isize,
}
```

`write(ptr, len)` writes bytes to the active RYMOS console.

`pid()` returns the current process ID.

`args(ptr, len)` copies the raw command-line argument bytes into the caller
buffer and returns bytes copied. Passing a null or zero-length buffer returns
the total argument length. The kernel currently stores up to 64 bytes of raw
argument data per process.

`argv_count()` returns the argv-style argument count. `argv[0]` is the program
name. For legacy `spawn`, `argv[1..]` are parsed from the raw argument buffer;
for `spawn_argv`, they are copied from the passed argument slice table.

`argv_get(index, ptr, len)` copies one argv entry and returns the full entry
length, or `-1` when the index is missing.

`read_line(ptr, len)` reads a line from the active console and returns bytes
copied, excluding the newline.

`file_size(path_ptr, path_len)` returns the size of a read-only BootFS file, or
`-1` when missing.

`file_read(path_ptr, path_len, buffer_ptr, buffer_len)` copies bytes from a
read-only BootFS file and returns bytes copied, or `-1` on error.

`open(path_ptr, path_len, flags)` opens a small process-local file descriptor,
or `-1` on error. Plain paths open read-only BootFS files while cwd is `/`.
Paths prefixed with `pfs:` open RYMFS persistent files. When cwd is `pfs:` or
below, relative paths resolve inside RYMFS. Supported flags are read (`1`),
write (`2`), create (`4`), truncate (`8`), append (`16`), and create-new
(`32`). `create_new` fails with `ERR_EXIST` when the path already exists.

`read(fd, buffer_ptr, buffer_len)` reads from a descriptor and advances its
offset. It returns bytes copied, `0` at EOF, or `-1` on error.

`write_fd(fd, buffer_ptr, buffer_len)` writes to a writable descriptor and
advances its offset. Append-mode descriptors write at EOF on every call. Today
file writes are supported for `pfs:` files only.

`seek(fd, offset)` sets an absolute descriptor offset and returns the new
offset, or `-1` on error.

`close(fd)` closes a descriptor and returns `0`, or `-1` on error.

`stat(path_ptr, path_len, stat_ptr)` fills file metadata for BootFS or RYMFS
paths. `RymosStat` carries `kind`, `fs`, `size`, `created_ticks`,
`modified_ticks`, and a permission-style `mode` bitmask (`1`=read, `2`=write,
`4`=exec). RYMFS entries (RYMFS5) persist real tick timestamps set at create
time and refreshed on every write/append/truncate; BootFS entries are
read-only ROM data and report `created_ticks`/`modified_ticks` as `0` with a
read+exec mode.

`list(namespace_ptr, namespace_len, index, name_ptr, name_len, stat_ptr)` lists
entries. Use an empty namespace for BootFS at `/`, `pfs:` for RYMFS root, or a
relative namespace after `chdir` into RYMFS.

`mkdir(path_ptr, path_len)` creates a RYMFS directory. Paths may use `pfs:` or
be relative to a RYMFS cwd. Parent directories must already exist.

`env_get(key_ptr, key_len, value_ptr, value_len)` copies an environment value
and returns bytes copied, or `-1`.

`env_list(index, key_ptr, key_len, value_ptr, value_len)` enumerates inherited
defaults plus process-local environment overrides.

`env_set(key_ptr, key_len, value_ptr, value_len)` sets or overrides a
process-local environment variable. The current limits are 8 overrides, 24-byte
keys, and 96-byte values.

`env_remove(key_ptr, key_len)` removes a process-local variable or masks an
inherited default for the current process and its synchronously spawned
children.

`spawn(name_ptr, name_len, args_ptr, args_len)` runs a child program
synchronously and returns its PID after the child exits. The child inherits the
parent's cwd and std fd mappings, so stdout can be redirected into a pipe. This
is not concurrent process execution yet; the fixed-address loader restores the
parent's read-only ELF segments before returning.

`spawn_argv(name_ptr, name_len, argv_ptr, argv_len)` is the argv-preserving
variant used by the runtime `Command::arg(...)` path. It accepts up to 8 args,
each up to 64 bytes, and preserves spaces inside individual args for
`argv_get`.

`wait(pid, status_ptr)` fills `RymosProcessStatus` for an exited process and
returns `0`, or `-1` if the PID is unknown, still running, or already waited.

`wait_any(status_ptr)` fills `RymosProcessStatus` for the next exited child of
the current process and returns that child PID, or `-1` if no child status is
available.

`mem_alloc_pages(page_count)` maps zeroed pages into the current program's heap
virtual window and returns the base virtual address, or `0` on failure.

`mem_map_pages(page_count, flags)` maps a zeroed private region and returns the
base virtual address, or `0` on failure. Flag `1` reserves an unmapped guard
page before and after the returned region.

`mem_unmap_pages(address, page_count)` unmaps a region previously returned by
`mem_map_pages` and returns `0`, or `-1` on error.

`time_ticks()` returns a monotonic CPU timestamp counter value. It is not
calibrated to wall-clock time yet.

`unlink(path_ptr, path_len)` removes a RYMFS file or empty directory. Paths
must use `pfs:`.

`rename(old_ptr, old_len, new_ptr, new_len)` renames a RYMFS file or empty
directory. Parent directories for the destination must already exist.

`cwd(buffer_ptr, buffer_len)` copies the current directory, such as `/` or
`pfs:src`, and returns bytes copied. Passing a null or zero-length buffer
returns the required length.

`chdir(path_ptr, path_len)` changes the per-program cwd to `/`, `pfs:`, or an
existing RYMFS directory.

`last_error()` returns the last errno-style code set by an ABI call.

`pipe(read_fd_ptr, write_fd_ptr)` creates a process-local in-memory pipe and
writes the read/write descriptor numbers to caller memory.

`dup2(old_fd, new_fd)` redirects standard fd `0`, `1`, or `2` to another open
descriptor. Passing the same std fd for both arguments resets that std fd to
its default console behavior.

## Core Runtime

Milestone 4 adds `rymos-user`, the first user-program runtime crate. Programs
now implement `rymos_main()` and use safe wrappers for the ABI calls:

- `print`, `println`, and `write`
- `abi_version` and `pid`
- `args` and `read_line`
- `file_size` and `file_read`
- `File::open`, `File::create`, `File::append`, `File::options`,
  `File::read`, and `File::close`
- `env_get`, `env_list`, `env_set`, `env_remove`, `spawn`, `wait`,
  `mem_alloc_pages`, `mem_map_pages`, `mem_unmap_pages`, `time_ticks`,
  `unlink`, `rename`, `cwd`, `chdir`, `last_error`, `pipe`, and `dup2`
- `Command::new(...).arg(...).args_raw(...).stdin(...).env(...)
  .env_remove(...).current_dir(...).stdout_file(...).stderr_file(...)
  .output()` and `.status()` as the first small
  `std::process::Command`-shaped wrapper. The wrapper preserves `.arg(...)`
  values through the argv ABI, applies per-child environment overrides,
  restores the parent environment after spawn, and can stream status-mode child
  stdout/stderr into RYMFS files.
- `stdish::{fs, env, process, io, time, path}` as a small no-std,
  alloc-backed compatibility shim for early `std` porting work.

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
- ABI v11 cwd/path resolution and errno-style `last_error`
- ABI v12 process-local in-memory pipes
- ABI v13 std fd redirection through `dup2`
- ABI v14 synchronous spawn with waitable child status
- ABI v15 inherited std fd mappings for synchronous spawn
- ABI v16 argv-style reads for future `std::env::args`
- ABI v17 process-local environment overrides for future `std::env::set_var`
- ABI v18 argv-preserving `spawn_argv` for future `std::process::Command`
- ABI v19 parent/child tracking and consuming `wait_any`
- ABI v20 mmap-like `mem_map_pages` / `mem_unmap_pages`
- `liballoc` bump heap in `rymos-user` grows through mapped pages
- ABI v21 RYMFS4 entries carry created/modified tick timestamps and a
  permission-style mode byte, exposed through `stat`/`list`
- RYMFS4 -> RYMFS5 (no ABI version change; purely an on-disk/internal format
  upgrade): files can now span up to 4 non-contiguous extents instead of one
  contiguous run, so fragmentation from other files no longer causes
  spurious "disk full" errors

## Next ABI Milestones

- Add `exit` as a kernel service instead of returning directly.
- Reclaim page-table pages for exited process heap windows.
- Add safe process spawning after isolated or relocatable app loading.
- Add per-process environment.
- Move from trusted kernel-mode apps to ring-3 userspace plus syscalls.
