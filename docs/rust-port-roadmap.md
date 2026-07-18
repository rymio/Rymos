# RYMOS Cargo And Rustc Port Roadmap

RYMOS can run Rust-built `no_std` ELF programs today. The goal of this roadmap
is to turn that into enough OS surface for `cargo` helpers and eventually
`rustc`.

For the exact commands to set up a machine and rebuild everything by hand
(including the forked `rustc` toolchain this roadmap depends on), see
`docs/dev-environment.md`.

## Current Foundation

- ABI v21 program entry through `rymos-user`.
- BootFS read handles and RYMFS read/write/seek handles.
- Nested RYMFS directories within the compact path limit.
- RYMFS unlink/rename for files and empty directories.
- RYMFS5 non-contiguous extents (up to 4 per file) with 256 metadata entries
  and 96-byte names, plus per-entry created/modified tick timestamps, a
  permission-style mode byte, and sparse writes (seeking past the current
  end and writing zero-fills the gap). Growth coalesces into the last extent
  when possible, so ordinary sequential writes don't burn through the extent
  budget on nothing but physically-adjacent sectors. The 256/96 ceilings are
  a raised fixed cap, not unbounded growth -- the header is a stack-local
  buffer in a few places, which is the real limiter on how far a fixed bump
  can safely go without also moving the entry table onto growable disk
  extents.
- RYMFS open flags include read, write, create, truncate, append, and
  create-new.
- `rymos-user` exposes `File::options()` as a small `OpenOptions`-style
  builder.
- `fswalk` verifies nested directories, append, create-new failure, rename,
  relative `.`/`..` paths, stat, and list.
- Per-program cwd, PFS relative path resolution, and errno-style last error.
- Process-local in-memory pipes.
- Std fd redirection through `dup2`.
- Synchronous spawn with waitable child status.
- Synchronous spawn inherits cwd and std fd mappings.
- Parent PID tracking plus consuming `wait(pid)` and `wait_any()` for exited
  children.
- `echoin` verifies child stdin can be fed through an inherited pipe.
- `spawnio` verifies combined child stdin feed and stdout capture.
- `spawnioe` verifies separate stdout/stderr capture.
- `cmdapi` verifies the reusable runtime `Command`/`command_output` helper,
  including argv-style `.arg(...)` assembly inside the current raw arg buffer.
- `hello` verifies ABI `argv_count`/`argv_get` reads.
- `Command::arg` uses argv-preserving `spawn_argv`, including args with spaces.
- Process-local environment overrides through `env_set` and `env_remove`.
- `Command::env` verifies child-only environment overrides with parent restore.
- `Command::status` verifies streamed child output plus exit-status checking.
- `Command::stdout_file` and `Command::stderr_file` verify status-mode child
  output can be redirected into RYMFS files.
- `rymos-user::stdish` starts a small std-shaped shim for fs, env, process, io,
  time, path, temp dirs, and errno-style errors.
- `stdshim` verifies the std-shaped runtime shim from inside RYMOS.
- Read-only environment variables.
- Standard descriptors for stdin/stdout/stderr.
- Monotonic CPU tick time source.
- Process table, PID, exit status, and wait/status queries.
- UEFI memory map, physical page allocator, kernel-owned PML4, virtual mapping,
  per-process heap windows, and heap page reclaim.
- mmap-like guarded `mem_map_pages` / `mem_unmap_pages` regions with
  process-exit reclaim.
- `alloc::Vec` and `alloc::String` work through kernel-mapped heap pages.
- `targets/x86_64-rymos.json` parses on current nightly (`target-pointer-width`
  and `target-c-int-width` as integers, explicit `rustc-abi = "softfloat"`,
  `max-atomic-width`); `core`, `alloc`, and `compiler_builtins` now build and
  link for `x86_64-rymos` via nightly `-Z build-std`, verified end to end by
  building and installing `hello` through `RYMOS_TARGET_MODE=custom`.
- A real `std` program compiles, links, and **runs** on `x86_64-rymos`.
  `toolchain/rust` is a git submodule forking `rust-lang/rust`, built locally
  with `./x.py build library/std --stage 1` (using `download-ci-llvm` to skip
  building LLVM, ~3.5 minutes on this machine) and linked with
  `rustup toolchain link rymos-fork toolchain/rust/build/host/stage1`. Patches
  on top of upstream:
  - `library/std/src/sys/pal/rymos/` -- based on `sys::pal::unsupported`
    (`init`/`cleanup`/`unsupported`/`abort_internal`), plus a real
    `#[unsafe(no_mangle)] extern "sysv64" fn _start(abi: *const RymosAbi) ->
    i32` that stores the ABI pointer (in `sys::pal::rymos::abi`, an
    independent `#[repr(C)]` view of `RymosAbi` kept in sync by hand -- same
    as the kernel/`rymos-user` copies already are) and calls the
    compiler-generated `main(argc, argv)` symbol from `#[lang = "start"]`.
    Same pattern as Hermit's `sys::pal::hermit::runtime_entry`.
  - `library/std/src/sys/alloc/rymos.rs` -- a real bump allocator over the
    ABI's `mem_alloc_pages`, mirroring `rymos-user`'s `BumpAllocator` exactly
    (needed once `std` itself allocates internally, e.g. the `Arc<Thread>`
    handle `Stdout`'s reentrant lock creates on first use -- see the postmortem
    in `docs/self-hosting.md`'s Recently Closed section).
  - `sys::stdio::rymos` -- `Stdout`/`Stderr::write` call the ABI's console
    `write`; `Stdin`/`panic_output` still behave like `unsupported` (no
    input, panic messages are discarded).
  - `sys::thread_local`, `sys::sync::{mutex,condvar,once,rwlock}` reuse the
    existing `no_threads` fallback (RYMOS processes are single-threaded
    today); `sys::random` reuses `unsupported::{fill_bytes,
    hashmap_random_keys}` (panics -- no randomness source yet);
    `sys::io::error` reuses the `generic` fallback (no real errno mapping
    yet).
  - `targets/x86_64-rymos.json` needed `"singlethread": true` to satisfy the
    `no_threads` implementations' `cfg(target_has_threads)` compile-time
    guard, since our CPU target otherwise looks fully thread-capable to
    rustc.
  - `library/std/Cargo.toml` needed a `check-cfg` entry for
    `cfg(target_os, values("rymos"))` since `rymos` isn't a known upstream
    target.
  - Found and fixed along the way, unrelated to the std work itself: the
    bootloader's `INITRD_READ_LIMIT` was a hardcoded 4 MiB with no size
    check before the UEFI read call, so it silently truncated bootfs once a
    debug `std` binary's much larger size pushed the total initrd past it.
    Raised to 32 MiB.
  Verified live in QEMU: a real `#![feature(restricted_std)]` binary using
  `println!`, `Vec`, and iterators printed correct output and exited cleanly
  with heap reclaimed. Still stubbed: `sys::fs`, `sys::env`, `sys::process`,
  `sys::time`, real randomness, real errno mapping.
- Spawned children now run in their own **isolated address space** instead of
  overwriting the parent's fixed-address program image. `create_process_address_space`
  gives each process a private PML4: a shallow top-level clone of the kernel's
  own PML4 (so kernel code and every process's heap/mmap windows stay shared),
  plus selectively-privatized PDPT/PD entries just for the `APP_LOAD_MIN..APP_LOAD_MAX`
  image range (copying their entries first, so anything outside that range --
  like the kernel's own low-memory mappings -- keeps pointing at the original
  shared tables). A new `load_program_elf_isolated` maps fresh physical pages
  for each `PT_LOAD` segment into the child's private tables instead of
  writing through the old shared identity-mapped window;
  `destroy_process_address_space` reclaims the private PT/PD/PDPT/PML4 pages
  (plus any data pages still mapped there) on exit. This let `spawn_prepared`
  drop its old parent-ELF-reload/restore-readonly-segments dance entirely --
  the parent's memory is untouched by construction now, not reconstructed
  afterward.

  This fixed a real, previously-live bug: the old scheme only ever reloaded
  the parent's *read-only* segments after a child returned (its writable
  `.data`/`.bss` was never saved and never restored), so any mutable global a
  parent held before calling `spawn` was silently corrupted the moment a
  child ran. Verified fixed live in QEMU: `cmdapi` sets a `static mut` marker,
  spawns four children (including status/file-redirected variants), and
  confirms the marker is untouched afterward.

  Two real bugs surfaced and got fixed while building this, both worth
  remembering for any future page-table work:
  - `ensure_kernel_pml4()` used to force CR3 back to the shared kernel PML4
    on *every* call whenever CR3 didn't already match -- including calls made
    by `abi_mem_alloc_pages`/`abi_mem_map_pages` from *inside* a running
    isolated child. That would have evicted the child's own private address
    space from CR3 mid-execution the moment it allocated heap memory. Fixed
    to only force CR3 the very first time (initializing `KERNEL_PML4_PHYS`
    from the firmware's tables); after that, whatever's active in CR3 is
    always valid for shared regions (see below) so there's no need to
    disturb it.
  - A shallow PML4 clone only captures whatever top-level entries already
    exist in the source PML4 *at clone time*. Heap/mmap top-level entries are
    created lazily on first use, so if a process is the first ever to touch a
    given 512 GiB slice of the heap/mmap range, its own already-cloned PML4
    has no way to learn about that entry once the kernel's PML4 gains it
    later -- causing an immediate page fault the first time that process
    allocates heap memory. Fixed by pre-touching the specific top-level
    entries for a child's own heap/mmap windows (computable directly from its
    PID) in the *shared* kernel PML4 before cloning, guaranteeing they exist
    in every future clone from that point on.
  - Also found: different `PT_LOAD` segments can legitimately share a single
    page (e.g. a `PT_GNU_RELRO` slice sharing a page with the preceding
    rodata segment) -- the isolated loader's first pass treated an
    already-mapped page as a hard failure. Fixed with `map_image_pages`,
    which skips pages another segment already mapped instead of aborting
    (page permissions aren't enforced at the PTE level yet regardless, so
    sharing is harmless).

  Deliberately *not* done here: real concurrency. There is still no scheduler,
  no saved per-process CPU register/stack context, and no preemption --
  execution is still a synchronous nested Sys-V call. Isolation means a child
  can no longer corrupt its parent's memory; it does not mean parent and
  child run at the same time. Also not done: relocatable/PIE loading -- every
  process still uses the exact same virtual address for its image, just
  backed by different physical pages via separate page tables, which is
  isolation-via-separate-tables, not relocation.

- **Real process reaping**, and a genuine attempt (then revert) at real
  concurrent execution: `process_find_slot` used to let a brand-new `spawn`
  reuse *any* `Exited`/`Failed` process-table slot, even one nobody had ever
  called `wait`/`wait_any` on -- silently destroying a real child's exit
  status before its actual parent collected it. Fixed via
  `process_find_reapable_slot`, which now requires `waited == true` before a
  zombie's slot can be reused; a table genuinely full of unreaped zombies now
  correctly fails a new spawn with `ERR_NOSPC` instead of corrupting one.
  Top-level console `run`'d processes (which have no ABI parent that will
  ever wait on them) are marked pre-reaped at exit so they don't become
  permanent zombies under the new rule. Verified live in QEMU via a `cmdapi`
  check that spawns a child, deliberately does not wait on it right away,
  then waits and confirms its real exit status still comes back correctly.

  Separately, a deferred/queued spawn model was built and then reverted:
  `spawn` would only enqueue a child (`Ready`) and return immediately instead
  of running it inline, with `wait`/`wait_any` actually running pending
  children (letting several be queued before any of them execute). This is
  a smaller, lower-risk cut at "real concurrent execution" than a
  preemptible scheduler -- no new interrupt infrastructure, no risk of a
  child getting cut off mid-update to the ABI's shared globals, since a
  started task still always ran to completion before anything else got the
  CPU. It broke live in QEMU anyway: `echoin` spun at 100% CPU waiting on
  stdin that was never connected, because rysh's `spawnredir`/`spawnstdin`/
  `spawnio`/`spawnioe` built-ins (and every `Command` helper in
  `rymos-user`) redirect stdio/cwd/env onto shared ambient globals, call
  `spawn`, and then immediately read the result or revert the redirection --
  several with *no* `wait()` call anywhere in the function at all, relying
  entirely on `spawn` completing synchronously first. Deferring execution
  left the redirection undone (or the pipe closed) before the child ever
  got to run. Auditing and fixing every affected caller across rysh and
  `rymos-user` would be a large, separate piece of work in its own right,
  not a bounded slice of this milestone, so the deferred model was reverted
  rather than shipped half-fixed. `run_ready_task` (the extracted
  run-a-child-to-completion helper) and the `Ready`-child lookups it enabled
  stay in place as harmless defense-in-depth, but nothing relies on them
  actually finding anything under today's eager spawn. Real interleaved
  execution still needs the ABI's flat globals (`APP_FDS`, `APP_CWD`,
  `APP_ENV`, heap/mmap pointers) moved into a real per-process control block,
  regardless of whether the eventual scheduler is cooperative or preemptive.

- RYMFS4 -> RYMFS5: files are no longer limited to a single contiguous run of
  sectors. Each entry can span up to 4 extents (`start_sector`/`sector_count`
  pairs); `pfs_find_free_run` scans every other entry's extents for gaps and
  `pfs_grow_extents` fills a file's shortfall by walking those gaps,
  splitting across multiple extents when no single run is big enough.
  Critically, growth searches starting right after the file's own last
  extent and *coalesces into it* when the free run found continues exactly
  where it left off -- without that, ordinary sequential writes in small
  chunks (each one growing the file past another sector boundary) would
  burn through all 4 extent slots on nothing but physically-adjacent
  sectors within the first couple KiB written, long before the file was
  actually fragmented against other files. That was a real bug caught
  during this work: an early version passed every functional test but
  silently truncated writes at exactly 4 sectors once a file's incremental
  growth exhausted the extent budget.

  Verified live in QEMU by direct on-disk inspection, not just program
  output: three ~6-sector files were written back-to-back, the middle one
  deleted (leaving a 6-sector hole), then a ~40-sector file was written.
  Reading the persisted header afterward showed it landed in exactly two
  extents -- one reusing the hole, one after the last remaining file --
  proving the allocator actually reused the fragmented gap rather than
  simply placing everything past the tail (which an empty 4 GiB disk would
  have made trivially possible either way, so program-visible behavior
  alone wouldn't have proven this).

- Directory ceiling raised (102 -> 256 entries), names raised (30 -> 96
  bytes), file descriptors raised (8 -> 32 per process), and sparse writes
  added. The entry-count/name-length bump was constrained by a real
  concern, not just picking bigger numbers: `read_header_silent`/`format`
  declare the on-disk header as a stack-local `[u8; PFS_HEADER_BYTES]`
  array, and this kernel has no stack guard page -- an overflow would be
  silent memory corruption, not a clean crash. Chose values that keep the
  per-call header copy in the tens of KiB (not hundreds) and verified
  clean via full QEMU boot regression rather than guessing blindly.
  Sparse writes needed two changes, not one: `pfs_write_at` now zero-fills
  `[old_size, offset)` before writing when `offset > old_size`
  (`pfs_zero_range`, skipping the read for whole sectors), but that path
  was unreachable until `abi_seek` stopped rejecting `offset > handle.len`
  for RYMFS handles (it still clamps BootFS reads, which are fixed-size ROM
  data). Verified live in QEMU: `fswalk` writes 4 bytes, seeks to offset 20,
  writes 4 more, and reads back exactly `head` + 16 zero bytes + `tail`;
  also opens 20 concurrent file descriptors (well past the old 8-fd limit)
  and round-trips a 76-byte path (well past the old 30-byte limit).

## Next Required OS Work

1. Filesystem semantics:
   - richer timestamps/permissions-style metadata -- done: RYMFS4 entries
     carry created/modified tick timestamps and a mode byte (ABI v21)
   - non-contiguous allocation for fragmented large files -- done: RYMFS5
     splits a file across up to 4 extents (see Current Foundation)
   - long path support and sparse writes -- done: names raised 30 -> 96
     bytes, sparse writes zero-fill the gap on seek-past-EOF-then-write
     (see Current Foundation)
   - true unbounded directory growth (256 entries is a raised fixed
     ceiling, not the entry table living on its own growable disk extents
     -- that would also need moving the header off the kernel's stack,
     since it's a stack-local buffer in a few places today)

2. Process execution:
   - isolated `spawn` without fixed-address restore -- done: see Current
     Foundation; child and parent no longer share the same backing memory
   - true process reaping -- done: a zombie's process-table slot can no
     longer be silently reused before its real parent collects its exit
     status via `wait`/`wait_any` (see Current Foundation)
   - real concurrent execution: a scheduler, saved per-process CPU
     register/stack context, and preemption (today's isolation only means a
     child can't corrupt its parent's memory, not that they run at once). A
     deferred/queued cooperative version (no preemption, just: enqueue at
     spawn, actually run later when something waits) was attempted and
     reverted -- it broke live in QEMU, because several of rysh's shell
     built-ins and every `rymos-user` `Command` helper redirect stdio/cwd/env
     and read the result with no intervening `wait()` call at all, relying
     entirely on `spawn` finishing synchronously before returning. Real
     concurrency of any kind still needs the ABI's flat globals (fds, cwd,
     env, heap/mmap pointers) moved into a real per-process control block
     first (see Current Foundation).
   - real `exec` (replace the current process image in place)
   - wait that blocks on running children (moot until real concurrent
     execution exists -- nothing is ever pending under today's synchronous
     spawn)

3. Runtime surface:
   - broaden argv support beyond the current 8 args / 64 bytes per arg
   - grow `Command` toward more complete stdin/stdout/stderr parity
   - calibrated time/clock calls
   - randomness stub or driver
   - richer path normalization beyond the compact RYMFS path limit
   - broader errno coverage across all ABI calls
   - wire `sys::fs`/`sys::env`/`sys::process`/`sys::time` onto the real ABI
     (today only stdio and the allocator are real; a `std` binary can print
     and allocate but not yet open a file, read an env var, or spawn a
     child -- see Current Foundation)

4. Memory:
   - page-table page reclaim for the shared heap/mmap windows -- partially
     done: the new per-process private image-window PT/PD/PDPT pages are
     correctly reclaimed on exit (`destroy_process_address_space`), but the
     older, more general gap remains for heap/mmap's own PT pages (which
     live in the shared kernel PML4, not the new private tables)
   - guard pages and allocation failure behavior
   - isolated address spaces for the program-image window -- done: see
     Current Foundation
   - relocatable/PIE program loading (still unimplemented; unrelated to the
     isolation work above, which keeps every process at the same virtual
     address)

## Port Order

1. Keep `no_std` programs working against ABI v21.
2. Build `core` and `compiler_builtins` for `x86_64-rymos`. Done: verified via
   `RYMOS_TARGET_MODE=custom` building and installing `hello`.
3. Add a tiny `std` compatibility shim for file/env/time/process basics.
   Superseded: real `std` now compiles and links via the `toolchain/rust`
   fork (see Current Foundation); `stdish` remains useful groundwork for the
   ABI-wiring work below.
4. Cross-compile small CLI tools that use `std::fs` and `std::env`. `std`
   binaries run today (stdio/alloc are real); blocked on wiring `sys::fs` and
   `sys::env` onto the ABI, described above.
5. Run cargo-like helper programs that spawn children.
6. Port `cargo` after process, pipes, directories, and env are reliable.
7. Port `rustc` last, after large files, memory, and child process behavior are
   boringly dependable.

## Near-Term Test Programs

- `fswalk`: expand toward recursive trees, sparse writes, and fragmentation.
- `spawncheck`: launch a child program and verify exit status.
- `pipecheck`: parent writes to child stdin and reads child stdout.
- `heapstress`: grow toward larger mmap/heap pressure and failure-path checks.
- `stdshim`: grow toward a real `std` compatibility layer.
