# RYMOS Cargo And Rustc Port Roadmap

RYMOS can run Rust-built `no_std` ELF programs today. The goal of this roadmap
is to turn that into enough OS surface for `cargo` helpers and eventually
`rustc`.

## Current Foundation

- ABI v10 program entry through `rymos-user`.
- BootFS read handles and RYMFS read/write/seek handles.
- Nested RYMFS directories within the compact path limit.
- RYMFS unlink/rename for files and empty directories.
- RYMFS3 contiguous dynamic extents with 96 metadata entries.
- Read-only environment variables.
- Standard descriptors for stdin/stdout/stderr.
- Monotonic CPU tick time source.
- Process table, PID, exit status, and wait/status queries.
- UEFI memory map, physical page allocator, kernel-owned PML4, virtual mapping,
  per-process heap windows, and heap page reclaim.
- `alloc::Vec` and `alloc::String` work through kernel-mapped heap pages.

## Next Required OS Work

1. Filesystem semantics:
   - append/truncate metadata behavior
   - non-contiguous allocation for fragmented large files
   - richer metadata and directory entries beyond the compact table

2. Process execution:
   - safe `spawn`/`exec` without overwriting callers
   - wait that blocks on running children
   - pipes

3. Runtime surface:
   - calibrated time/clock calls
   - randomness stub or driver
   - current directory and path normalization
   - errno-style error reporting

4. Memory:
   - page-table page reclaim
   - mmap-like reserve/map/unmap calls
   - guard pages and allocation failure behavior
   - isolated address spaces or relocatable program loading

## Port Order

1. Keep `no_std` programs working against ABI v10.
2. Build `core` and `compiler_builtins` for `x86_64-rymos`.
3. Add a tiny `std` compatibility shim for file/env/time/process basics.
4. Cross-compile small CLI tools that use `std::fs` and `std::env`.
5. Run cargo-like helper programs that spawn children.
6. Port `cargo` after process, pipes, directories, and env are reliable.
7. Port `rustc` last, after large files, memory, and child process behavior are
   boringly dependable.

## Near-Term Test Programs

- `fswalk`: create nested directories, write/read files, rename, delete.
- `spawncheck`: launch a child program and verify exit status.
- `pipecheck`: parent writes to child stdin and reads child stdout.
- `heapstress`: allocate multiple MiB and verify reclaim after exit.
- `stdhello`: first cross-compiled program using a small `std` subset.
