# RYMOS Rust Self-Hosting

Milestone 8 does not port `rustc` yet. It creates the tracking layer for that
future port and packages the current status into the OS image.

The source manifest is:

```text
rymos-selfhost.toml
```

The generated BootFS report is:

```text
bootfs/build/selfhost.txt
```

Generate it with:

```sh
make selfhost-status
```

`make programs` also regenerates it, so `make image` packages it into
`INITRD.RFS`.

Inside RYMOS:

```text
bootcat build/selfhost.txt
run rysh build/demo.rym
```

The demo script prints both the package index and self-hosting status.

## Porting Order

The realistic Rust compiler path is:

1. Keep ABI v10 stable while adding richer directory and larger-file filesystem calls.
2. Build on the physical page allocator and paging scaffolding with activated
   virtual memory and growable user heaps.
3. Add safe process services: `spawn`, `exec`, pipes, and exit/wait without overwriting callers.
4. Build `core` and `compiler_builtins` for `x86_64-rymos`.
5. Add enough `std` support for filesystem, process, env, time, sync, and IO.
6. Cross-compile small Rust tools for RYMOS.
7. Port `cargo` helpers only after process/filesystem behavior is stronger.
8. Port `rustc` last, once large files, lots of memory, and child processes
   are normal.

## Current Truth

RYMOS can run Rust-built programs, but `rustc` itself is blocked by missing
runtime foundations:

- non-contiguous persistent file allocation beyond the current contiguous extents
- richer directory semantics and metadata beyond the compact fixed table
- larger-file descriptor support with sparse allocation
- kernel-backed heap allocation, virtual memory, and `mmap`-like behavior
- safe process spawning, pipes, and concurrent child processes
- time, randomness, synchronization, and enough terminal behavior for tools
- enough `std` surface to satisfy the compiler

Current progress: the kernel receives the UEFI memory map, has a first
physical page allocator, can inspect the active PML4, can clone it into a
kernel-owned PML4, can allocate zeroed page-table pages, and can create a
verified high-half scratch virtual mapping. ABI v8 exposes kernel-backed heap
page allocation with per-process virtual heap windows, and `rymos-user` grows
its bump heap through mapped pages.
`programs/allocdemo` proves `alloc::vec::Vec` and `alloc::string::String` work
inside a RYMOS program. Heap data pages are unmapped and reclaimed when a
process exits; page-table pages are not reclaimed yet.
ABI v10 also exposes read-only environment variables, process wait/status,
standard descriptors, monotonic ticks, and RYMFS unlink/rename. RYMFS3 adds
96 metadata entries and contiguous dynamic extents inside the 4 GiB disk. Real
`spawn` is blocked until programs have isolated or relocatable address spaces.

See `docs/rust-port-roadmap.md` for the concrete cargo/rustc port sequence.

## How Far To Cargo And Rustc

RYMOS is early but on the right route. Roughly:

- `core`/`compiler_builtins`: close, once the custom target path is polished.
- `alloc`: prototype works through kernel-mapped heap pages.
- small `no_std` Rust programs: working today.
- small `std` Rust programs: medium distance; needs file, env, time, and heap
  growth.
- `cargo`: far; needs process spawning, filesystem trees, env, time, and pipes.
- `rustc`: very far; needs all of `cargo`'s foundations plus much stronger
  memory management, large files, many descriptors, and reliable child process
  execution.

That is why milestone 8 is a readiness milestone: it prevents us from losing
the map while we build the real road.
