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

For the full fresh-machine setup and manual rebuild steps (including the
forked `rustc` toolchain under `toolchain/rust`), see
`docs/dev-environment.md`.

## Current Foundation

RYMOS now has the first serious self-hosting substrate, but not a native Rust
toolchain yet.

- ABI v21 runs trusted `no_std` ELF programs through `rymos-user`.
- RYMFS5 persists files up to 256 MiB with 256 compact metadata entries and
  96-byte names, nested directories, unlink/rename, append, create-new,
  seek, stat, and list, created/modified tick timestamps, a permission-style
  mode byte per entry, non-contiguous allocation (a file can spread across
  up to 4 extents instead of needing one giant contiguous run, so
  fragmentation from other files no longer causes spurious "disk full"
  errors), and sparse writes (seeking past the current end and writing
  there zero-fills the gap instead of leaving stale disk contents).
- Process services include PIDs, parent PID tracking, synchronous spawn,
  consuming `wait`/`wait_any`, inherited cwd/std fds, up to 32 open file
  descriptors per process (raised from 8), pipes, and stdout/stderr
  capture/redirection. Spawned children now run in their own isolated address
  space (still synchronous, not concurrent) instead of overwriting the
  parent's fixed-address program image. Process-table slot reuse now
  correctly requires a zombie to already be reaped (`waited`) before a new
  spawn can take its slot, instead of silently overwriting an uncollected
  child's exit status.
- Memory services include the UEFI memory map, physical page allocation,
  kernel-owned PML4, per-process heap windows, guarded mmap-like
  `mem_map_pages`/`mem_unmap_pages`, process-exit data-page reclaim, and (new)
  per-process private PML4/PDPT/PD structures for the fixed program-image
  window, reclaimed on exit.
- Runtime support includes `alloc::Vec`/`String`, argv reads, env get/set/remove,
  cwd/path handling, errno-style `last_error`, monotonic ticks, and a `stdish`
  shim for early fs/env/process/io/time/path/temp-dir/error work.
- Boot smoke programs prove the current surface: `cmdapi`, `fswalk`,
  `heapstress`, `stdshim`, `echoin`, `allocdemo`, and `rysh`.
- `targets/x86_64-rymos.json` parses on current nightly and
  `-Z build-std=core,alloc,compiler_builtins -Z json-target-spec` builds and
  links real programs for it, verified end to end by building and installing
  `hello` through `RYMOS_TARGET_MODE=custom`.
- `toolchain/rust` (a forked `rust-lang/rust` checkout, git submodule) builds
  a real `library/std` for `x86_64-rymos`, with a real ABI-wired `_start`
  entry and a real bump allocator over `mem_alloc_pages`. A genuine `std`
  binary (`#![feature(restricted_std)]`, `println!`, `Vec`, iterators) boots
  and runs correctly in QEMU today. Still stubbed: `fs`/`env`/`process`/time,
  and `random`/`io::error` (see What Is Left, category 5).

See `docs/rust-port-roadmap.md` for the detailed cargo/rustc port sequence.

## Recently Closed

- **Real process reaping** (was: a brand-new spawn could silently reuse *any*
  `Exited`/`Failed` process-table slot, even one nobody had called
  `wait`/`wait_any` on yet; now: slot reuse requires the zombie to already be
  reaped): a full process table used to let a fresh spawn quietly overwrite
  an unwaited child's exit status before its real parent ever collected it --
  `wait`/`wait_any` would then report "not found" instead of the child's
  actual result, or (once real blocking wait exists) could hang forever.
  `process_find_reapable_slot` now only offers up `Exited`/`Failed` slots
  with `waited == true`; a table full of genuinely-unreaped zombies now
  correctly fails a new spawn with `ERR_NOSPC` instead of corrupting one.
  Top-level console `run`'d processes have no ABI-level parent that will
  ever call `wait` on them, so they're marked pre-reaped at exit instead of
  becoming permanent zombies under the new rule. Verified live in QEMU: a
  cmdapi check spawns a child, deliberately does not wait on it immediately
  (so its slot must survive as an unreapable zombie), then waits on it and
  confirms its real exit status still comes back correctly.

  A genuinely deferred/queued spawn model (`spawn` only enqueues a child;
  `wait`/`wait_any` are what actually run pending children, letting several
  be queued before any of them execute) was also attempted for "add real
  concurrent execution" and reverted after a live QEMU hang exposed a
  real architectural conflict: several of rysh's own shell built-ins
  (`spawnredir`, `spawnstdin`, `spawnio`, `spawnioe`) and every `Command`
  helper in `rymos-user` redirect stdio/cwd/env onto shared ambient globals,
  spawn, and then either revert the redirection or read the result --
  several with *no* intervening `wait()` call at all -- relying entirely on
  `spawn` completing synchronously before returning. Deferring execution left
  a redirected pipe (`echoin`'s stdin in this case) disconnected from
  anything before the child ever ran, so it spun at 100% CPU waiting for
  input that would never arrive over a file-backed serial console. Fixing
  every affected caller across rysh and `rymos-user` would be a separate,
  large audit in its own right, not a bounded piece of this category --
  reverted rather than shipped half-fixed. Real concurrent (interleaved)
  execution still needs the ABI's flat globals (`APP_FDS`, `APP_CWD`,
  `APP_ENV`, heap/mmap pointers) to move into a real per-process control
  block, plus either a cooperative scheduler with saved per-process CPU
  context or full preemption -- unchanged by this attempt, and still the
  next, separate lift for category 2.

- **Directory ceiling, long paths, sparse writes, more file descriptors**
  (the rest of category 1 in What Is Left): entries raised 102 -> 256,
  names raised 30 -> 96 bytes, per-process file descriptors raised 8 -> 32,
  and sparse writes added. The entry/name bump wasn't just "pick bigger
  numbers": the on-disk header is read into a stack-local
  `[u8; PFS_HEADER_BYTES]` array in a few places (`read_header_silent`,
  `format`), and this kernel has no stack guard page, so an oversized bump
  would risk silent memory corruption rather than a clean crash. Chose
  values that keep the per-call copy in the tens of KiB and verified clean
  via a full QEMU boot regression, rather than guessing blindly.

  Sparse writes needed two fixes, not one: `pfs_write_at` now zero-fills
  the gap between the old logical end and a write's start offset when
  writing past it (`pfs_zero_range`, skipping the read for whole sectors),
  but that path was unreachable until a second bug was fixed --
  `abi_seek` unconditionally rejected any `offset > handle.len`, so seeking
  past EOF (legal POSIX `lseek` behavior) failed before a sparse write
  could ever be attempted. Still clamps BootFS handles, which are
  fixed-size read-only ROM data where seeking past the end is meaningless.

  Verified live in QEMU: `fswalk` writes 4 bytes, seeks to offset 20,
  writes 4 more, and reads back exactly `head` + 16 zero bytes + `tail`;
  opens 20 concurrent file descriptors (well past the old 8-fd limit); and
  round-trips a 76-byte path (well past the old 30-byte limit) -- all
  without disturbing the existing regression suite (cmdapi, heapstress,
  stdshim, the RYMFS5 fragmentation demo).

  Still open, and worth being honest about: 256 entries/96-byte names is a
  *raised fixed ceiling*, not true unbounded directory growth. That would
  mean moving the entry table itself onto growable disk extents (reusing
  the same extent machinery files already use), which also requires moving
  the header off the kernel's stack first -- a bigger, separate lift.

- **RYMFS non-contiguous extents** (was: one contiguous run per file,
  fragmentation from other files could cause spurious "disk full"; now:
  RYMFS4 -> RYMFS5, up to 4 extents per file): `pfs_find_free_run` scans
  every other entry's extents for gaps instead of requiring one run big
  enough for the whole file; `pfs_grow_extents` fills a shortfall by walking
  those gaps, splitting across multiple extents when needed -- and
  critically, coalesces a newly-found run into the file's own last extent
  when it continues exactly where that extent left off. Growing a file no
  longer needs to copy existing data to a new location either (the old
  design reallocated the whole file fresh on every capacity increase); it
  just appends (or extends) extents in place.

  A real bug surfaced while building this: without the coalescing step,
  ordinary sequential writes in small chunks (each one crossing another
  sector boundary) burned through all 4 extent slots on nothing but
  physically-adjacent sectors within the first couple KiB written -- every
  functional test still passed, because each individual write "succeeded,"
  but the file silently stopped growing past 4 sectors once the budget was
  exhausted. Caught by comparing requested vs. actual written byte counts,
  not by a crash.

  Verified live in QEMU by inspecting the persisted on-disk header
  directly, not just program-visible output (an empty 4 GiB disk makes
  "just place everything after the last file" trivially possible either
  way, so passing writes/reads alone wouldn't prove fragmentation-reuse
  happened): three files were written back-to-back, the middle one
  deleted, then a file too big for that single hole was written. The
  persisted entry showed exactly two extents -- one reusing the hole, one
  after the last remaining file.

- **Spawned children get their own isolated address space** (was: every
  process shared the same fixed-address program image, requiring a
  snapshot/reload-ELF dance around every spawn; now: each child gets private
  page tables for that window): `create_process_address_space` gives a
  process its own PML4 -- a shallow top-level clone of the kernel's own
  (sharing kernel code and every process's heap/mmap windows), plus
  selectively-privatized PDPT/PD entries just for the
  `APP_LOAD_MIN..APP_LOAD_MAX` image range. `load_program_elf_isolated` maps
  fresh physical pages for each `PT_LOAD` segment into those private tables
  instead of writing through the old shared window;
  `destroy_process_address_space` reclaims all of it (structural PT/PD/PDPT/
  PML4 pages plus any data pages) on exit. This let `spawn_prepared` drop its
  old parent-ELF-reload dance entirely.

  This fixed a real, previously-live bug: the old scheme only ever restored
  the parent's *read-only* segments after a child returned -- its writable
  `.data`/`.bss` was never saved and never restored, so any mutable global a
  parent held before calling `spawn` was silently corrupted the moment a
  child ran. Verified fixed live in QEMU: `cmdapi` sets a `static mut`
  marker, spawns four children, and confirms it afterward
  (`cmdapi: parent globals survive spawn ok`).

  Three real bugs surfaced and got fixed while building this:
  - `ensure_kernel_pml4()` used to force CR3 back to the shared kernel PML4
    on every call where CR3 didn't already match it -- including calls made
    from *inside* an already-running isolated child (e.g. its own
    `mem_alloc_pages`), which would have evicted the child's private address
    space from CR3 mid-execution. Fixed to only force CR3 on the very first
    initialization.
  - A shallow PML4 clone only captures top-level entries that exist *at
    clone time*. Heap/mmap top-level entries are created lazily on first
    use, so the first process ever to touch a given 512 GiB slice of that
    range would page-fault immediately on its first heap allocation, since
    its own already-cloned PML4 has no way to learn about an entry the
    kernel's PML4 gained afterward. Fixed by pre-touching a child's specific
    heap/mmap top-level entries (computable from its PID) before cloning.
  - Different `PT_LOAD` segments can legitimately share one page (e.g. a
    `PT_GNU_RELRO` slice sharing a page with the preceding rodata segment).
    The isolated loader's first pass treated an already-mapped page as a
    hard failure; `map_image_pages` now skips pages another segment already
    mapped instead of aborting.

  Deliberately not attempted here: real concurrency (still no scheduler, no
  saved per-process CPU context, no preemption -- isolation stops a child
  from corrupting its parent, it doesn't make them run at the same time) or
  relocatable/PIE loading (every process still uses the same virtual address
  for its image, just backed by different physical pages).

- **A real `std` program boots and runs on RYMOS** (was: compiles and links
  only, no working entry point; now: actually runs): added a real `_start`
  in `sys::pal::rymos::mod.rs` matching our ABI's
  `_start(abi: *const RymosAbi) -> i32` convention -- it stashes the incoming
  ABI pointer in a static (`sys::pal::rymos::abi`, an independent `#[repr(C)]`
  view of `RymosAbi` kept in sync by hand, same as the kernel/`rymos-user`
  copies already are) and calls the compiler-generated `main(argc, argv)`
  symbol from `#[lang = "start"]`, the same pattern Hermit's
  `sys::pal::hermit::runtime_entry` uses. Also replaced the inert
  `sys::alloc::rymos` stub with a real bump allocator over the ABI's
  `mem_alloc_pages` (mirrors `rymos-user`'s `BumpAllocator` exactly), and
  wired `sys::stdio::rymos::Stdout::write` to the ABI's console `write` call.
  Verified live in QEMU: a real binary using `println!`, integer formatting,
  `Vec<i32>` collection, and `.iter().sum()` printed correct output and
  exited cleanly with heap reclaimed.

  Two real bugs found and fixed along the way, both worth remembering:
  - **The crash wasn't a bug in the new code -- it was the missing
    allocator.** `_start` → `lang_start` → empty `main()` worked immediately.
    Adding one `println!()` crashed with a CPU `#UD` (invalid opcode) inside
    `sys::pal::rymos::abort_internal`, with no panic message printed at all.
    Root cause: `Stdout`'s reentrant lock allocates a `Thread` handle
    (`Arc`) on first use; our allocator returned null unconditionally;
    `handle_alloc_error` aborted before anything could be printed (our
    `panic_output()` stub also discards messages, so a real panic there
    would look identical). Bisecting with an empty `main()` first, before
    reading any further disassembly, found this in two boot cycles instead
    of a long dead-end down the panic/threading internals.
  - **A pre-existing bootloader limit, unrelated to this work, that this
    testing happened to trip**: `INITRD_READ_LIMIT` was hardcoded to 4 MiB
    with no size check before the UEFI `File.Read()` call, so it silently
    truncated -- no error, just missing files -- once a debug (unstripped)
    `std` binary (~7 MB total initrd) pushed past it. Raised to 32 MiB.
    Real std binaries are dramatically larger than `no_std` ones (single
    digits of MiB unstripped, several hundred KiB stripped/release), so this
    ceiling will keep mattering.

- **Real `std` compiles and links for `x86_64-rymos`** (was: confirmed
  infeasible without forking rustc; now: forked and done for the
  compiles-and-links bar): `toolchain/rust` is a `rust-lang/rust` submodule,
  built locally with `./x.py build library/std --stage 1`
  (`download-ci-llvm` skips building LLVM; ~3.5 minutes total) and linked via
  `rustup toolchain link rymos-fork toolchain/rust/build/host/stage1`. Added
  `sys::pal::rymos` (copy of `sys::pal::unsupported`); reused the existing
  `no_threads` TLS/sync fallback, `random::unsupported`, and
  `io::error::generic`; set `"singlethread": true` in the target spec to
  match; added a `check-cfg` entry for the new `target_os` value.
- **Richer RYMFS metadata** (was open, now done): RYMFS3 -> RYMFS4 bumped the
  on-disk entry format (40 -> 57 bytes, header 8 -> 12 sectors) to add
  `created`/`modified` tick timestamps and a read/write/exec mode byte per
  entry. ABI bumped 20 -> 21; `stat`/`list` expose the new fields. Verified
  live in QEMU: `fswalk` mkdir/write/append/rename/chdir/stat/list all pass
  against the new format and print real timestamps/mode.
- **`core`/`alloc`/`compiler_builtins` for the custom target** (was
  "planned"/blocked on a broken target spec, now done): fixed
  `target-pointer-width`/`target-c-int-width` to integers and added
  `rustc-abi`/`max-atomic-width` for current nightly's stricter target-spec
  schema. `RYMOS_TARGET_MODE=custom` builds real ELF programs via
  `build-std`.

## What Is Left

The remaining blockers are now narrower and more concrete:

1. Filesystem:
   - add non-contiguous extents -- done: RYMFS4 -> RYMFS5, up to 4 extents
     per file with coalescing growth; see Recently Closed
   - strengthen long path support -- done: names raised 30 -> 96 bytes
   - support sparse writes -- done: seek-past-EOF-then-write zero-fills the
     gap; see Recently Closed
   - more file descriptors -- done: 8 -> 32 per process; see Recently Closed
   - true unbounded directory growth (256 entries is a raised fixed
     ceiling, not the entry table living on its own growable disk extents --
     see Recently Closed for why that's a bigger, separate lift)

2. Process model:
   - remove the fixed-address restore model -- done: see Recently Closed;
     children now run in their own isolated address space instead
   - add true process reaping -- done: a zombie's process-table slot can no
     longer be silently reused before its real parent collects its exit
     status; see Recently Closed
   - add real concurrent execution: a scheduler, saved per-process CPU
     register/stack context, and preemption (today's isolation only means a
     child can't corrupt its parent's memory, not that they run at once).
     A deferred/queued (cooperative, non-preemptive) version of this was
     attempted and reverted -- see Recently Closed for the real conflict it
     found with rysh's shell built-ins and the `Command` helpers. Still
     needs the ABI's flat globals moved into a real per-process control
     block before any form of this (cooperative or preemptive) is safe.
   - add relocatable/PIE program loading (unrelated to the isolation work
     above, which keeps every process at the same virtual address)
   - add real `exec`
   - support blocking wait once concurrency exists (moot under today's
     synchronous spawn -- nothing is ever pending when `wait` is called; only
     meaningful once real concurrent execution exists)

3. Memory:
   - reclaim page-table pages for the shared heap/mmap windows -- partially
     done: the new private per-process image-window PT/PD/PDPT pages are
     correctly reclaimed on exit, but heap/mmap's own PT pages (which live
     in the shared kernel PML4) still aren't
   - add stronger allocation failure tests and guard-page fault handling
   - support larger process address spaces
   - give programs isolated address spaces -- done for the program-image
     window: see Recently Closed

4. Time, sync, and OS services:
   - calibrate ticks into real wall/monotonic time units
   - add sleep/timer behavior
   - add terminal/TTY behavior beyond the current console stream
   - (synchronization primitives for `std` turned out not to need new OS
     work: RYMOS is single-threaded per process today, so reusing `std`'s
     existing `no_threads` fallback is the *correct* answer, not a stub --
     revisit only if/when real concurrent threads within a process arrive)

5. Rust target and libraries:
   - map `std::fs`, `std::env`, `std::process`, and time onto the ABI (today
     these are still unwired -- a `std` binary can print and allocate, but
     opening a file, reading an env var, or spawning a child would either not
     compile against our patched `sys` modules yet or hit an inert stub)
   - real randomness and real errno mapping (`sys::random`/`sys::io::error`
     still reuse the generic `unsupported`/`generic` fallbacks: `fill_bytes`
     panics, IO errors are always reported as "operation successful")
   - real thread support if it's ever needed (today `no_threads` is correct
     for RYMOS's single-threaded-per-process reality, but note this in case
     that reality changes -- see Process model, item 2)
   - cross-compile small `std` CLI programs that actually touch `fs`/`env`
     before attempting cargo

6. Cargo and rustc:
   - run cargo-like helper programs first
   - port cargo after filesystem/process/env/path behavior is boringly reliable
   - port rustc last, after large files, lots of memory, process spawning, and
     `std` behavior are dependable

## How Far To Cargo And Rustc

RYMOS is early but on the right route. Roughly:

- `core`/`compiler_builtins`: done. `targets/x86_64-rymos.json` now parses on
  current nightly and `-Z build-std=core,alloc,compiler_builtins -Z
  json-target-spec` builds and links real programs (verified with `hello` via
  `RYMOS_TARGET_MODE=custom`).
- `alloc`: working for smoke programs through kernel-mapped heap pages, and now
  also builds through nightly `build-std` for the custom target.
- small `no_std` Rust programs: working today.
- small std-shaped Rust programs: started through `rymos-user::stdish`.
- small real `std` Rust programs: working today. `std` compiles, links, and
  *runs* for `x86_64-rymos` via a forked `rust-lang/rust` (`toolchain/rust`),
  the same way other small OSes (Redox, Hermit, ...) got their own PAL
  support -- verified live in QEMU with `println!`, `Vec`, and iterators.
  What's left is breadth, not "can this work at all": `fs`/`env`/`process`/
  time are still unwired stubs.
- `cargo`: still far; needs stronger process, filesystem, env, time, and path
  behavior, plus `std::fs`/`std::env`/`std::process` actually wired (not just
  stdio/alloc as today).
- `rustc`: very far; needs all of `cargo`'s foundations plus much stronger
  memory management, large files, many descriptors, and reliable child process
  execution.

That is why milestone 8 is a readiness milestone: it prevents us from losing
the map while we build the real road.
