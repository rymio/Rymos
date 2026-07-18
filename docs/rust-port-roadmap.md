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
  with heap reclaimed. Still stubbed at the time: `sys::fs`, `sys::env`,
  `sys::process`, `sys::time`, real randomness, real errno mapping -- see the
  next entry for all of those.

- **Real `std::fs`/`std::env`/`std::time`/`std::random` wired onto the ABI**
  (category 5): `sys::pal::rymos::abi`'s `RymosAbi` struct previously only
  had real field *types* through `mem_alloc_pages` (the last field the pal
  actually called; everything after was an untyped placeholder, by explicit
  design at the time). Extended it with every remaining field's exact C ABI
  signature, re-derived from `runtime/rymos-user/src/lib.rs`'s copy (the two
  have to match by hand, same as the kernel's own copy already does),
  including packed conventions like `env_list`'s `(key_len << 32) |
  value_len` -- cross-checked against `kernel/src/main.rs`'s `abi_env_list`
  directly, not assumed. Then implemented, for real, against it:
  - `sys::env`: `getenv`/`setenv`/`unsetenv`/`env()`, no local cache (unlike
    e.g. Hermit's `HashMap`-backed one) since the kernel is already the
    single source of truth for a process's environment.
  - `sys::args`: `args()` via `argv_count`/`argv_get`, queried live every
    call rather than cached at start (there's no ready-made argv array to
    hand off in the first place, and argv never changes after start anyway).
  - `sys::paths`: `getcwd`/`chdir`/`temp_dir` (the last reading the `TMPDIR`
    env var the kernel already seeds by default).
  - `sys::fs`: a real `File` plus `stat`/`readdir`/`mkdir`/`unlink`/
    `rename`-backed directory ops. No symlinks/hard links/file locking/
    settable permissions on RYMOS, so those honestly stay `unsupported()`.
    `remove_dir_all`/`Dir` are reused from `sys::fs::common` for free;
    `copy` is hand-written rather than reusing `common::copy`, since that
    needs fd-based `fstat` (RYMOS's `stat` is path-based only).
  - `sys::io::error`: real `ERR_*`-to-`ErrorKind` mapping via `last_error`,
    replacing the old `generic` fallback that reported every error as
    "operation successful" regardless of what actually happened.
  - `sys::random`: `RDRAND` when `CPUID` reports support, falling back to a
    `SplitMix64` stream seeded from `time_ticks` plus stack/heap addresses
    otherwise -- disclosed as not cryptographically secure, but real,
    changing output instead of the previous panic.
  - `sys::time`: `Instant` is real for ordering/equality (`time_ticks`, a
    raw `rdtsc` read), but duration arithmetic honestly returns `None`
    rather than fabricating a tick-to-nanosecond conversion factor with no
    calibration reference anywhere in the kernel to base it on (no PIT/
    timer, no CPUID TSC-frequency detection) -- category 4 already lists
    tick calibration as separate, not-yet-done work, and a silently-wrong
    number seemed worse than an honest gap here, the same call already made
    for `sys::random`'s old panic over fake output.
  - `sys::process`: `Command`/spawning deliberately stays unsupported -- a
    real architectural mismatch (RYMOS resolves programs by name through
    `bootfs`, not a resolved filesystem path; `Stdio`/pipe wiring would need
    `sys::pipe` support this port has never touched), not a bounded
    ABI-wiring task like the others. `std::process::id()` is real, though.

  Verified live in QEMU with a new `stdreal` program -- a genuine
  `#![feature(restricted_std)]` binary (unlike every other test program,
  not routed through `rymos-user`) exercising `std::process::id()`, real
  argv, env get/set/iterate/remove, `current_dir`/`temp_dir`,
  `fs::write`/`read_to_string`/`exists`/`read_dir`, `Instant` ordering, and a
  `HashMap` (exercising the random support internally) -- all correct,
  alongside the full existing `no_std` regression suite.

  Two real bugs found and fixed along the way:
  - A genuine crash on the first run: `std::process::id()` aborted via
    `ud2` instead of returning a pid. Disassembly showed the call site was
    `sys::process::unsupported::getpid` (which panics), not the real one
    just added -- a stale `-Z build-std` target cache had kept using the
    pre-edit sysroot build despite the source changing. This is the exact
    gotcha `docs/dev-environment.md` already documents (`rm -rf
    target/x86_64-rymos` before re-testing) hit live, not a new bug class.
  - (See above) the packed `env_list` return convention had to be verified
    against the kernel source directly rather than assumed from the field
    name alone.

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

- **Heap/mmap page-table reclaim, and first CPU exception handling**
  (category 3, memory): `process_reclaim_mappings` only freed a process's
  tracked *data* pages on exit, never the PT/PD page-table pages that
  pointed to them, which leaked permanently. `reclaim_process_window_tables`
  fixes this without needing a private-PML4-style walk: since PIDs are never
  reused and each PID's heap (256 MiB)/mmap (1 GiB) window sits at a fixed,
  alignment-guaranteed address purely as a function of its own PID, a heap
  window always owns a clean run of 128 PT-pointing PD entries (never a
  whole PD -- 4 PIDs share the rest of it) and a mmap window always owns one
  whole, exclusively-owned PD (freeable in full, not just its PTs). Verified
  live in QEMU: three consecutive spawn/exit cycles of the same program
  leave the allocator's used-page count exactly flat instead of growing.

  This kernel had no IDT at all before this, so any fault triple-faulted the
  whole machine with zero diagnostics. Added one covering all 32 CPU
  exception vectors, via hand-written naked-function stubs rather than the
  nightly-only `x86-interrupt` ABI (this kernel builds on stable): every
  stub normalizes the stack (a filler error code for vectors that don't get
  a real one) and jumps to one shared handler that prints vector, mnemonic,
  error code, `CR2` (page faults), faulting RIP, and the current PID/process
  name over serial, then halts. No new GDT needed -- it reads whichever code
  segment UEFI's firmware already set up. Verified live in QEMU via a new
  `faultcheck` program (deliberately outside the automated regression, since
  passing halts the machine) across both guard-page directions, a raw
  hardware divide-by-zero (Rust's own `/` panics before ever reaching
  hardware, so this needs inline asm to bypass that and exercise the real
  `#DE` path), and `ud2`. Found and disclosed, not fixed: a null-pointer
  write doesn't fault today, because address 0 is already mapped as part of
  the low-memory identity mapping rather than being guarded. Deliberately
  not attempted: recovering from a fault instead of halting everything
  (needs a prepared non-local-exit point to unwind an arbitrary nested Rust
  call stack); a dedicated IST/TSS stack for the double-fault handler (so a
  double fault from genuine stack exhaustion could still rarely triple-fault
  if the handler itself has no stack room). Assessed, not changed: per-
  process address spaces don't need to be larger yet -- the 256 MiB
  heap/768 MiB mmap windows already exceed what fits in a 256 MiB QEMU test
  VM once the 142 MiB image-window reservation is subtracted, so physical
  RAM and that reservation are the real ceiling today, not the windows.

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
   - broaden argv support beyond the current 8 args / 64 bytes per arg (this
     is `rymos-user`'s own no_std `PROCESS_ARGV_*` ceiling; unrelated to
     `std::env::args()`, which reads argv straight from the ABI with no
     fixed-count limit of its own -- see Current Foundation)
   - grow `Command` toward more complete stdin/stdout/stderr parity (the
     `rymos-user::Command` builder used by `no_std` programs; `std`'s own
     `std::process::Command` stays unsupported for a different, architectural
     reason -- see Current Foundation)
   - calibrated time/clock calls (also blocks `std::time::Instant`'s duration
     arithmetic -- see Current Foundation and category 4)
   - randomness stub or driver -- done at the `std` level: `sys::random`
     does `RDRAND`-or-TSC-seeded-fallback; see Current Foundation
   - richer path normalization beyond the compact RYMFS path limit
   - broader errno coverage across all ABI calls (the kernel's own `ERR_*`
     set is still small; `std`-level mapping of what exists today onto
     `io::ErrorKind` is done -- see Current Foundation)
   - wire `sys::fs`/`sys::env`/`sys::process`/`sys::time` onto the real ABI --
     done for `fs`/`env`/`time` (ordering only); `std::process::Command`
     deliberately still unsupported, a real design mismatch rather than a
     bounded wiring gap -- see Current Foundation

4. Memory:
   - page-table page reclaim for the shared heap/mmap windows -- done:
     `reclaim_process_window_tables` frees the PT (heap) or PD+PT (mmap)
     pages an exiting PID's window exclusively owned; see Current Foundation
   - guard pages and allocation failure behavior -- done: an IDT with
     handlers for all 32 CPU exception vectors turns a fault into a
     diagnostic and a halt instead of a silent reset; see Current Foundation
     for what's still open (fault recovery, IST/TSS for double faults, and
     the address-0 identity-mapping gap found along the way)
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
4. Cross-compile small CLI tools that use `std::fs` and `std::env`. Done for
   a hand-written manual test (`stdreal`, see Current Foundation): real
   `fs`/`env`/`args`/`cwd`/`time`-ordering/`random` all work. Still ahead:
   this was one manually-built-and-tested binary, not a repeatable pipeline
   -- `scripts/rymos-sdk.py`/`make programs` only build `no_std` programs
   against the stable-compatible fallback target today, not real `std` via
   the forked toolchain's `-Z build-std` path.
5. Run cargo-like helper programs that spawn children. Blocked on
   `std::process::Command`, which is a real design mismatch (RYMOS resolves
   programs by name through `bootfs`, not a resolved filesystem path) rather
   than a bounded wiring gap -- see Current Foundation.
6. Port `cargo` after process, pipes, directories, and env are reliable.
7. Port `rustc` last, after large files, memory, and child process behavior are
   boringly dependable.

## Near-Term Test Programs

- `fswalk`: covers nested directories, sparse writes, non-contiguous
  extents/fragmentation, long paths, and many concurrent file descriptors.
- Exit-status and stdin/stdout piping checks (originally sketched as
  separate `spawncheck`/`pipecheck` programs) ended up folded into existing
  ones instead: `cmdapi` verifies spawn/wait exit status end to end (plus
  parent-globals-survive-spawn and process-reaping regressions), and
  `echoin`/rysh's `spawnio`/`spawnioe` cover piped child stdin and
  stdout/stderr capture.
- `heapstress`: covers mmap/heap pressure and guarded-region access; still
  room to grow toward more allocation-failure-path checks.
- `stdshim`: `no_std` programs' std-shaped compatibility shim
  (`rymos-user::stdish`) -- still useful groundwork/parity checking even now
  that `stdreal` covers genuine `std` directly; the two test different
  layers, not the same thing twice.
- `stdreal`: a genuine `#![feature(restricted_std)]` binary (not routed
  through `rymos-user`) exercising real `std::fs`/`std::env`/`std::process`/
  `std::time`/`std::random`. Built manually via the forked-toolchain
  `-Z build-std` path (see `docs/dev-environment.md`), not through
  `scripts/rymos-sdk.py`, and deliberately left out of
  `rymos-packages.toml`/`autoexec.bat` for the same reason -- there's no
  build pipeline for real `std` programs yet, just this one hand-built test.
- `faultcheck`: manual CPU exception diagnostic (guard-page touches, raw
  divide-by-zero, invalid opcode) -- deliberately not part of the automated
  regression, since a passing run halts the machine on purpose.
