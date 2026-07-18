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
  window, reclaimed on exit. Heap/mmap page-table page reclaim on process
  exit is done too (previously only the data pages were freed, not the
  tables that pointed to them). The kernel also has its first CPU exception
  handling: an IDT covering all 32 exception vectors turns a fault (a
  guard-page touch, a bad pointer, a divide-by-zero) into a clear serial
  diagnostic and a clean halt instead of a silent QEMU reset.
- Runtime support includes `alloc::Vec`/`String`, argv reads, env get/set/remove,
  cwd/path handling, errno-style `last_error`, monotonic ticks, and a `stdish`
  shim for early fs/env/process/io/time/path/temp-dir/error work.
- Boot smoke programs prove the current surface: `cmdapi`, `fswalk`,
  `heapstress`, `stdshim`, `echoin`, `allocdemo`, `rysh`, and (a genuine `std`
  binary, not a `no_std` one) `stdreal`.
- `targets/x86_64-rymos.json` parses on current nightly and
  `-Z build-std=core,alloc,compiler_builtins -Z json-target-spec` builds and
  links real programs for it, verified end to end by building and installing
  `hello` through `RYMOS_TARGET_MODE=custom`.
- `toolchain/rust` (a forked `rust-lang/rust` checkout, git submodule) builds
  a real `library/std` for `x86_64-rymos`, with a real ABI-wired `_start`
  entry and a real bump allocator over `mem_alloc_pages`. Real `std::fs`,
  `std::env` (including argv, cwd, and `temp_dir`), `std::time::Instant`
  (ordering only, not duration arithmetic), and `std::random`-backed
  `HashMap` support now round-trip against the ABI too -- not just
  stdio/alloc. `std::process::Command` (spawning) stays unsupported on
  purpose: it's a real design mismatch with how RYMOS resolves programs, not
  a bounded wiring task (see Recently Closed). Still stubbed: real errno
  detail beyond the basic `ERR_*` set already mapped, and `Instant`'s
  duration arithmetic (needs category 4's tick calibration first).

See `docs/rust-port-roadmap.md` for the detailed cargo/rustc port sequence.

## Recently Closed

- **Real `std::fs`/`std::env`/`std::time`/`std::random` wired onto the ABI**
  (category 5): before this, only `stdio` and the allocator were real in the
  forked `toolchain/rust`'s `sys::pal::rymos` -- `sys::fs`, `sys::env`,
  `sys::process`, `sys::time`, `sys::random`, and `sys::io::error` were all
  inert stubs (panicking or silently claiming success). The underlying
  kernel ABI already had everything needed (`open`/`read`/`write_fd`/`seek`/
  `close`/`stat`/`list`/`mkdir`/`unlink`/`rename`, `env_get`/`env_list`/
  `env_set`/`env_remove`, `cwd`/`chdir`, `argv_count`/`argv_get`,
  `time_ticks`, `last_error`) -- this was entirely about extending
  `sys::pal::rymos::abi`'s previously-truncated `RymosAbi` struct (it only
  had real signatures through `mem_alloc_pages`, the last field the pal
  actually called; everything after it was an untyped placeholder) with
  every real field, then implementing each `sys` module against it:
  - `sys::env`: real `getenv`/`setenv`/`unsetenv`/`env()`. No local cache
    needed (unlike e.g. Hermit's `HashMap`-backed one) -- the kernel is
    already the single source of truth for a process's environment, and
    it's also what a spawned child inherits from, so every call just
    round-trips to it directly.
  - `sys::args`: real `args()` via `argv_count`/`argv_get`, queried live on
    every call rather than cached at start-up like Unix's argc/argv --
    there's no ready-made argv array to hand off in the first place, and
    unlike env vars, argv never changes after a process starts, so there's
    nothing to keep in sync by caching it.
  - `sys::paths`: real `getcwd`/`chdir`/`temp_dir` (the last one reading the
    `TMPDIR` env var the kernel already seeds by default).
  - `sys::fs`: a real `File` (open/read/write/seek/close) plus
    `stat`/`readdir`/`mkdir`/`unlink`/`rename`-backed directory operations.
    RYMOS has no symlinks, hard links, file locking, or settable permission
    bits beyond a read/write/exec mode byte, so those honestly stay
    `unsupported()` rather than faking behavior that doesn't exist.
    `remove_dir_all` and `Dir` are reused from `sys::fs::common` for free,
    since they're already generic over exactly the primitives just built;
    `copy` is hand-written rather than reusing `common::copy`, because that
    generic version needs `File::file_attr` (fd-based `fstat`), which RYMOS
    has no equivalent of -- the ABI's `stat` is path-based only.
  - `sys::io::error`: real `ERR_*`-to-`ErrorKind` mapping via the ABI's
    `last_error`, replacing the old `generic` fallback that reported every
    error as "operation successful" regardless of what actually happened.
  - `sys::random`: `RDRAND` when `CPUID` reports the CPU supports it,
    falling back to a `SplitMix64` stream seeded from `time_ticks` plus a
    couple of ASLR-ish stack/heap addresses when it doesn't -- disclosed as
    *not* cryptographically secure (a determined attacker who can measure or
    influence boot timing could plausibly predict it), but real, changing
    output instead of the previous panic.
  - `sys::time`: `Instant` is real for ordering/equality (backed by
    `time_ticks`, a raw `rdtsc` read), but duration arithmetic
    (`checked_sub_instant` and friends, which `.elapsed()` needs) honestly
    returns `None` rather than fabricating a conversion factor from raw TSC
    ticks to real time units -- there is no calibration reference anywhere
    in the kernel yet (no PIT/timer, no CPUID TSC-frequency detection), and
    category 4 already lists "calibrate ticks into real wall/monotonic time
    units" as separate, not-yet-done work. Treating 1 tick as 1 nanosecond
    would silently produce wildly wrong (not just imprecise) numbers; an
    honest `None` was judged better than a plausible-looking lie, consistent
    with why `sys::random`'s old panic was preferred over fake output before
    this pass replaced it with something real.
  - `sys::process`: deliberately left `Command`/spawning unsupported -- a
    real architectural mismatch (RYMOS resolves programs by name through
    `bootfs`, not a resolved filesystem path the way Unix's fork/exec model
    assumes; `Stdio`/pipe wiring at the `std` layer would also need
    `sys::pipe` support this port has never touched), not a bounded
    extension of wiring an already-designed ABI onto `std` the way the
    others were. `std::process::id()` is real, though, since every RYMOS
    program already knows its own pid via the ABI's `pid` call.

  Verified live in QEMU with a new `stdreal` program -- a genuine
  `#![feature(restricted_std)]` binary (not routed through `rymos-user`,
  unlike every other test program) exercising `std::process::id()`, real
  argv, env get/set/iterate/remove, `current_dir`/`temp_dir`,
  `fs::write`/`read_to_string`/`exists`/`read_dir`, `Instant` ordering, and a
  `HashMap` (exercising the new random support internally) -- all correct,
  run alongside the full existing `no_std` regression suite without
  disturbing it.

  Two real bugs found and fixed along the way:
  - A genuine, live crash the first time `stdreal` ran: `std::process::id()`
    aborted via `ud2` instead of returning a pid. The disassembly showed it
    was calling `sys::process::unsupported::getpid` (which panics) instead
    of the real one just added -- a stale `-Z build-std` target cache had
    kept using the pre-edit sysroot build despite the source changing.
    `docs/dev-environment.md` already documented this exact gotcha
    (`rm -rf target/x86_64-rymos` before re-testing); this is a live
    instance of hitting it, not a new class of bug.
  - `sys::pal::rymos::abi`'s `RymosAbi` struct previously only had real
    field types through `mem_alloc_pages` (by design -- nothing past it was
    called yet, and the doc comment said so explicitly). Extending it
    required re-deriving every remaining field's exact C ABI signature from
    `runtime/rymos-user/src/lib.rs`'s copy (the two must match by hand, same
    as the kernel's own copy already has to match `rymos-user`'s) --
    including packed return-value conventions like `env_list`'s
    `(key_len << 32) | value_len`, cross-checked directly against
    `kernel/src/main.rs`'s `abi_env_list` rather than assumed.

- **Heap/mmap page-table reclaim, and first CPU exception handling** (category
  3, memory): `process_reclaim_mappings` only ever freed the tracked *data*
  pages a process's heap/mmap allocations used, never the PT/PD page-table
  pages that pointed to them -- those leaked permanently on every process
  exit. Fixed by `reclaim_process_window_tables`, which frees them safely
  without needing a private-PML4-style walk: because PIDs are never reused
  and each PID's heap (256 MiB) / mmap (1 GiB) window sits at a fixed,
  alignment-guaranteed address that's purely a function of its own PID, a
  heap window always owns a clean, non-overlapping run of 128 PT-pointing PD
  entries (never a whole PD, since 4 different PIDs share the rest of it),
  and a mmap window always owns one whole, exclusively-owned PD (so the PD
  itself can be freed too, not just its PTs). Verified live in QEMU: three
  consecutive spawn/exit cycles of the same program leave the physical
  allocator's used-page count exactly flat rather than growing per cycle.

  Separately, this kernel had no IDT at all before this: any fault (a
  guard-page touch, a null-pointer access, a divide-by-zero, an invalid
  opcode) triple-faulted the whole machine with zero diagnostics -- QEMU
  just silently reset, which is exactly the failure mode "add stronger
  allocation failure tests and guard-page fault handling" was asking to
  close. Added an IDT covering all 32 CPU exception vectors, each backed by
  a small hand-written naked-function stub rather than the `x86-interrupt`
  calling convention (still nightly-only; this kernel builds on stable) --
  every stub normalizes the stack to a consistent layout (pushing a filler
  error code for the vectors that don't get a real one from the CPU) and
  jumps to one shared handler that prints a clear diagnostic over serial
  (vector, mnemonic, error code, `CR2` for page faults, faulting RIP, and
  the current PID/process name if any) and halts, instead of resetting with
  no explanation. No new GDT was needed either: it reads whichever code
  segment selector UEFI's firmware already set up at IDT-init time rather
  than assuming a fixed value. Verified live in QEMU via a new `faultcheck`
  program across a leading guard-page touch, a trailing guard-page touch, a
  raw hardware divide-by-zero (issued via inline asm -- Rust's own `/`
  always checks for a zero divisor and panics before ever reaching hardware,
  so exercising the real `#DE` exception path needs bypassing that
  deliberately), and `ud2` (invalid opcode) -- each produced the expected
  diagnostic and a clean halt. `faultcheck` is deliberately *not* wired into
  `autoexec.bat`: a passing run halts the machine on purpose, which would
  stop the rest of the automated boot regression from ever running.

  Found and disclosed along the way, not fixed: a null-pointer write does
  *not* fault today, because address 0 turned out to already be mapped as
  part of RYMOS's low-memory identity mapping rather than being guarded --
  a real, separate memory-layout gap, left for future hardening rather than
  quietly worked around.

  Deliberately not attempted: recovering from a fault (killing just the
  offending process while the rest of the OS keeps running, instead of
  halting everything). That needs a prepared non-local-exit point to unwind
  out of the arbitrary nested Rust call stack a fault can land in, which is
  real, separate work -- every handler here only ever diagnoses and halts.
  Also not attempted: a dedicated IST/TSS stack for the double-fault
  handler, so a double fault caused by genuine kernel stack exhaustion could
  still (rarely) overrun into an actual triple fault if the handler itself
  has no stack room left -- every other fault still gets a clean diagnostic
  regardless.

  Assessed, not changed: whether per-process address spaces need to be
  larger. They don't right now -- the per-process heap (256 MiB) and mmap
  (768 MiB) windows are already far bigger than what fits in a 256 MiB QEMU
  test VM once the fixed 142 MiB program-image reservation is subtracted;
  the real ceiling today is available physical RAM and that reservation,
  not the virtual window sizes, so raising the latter wouldn't currently
  unlock anything.

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
   - reclaim page-table pages for the shared heap/mmap windows -- done: see
     Recently Closed
   - add stronger allocation failure tests and guard-page fault handling --
     done: an IDT with handlers for all 32 CPU exception vectors turns a
     fault into a clear diagnostic and a halt instead of a silent reset; see
     Recently Closed for what's still open (recovery, IST/TSS for double
     faults, and the address-0 identity-mapping gap `faultcheck` found)
   - support larger process address spaces -- assessed, not needed yet: see
     Recently Closed for why the real ceiling today is physical RAM and the
     fixed image-window reservation, not the virtual window sizes
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
   - map `std::fs` and `std::env` (including argv, cwd, `temp_dir`) onto the
     ABI -- done: see Recently Closed
   - real randomness and real errno mapping -- done: `sys::random` does
     `RDRAND`-or-TSC-seeded-fallback, `sys::io::error` maps the ABI's `ERR_*`
     codes to real `ErrorKind`s; see Recently Closed. Still narrow: only the
     handful of error codes the ABI actually defines are mapped, not a broad
     errno taxonomy
   - `std::time::Instant` -- done for ordering/equality; duration arithmetic
     (`.elapsed()` and friends) still honestly unsupported, gated on
     category 4's tick calibration (no PIT/timer or CPUID TSC-frequency
     detection exists yet to convert raw `rdtsc` ticks into real time units)
   - `std::process::Command` (spawning) -- deliberately still unsupported:
     a real design mismatch (RYMOS resolves programs by name through
     `bootfs`, not a resolved filesystem path; `Stdio`/pipe wiring would
     need `sys::pipe` support too), not a bounded wiring task like the
     others were; see Recently Closed. `std::process::id()` is done.
   - real thread support if it's ever needed (today `no_threads` is correct
     for RYMOS's single-threaded-per-process reality, but note this in case
     that reality changes -- see Process model, item 2)
   - cross-compile small `std` CLI programs that actually touch `fs`/`env` --
     done: see `stdreal` in Recently Closed. Still ahead: this was one
     hand-written manual test, not a build pipeline -- integrating real
     `std` builds into `scripts/rymos-sdk.py`/`make programs` (today's SDK
     only builds `no_std` programs against the stable-compatible fallback
     target) is still its own separate lift before attempting cargo

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
- small real `std` Rust programs: working today, and now with real breadth,
  not just "can this work at all": `std` compiles, links, and *runs* for
  `x86_64-rymos` via a forked `rust-lang/rust` (`toolchain/rust`), the same
  way other small OSes (Redox, Hermit, ...) got their own PAL support, and
  `fs`/`env`/`args`/`cwd`/`temp_dir`/`Instant`-ordering/`random` are real
  now too (see Recently Closed) -- verified live in QEMU with a `stdreal`
  program touching all of them plus `println!`/`Vec`/`HashMap`/iterators.
  What's left: `std::process::Command` (a real design mismatch, not a
  wiring gap), `Instant` duration arithmetic (needs tick calibration,
  category 4), and turning the one hand-built `stdreal` test into a
  repeatable build pipeline.
- `cargo`: still far; needs `std::process::Command` (currently a deliberate,
  documented gap -- see Recently Closed) plus the stronger process,
  filesystem, env, time, and path behavior category 2-4 still have open.
- `rustc`: very far; needs all of `cargo`'s foundations plus much stronger
  memory management, large files, many descriptors, and reliable child process
  execution.

That is why milestone 8 is a readiness milestone: it prevents us from losing
the map while we build the real road.
