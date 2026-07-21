# RYMOS Cargo And Rustc Port Roadmap

RYMOS can run Rust-built `no_std` ELF programs today. The goal of this roadmap
is to turn that into enough OS surface for `cargo` helpers and eventually
`rustc`.

For the exact commands to set up a machine and rebuild everything by hand
(including the forked `rustc` toolchain this roadmap depends on), see
`docs/dev-environment.md`.

## Current Foundation

- ABI v22 program entry through `rymos-user`.
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

- **Real `std::process::Command`, and a real build pipeline for `stdreal`**
  (category 6 -- the last major stretch before self-hosting becomes
  reachable): `Command` was left deliberately unsupported after category 5,
  flagged as "a real design mismatch, not a bounded wiring task."
  Revisiting it found the mismatch narrower than first assessed: RYMOS's
  `spawn`/`spawn_argv` run the child to completion *inline* before
  returning (no fork+exec, no real concurrency -- category 2), and
  `rymos-user`'s own `Command` (used successfully throughout this project)
  already proved "snapshot the ambient stdio/cwd/env state, mutate it,
  spawn synchronously, restore it" is correct for that model. Porting the
  same pattern into `std`'s `sys::process` -- rather than inventing
  something new -- turned out bounded after all: `sys::pipe::rymos` wraps
  the ABI's `pipe`/`read`/`write_fd`/`close` (real in-memory pipes, not a
  stub); `sys::process::rymos` implements `Command`/`Stdio`/`Process`/
  `ExitStatus`, dup2'ing the child's stdio onto the ambient std fds before
  spawning and restoring the original std fds/cwd/env afterward regardless
  of outcome. `Stdio::null()` has no real `/dev/null`, so it uses a scratch
  pipe whose unused end is dropped immediately -- reading an empty pipe
  with no writer returns `0` (EOF) right away rather than blocking (see
  `kernel/src/main.rs`'s `abi_read`), so this behaves like a real null
  device both directions without needing a dedicated null-device concept.
  Because the child always finishes before `spawn` returns, `Process` just
  stores the already-known exit code -- `wait`/`try_wait` are lookups, not
  real waits, matching how `abi::wait` already worked.

  The one real, disclosed limitation: `Command::spawn()` followed by
  writing to `Child::stdin` does *not* behave like a real OS -- the child
  has already run (and already read whatever was in its stdin pipe, or read
  nothing) by the time `spawn()` returns, so anything written afterward
  never reaches it. Doesn't affect `output()`/`.status()` (what cargo/rustc
  invocation actually uses, since neither feeds stdin data by default) --
  only the less-common pattern of spawning then interactively writing to a
  child's stdin. Real concurrent execution now exists (category 2's
  scheduler work, see Current Foundation) -- `spawn_argv` itself genuinely
  enqueues and returns without running the child -- but `sys::process::rymos`'s
  `spawn_now` still deliberately bundles an `abi::wait` right after
  `spawn_argv`, preserving `Command::spawn()`'s old synchronous-completion
  semantics rather than exposing the new async spawn directly. Exposing a
  real interactive `Child::stdin` would mean *not* bundling that wait --
  a deliberate, still-open follow-up, not more ABI wiring.

  Verified live in QEMU: `stdreal`'s `Command::output()` captured a real
  child's exit code (`0`) and 326 bytes of its actual stdout; `.status()`
  with an `env()` override succeeded, confirmed not to leak into the
  parent's own environment afterward -- both against the full existing
  regression suite, undisturbed.

  Separately, `stdreal` went from a hand-run sequence of manual commands to
  a real, repeatable pipeline: `RYMOS_TARGET_MODE=std python3
  scripts/rymos-sdk.py install stdreal` now builds it via the `rymos-fork`
  toolchain's `-Z build-std` path end to end, automating three things that
  were previously easy to get wrong by hand (and did, earlier this
  session): always clearing the stale `-Z build-std` target cache first
  (the exact gotcha that produced a real, confirmed false crash in category
  5); recreating the `rust-lld` symlink the stage1 sysroot loses on every
  toolchain rebuild; and stripping debug symbols afterward (a `--release`
  real-`std` binary is still ~1.2 MB unstripped vs ~100 KB stripped).
  Deliberately kept out of `rymos-packages.toml`/`install-all`'s default
  flow -- `std` mode is never auto-selected, and only `stdreal` opts into
  it today.

- **Calibrated time, real sleep, wall clock, and terminal size** (category
  4): `time_ticks` used to be a raw `rdtsc` read with no defined
  relationship to real time -- fine for ordering, meaningless for duration
  math (exactly what left `std::time::Instant`'s duration arithmetic
  honestly unsupported in category 5, just above). `calibrate_tsc` measures
  `rdtsc` ticks per second once at boot by polling the legacy PIT's channel
  2 (arm a known countdown, `rdtsc` before/after, see how many ticks
  elapsed over a known real-time interval) -- entirely by polling, no timer
  interrupt needed, consistent with this kernel deliberately having none
  beyond category 3's CPU exception handlers. Averaged across 4 rounds
  (the PIT's 16-bit counter caps one run at ~55 ms) to reduce jitter from a
  single short sample. ABI v22's `time_ticks` now returns real calibrated
  nanoseconds since boot.

  Real wall-clock time exists too: `time_unix_nanos` reads the CMOS RTC
  once at boot (BCD-or-binary, 12-or-24-hour, the classic
  wait-for-UIP-then-double-read technique against a torn read), paired with
  the calibration's boot `rdtsc` snapshot so later reads are cheap. Days
  since epoch via Howard Hinnant's `days_from_civil` algorithm rather than a
  hand-rolled month-length table (easy to get subtly wrong around leap
  years). The CMOS year register is only two digits with no standardized
  century register, so this assumes the 21st century -- disclosed, fine for
  a log timestamp, not an NTP substitute.

  `sleep_nanos` busy-waits against the calibrated clock -- correct, not a
  placeholder, since RYMOS has no scheduler to hand the CPU to during a
  sleep regardless. `term_size` reports the console's real row/column count.

  All four wired end to end: kernel ABI (v22), `rymos-user`, and
  `toolchain/rust`'s `std` -- `sys::time::rymos`'s `Instant` duration
  arithmetic and `SystemTime` are both genuinely real now (previously
  `Instant`'s duration math was honestly `None` and `SystemTime` reused the
  panicking `unsupported` stub specifically because no calibration existed
  yet). `std::thread::sleep` is real too: sleeping the *current* thread
  needs no multi-threading support, so reusing `sys::thread::unsupported`'s
  `Thread`/`available_parallelism`/etc. (genuinely correct -- RYMOS stays
  single-threaded per process) while overriding just `sleep` was the right
  split, not all-or-nothing.

  Verified live in QEMU: `stdshim` sleeps 10 ms and confirms elapsed ticks
  advanced by at least that much, plus a wall-clock print showing a real,
  plausible timestamp; `stdreal` sleeps 15 ms via `std::thread::sleep` and
  measures ~15.14 ms via real `Instant::checked_duration_since`;
  `SystemTime::now().duration_since(UNIX_EPOCH)` reported 20,652 days since
  epoch -- independently checked, that's mid-2026, matching the actual
  date, not just "a number that didn't crash."

  Assessed, scoped down deliberately: "terminal/TTY behavior beyond the
  current console stream" was the vaguest of the three original items, with
  no concrete downstream consumer identified (unlike tick calibration,
  which category 5 had already surfaced a real, documented need for). Did
  the cheap, clearly-real, bounded piece (terminal size) rather than
  speculatively building raw/cooked mode switching or ANSI/VT100 escape
  handling with no consumer in sight.

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

  That precondition is now done, in a later session: all per-process ABI
  state (18 flat `static mut APP_*` globals) moved onto `Process` itself --
  a real control block, addressed through one `CURRENT_PROCESS_INDEX`
  global -- eliminating the `AppStateSnapshot`/`app_snapshot`/`app_restore`/
  `app_restore_after_spawn` save-and-restore-around-spawn machinery
  entirely (the same machinery a nested-spawn stress test upstream had
  already found and fixed a real stack-corruption bug in). `APP_CONSOLE`/
  `APP_BOOTFS` stayed flat (genuinely process-independent -- one physical
  console/boot image for the whole kernel) and `APP_FDS`/`APP_PIPES` stayed
  a shared table deliberately (already a de facto shared resource under
  today's one-task-at-a-time model; real per-process fd/pipe isolation
  with duplication-on-spawn semantics is a separate, larger lift, not
  attempted here). Deliberately mechanical and behavior-preserving --
  spawn is still fully synchronous, no scheduler yet. Verified live in
  QEMU: the full existing regression suite, including `cargolike`'s
  nested-spawn test and a 3-level `relay` chain, produced byte-identical
  output before and after. A scheduler with saved per-process CPU context,
  per-process kernel stacks, and eventually preemption is still ahead --
  paused deliberately before starting that part, given its size and risk
  (hand-written context-switch assembly, and later the first interrupt
  handler in this kernel that must resume rather than diagnose-and-halt),
  rather than pushed through in one sitting.

  That scheduler is now done too, in a later session still: `spawn`
  genuinely enqueues a child and returns immediately rather than running it
  inline. A hand-written `context_switch` (naked asm, SysV callee-saved
  registers + `rsp` only -- a fiber-style save, not a full interrupt frame,
  since it only ever fires at a deliberate call site) plus per-pid kernel
  stacks (a new address window, same pre-touched-PML4 pattern as
  heap/mmap, but with a guard gap left unmapped since a stack has no
  software bump-pointer check the way heap/mmap do) make `wait`/`wait_any`
  real blocking calls: a caller whose target hasn't exited marks itself
  `Blocked` and yields into the scheduler, reconsidered only once that pid
  actually exits. This is the same deferred-spawn idea reverted above, but
  this time the callers it broke were actually fixed instead of worked
  around: rysh's `spawnredir`/`spawnstdin`/`spawnio`/`spawnioe` and
  `rymos-user`'s `run_command_output`/`run_command_status` now `wait()` the
  child before reverting stdio redirection, not after. Verified live in
  QEMU by replaying the exact `echoin`-via-rysh hang from the reverted
  attempt (now completes cleanly with real captured pipe data) plus the
  full regression suite unchanged. Still cooperative, not preemptive --
  yields only happen at `wait`/`wait_any` and process exit, so this alone
  doesn't let two daemons interleave; that needs a timer interrupt actually
  driving the reschedule decision, still ahead. `sys::process::rymos`'s
  `spawn_now` deliberately keeps bundling a `wait` right after `spawn_argv`
  regardless, to preserve `Command::spawn()`'s old synchronous-completion
  semantics rather than exposing the new async spawn directly to `std`.

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

- **`cargolike`/`bigoutput`: a first cargo-shaped smoke test, run live to
  find out what `stdreal` hadn't exercised** (see
  `docs/self-hosting.md`'s Recently Closed for the full writeup). Found and
  fixed three real gaps: a 1 KiB `APP_PIPE_BUFFER_SIZE` that silently
  truncated `Command::output()`'s captured stdout once it exceeded that
  (raised to 8 KiB); an 8-slot `APP_ENV_COUNT` (only 2 free once the
  kernel's own 6 default vars are counted) that didn't just fail a
  `set_var` call once exhausted but panicked, and since this target builds
  `std` with `panic_abort`, aborted the whole process (caught cleanly by
  category 3's exception handler, but a hard stop all the same) -- raised
  to 64 slots, and re-verified that the raised ceiling still fails the same
  safe way once genuinely exhausted; and `FileAttr::modified()`/`created()`,
  stubbed `unsupported()` even though `Stat`'s `created_ticks`/
  `modified_ticks` already existed -- the on-disk PFS values turned out to
  be raw `rdtsc` cycles rather than the calibrated nanoseconds-since-boot
  unit `time_ticks` exposes (userspace has no way to convert raw cycles back
  to real time without kernel-internal `TSC_HZ`/`BOOT_TSC`), so the kernel
  now stores `ns_since_boot()` there instead, and `FileAttr::modified()`/
  `created()` convert it to a real `SystemTime` via
  `time_unix_nanos() - time_ticks()` as the boot offset. `accessed()` stays
  `unsupported()` -- `Stat` has no real access-time field to report.
  Confirmed *not* an issue: 8 sequential `Command::output()` spawns in a row
  don't leak process-table slots.

- **`relay`: a nested-spawn stress test, run live to find out what
  `cargolike`'s flat/sequential tests couldn't see -- found and fixed three
  real, layered bugs** (see `docs/self-hosting.md`'s Recently Closed for the
  full writeup). A real cargo -> rustc -> linker chain is *nested*, not
  sequential: a child spawns its own child before the outer
  `Command::output()` call returns, keeping every enclosing level's
  `AppStateSnapshot` alive on the kernel's single call stack until the whole
  chain unwinds. `relay` (re-spawns itself `depth` times before spawning
  `hello`) found three bugs stacked on top of each other, each masking the
  next until the one before it was fixed:
  - Pipe-slot exhaustion: each `Command::output()` call holds 3 pipe slots
    open for its entire duration, including a nested child's runtime --
    `APP_PIPE_COUNT` (4) supported barely more than one link of nesting
    before the next link's pipe allocation failed with `ERR_NOSPC`. Raised
    to 12.
  - The real, deeper bug: raising `APP_PIPE_COUNT` high enough for a nested
    chain to actually *proceed* didn't fix anything by itself -- it exposed
    what looked like a bug in `app_restore_after_spawn`'s pipe-buffer merge,
    but the real root cause was `reset_stdio` (in both `rymos-user` and
    `sys::process::rymos`): it unconditionally reset STDIN/STDOUT/STDERR
    back to *the real console* after a `Command` call, correct only when the
    caller's own ambient stdio already was the console -- wrong for a nested
    call, whose caller's stdout is itself someone else's capturing pipe.
    Confirmed directly against `relay`, bypassing `std`: a chain reported
    success but its output was truncated to just its first line, with the
    rest leaking straight to the console instead of being captured. Fixed
    with a new ABI call, `std_fd` (ABI v23), that reports what a std fd
    *currently* resolves to, so both `Command` implementations save the real
    pre-redirect value and restore exactly that, instead of assuming
    "console."
  - A third bug surfaced once the first two were fixed: a real 3-4 level
    chain still hung (not crashed) partway through unwinding the innermost
    spawn. `app_restore_after_spawn` used to snapshot the *entire* live
    `APP_PIPES` array into a local before merging -- an extra full-size copy
    on top of the already-large `AppStateSnapshot` parameter, stacked at
    every nesting level. This kernel has no stack guard page, so overflowing
    it silently corrupts memory instead of cleanly faulting -- a hang, not a
    diagnosed crash. Fixed by merging the live pipe data directly into the
    snapshot's own already-allocated field instead of a second full-array
    copy.
  - Verified live in QEMU: a 3-level nested `relay` chain (bypassing `std`)
    and a 4-level chain through `cargolike`'s `std::process::Command` both
    complete cleanly, with every level's output (including a real spawned
    child's file contents) correctly propagated all the way to the top.

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
   - real concurrent execution: a scheduler and saved per-process CPU
     register/stack context -- done: see Current Foundation. A first
     deferred/queued cooperative attempt (enqueue at spawn, actually run
     later when something waits) was tried and reverted because several of
     rysh's shell built-ins and every `rymos-user` `Command` helper
     redirected stdio/cwd/env and read the result with no intervening
     `wait()` call, relying on `spawn` finishing synchronously. The second
     attempt, after the ABI's flat globals moved into a real per-process
     control block, fixed those callers for real instead of reverting again
     -- `spawn` now genuinely enqueues and returns, a hand-written
     `context_switch` plus per-pid kernel stacks make `wait`/`wait_any`
     real blocking calls, and the previously-broken callers now `wait()`
     before reverting redirection. Still cooperative, not preemptive: no
     timer interrupt exists yet, so this doesn't yet let two daemons
     interleave -- that's still ahead.
   - real `exec` (replace the current process image in place)
   - wait that blocks on running children -- done: see Current Foundation
     (`wait`/`wait_any` really block on a still-running child now, instead
     of being pure table lookups)

3. Runtime surface:
   - broaden argv support beyond the current 8 args / 64 bytes per arg (this
     is `rymos-user`'s own no_std `PROCESS_ARGV_*` ceiling; unrelated to
     `std::env::args()`, which reads argv straight from the ABI with no
     fixed-count limit of its own -- see Current Foundation)
   - grow `Command` toward more complete stdin/stdout/stderr parity (the
     `rymos-user::Command` builder used by `no_std` programs -- `std`'s own
     `std::process::Command` is now real too, using the same pattern; see
     Current Foundation. Both share the same real limitation: interactive
     `Child::stdin` writes after spawning don't work, since RYMOS's spawn
     runs the child to completion before returning)
   - calibrated time/clock calls -- done: PIT-based `calibrate_tsc` at boot
     plus CMOS RTC wall-clock reads unblocked `std::time::Instant`'s duration
     arithmetic and a real `SystemTime`; see Current Foundation
   - randomness stub or driver -- done at the `std` level: `sys::random`
     does `RDRAND`-or-TSC-seeded-fallback; see Current Foundation
   - richer path normalization beyond the compact RYMFS path limit
   - broader errno coverage across all ABI calls (the kernel's own `ERR_*`
     set is still small; `std`-level mapping of what exists today onto
     `io::ErrorKind` is done -- see Current Foundation)
   - wire `sys::fs`/`sys::env`/`sys::process`/`sys::time` onto the real ABI --
     done for all four now: `fs`/`env`/`time` (ordering *and* duration
     arithmetic, plus a real `SystemTime`), and `process` (`Command` is real
     via `sys::pipe::rymos` + `sys::process::rymos`, using the same pattern
     `rymos-user::Command` already proved correct); see Current Foundation

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

1. Keep `no_std` programs working against ABI v22.
2. Build `core` and `compiler_builtins` for `x86_64-rymos`. Done: verified via
   `RYMOS_TARGET_MODE=custom` building and installing `hello`.
3. Add a tiny `std` compatibility shim for file/env/time/process basics.
   Superseded: real `std` now compiles and links via the `toolchain/rust`
   fork (see Current Foundation); `stdish` remains useful groundwork for the
   ABI-wiring work below.
4. Cross-compile small CLI tools that use `std::fs` and `std::env`. Done,
   and now with a real build pipeline, not just a hand-built test: real
   `fs`/`env`/`args`/`cwd`/`time`/`random`/`process::Command` all work (see
   Current Foundation), and `RYMOS_TARGET_MODE=std python3
   scripts/rymos-sdk.py install stdreal` builds it repeatably via the
   forked toolchain's `-Z build-std` path.
5. Run cargo-like helper programs that spawn children. Done, and it found
   real gaps: `cargolike` (many injected env vars per child, a recursive
   directory walk with real mtimes, several sequential child invocations,
   and a large captured child output) plus `bigoutput` (a helper that writes
   ~6.6 KB of stdout to exercise that last one) ran live in QEMU and
   surfaced three real, now-fixed gaps `stdreal`'s hand-written smoke test
   alone hadn't exercised -- see Current Foundation and
   `docs/self-hosting.md`'s Recently Closed for the pipe-buffer truncation,
   env-var-ceiling abort, and unwired-mtime fixes. Ruled out: repeated
   sequential spawns (8 in a row) don't leak process-table slots. A
   follow-up, `relay` (spawns nested rather than sequential children, the
   real shape of cargo -> rustc -> linker), found three real, layered bugs
   in nested `Command` usage -- pipe-slot exhaustion, a `dup2`-based stdio-
   restore bug that dropped a nested child's output to the console instead
   of its caller's pipe, and a kernel-stack-cost bug that hung a deep
   chain -- all three now fixed and verified 4 levels deep; see Current
   Foundation and `docs/self-hosting.md`'s Recently Closed.
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
  through `rymos-user`) exercising real `std::fs`/`std::env`/`std::process`
  (including a real spawned child via `Command::output()`/`.status()`)/
  `std::time`/`std::random`. Has a real, repeatable build pipeline now:
  `RYMOS_TARGET_MODE=std python3 scripts/rymos-sdk.py install stdreal`
  builds it via the forked toolchain's `-Z build-std` path, clearing the
  stale target cache and stripping debug symbols automatically (see
  `docs/dev-environment.md`). Still deliberately left out of
  `rymos-packages.toml`/`autoexec.bat`'s default flow -- `std` mode is
  never auto-selected, and only `stdreal` opts into it today.
- `faultcheck`: manual CPU exception diagnostic (guard-page touches, raw
  divide-by-zero, invalid opcode) -- deliberately not part of the automated
  regression, since a passing run halts the machine on purpose.
- `cargolike`: a cargo-*shaped* (not cargo itself) `std` smoke test -- many
  injected env vars, a recursive directory walk with real mtimes, several
  sequential child invocations, a nested child invocation (via `relay`), and
  a large captured child output (via `bigoutput`). Built and run the same
  `RYMOS_TARGET_MODE=std` way as `stdreal`, and deliberately kept out of
  `rymos-packages.toml`/`autoexec.bat` for the same reason. `run cargolike
  envtest` separately exercises the env-var-ceiling abort path (a known,
  deliberate crash, kept out of the main suite so it doesn't stop the rest
  of the checks from reporting first).
- `bigoutput`: `no_std` helper that writes ~6.6 KB of stdout, well past any
  single ABI pipe buffer, so `cargolike` can check `Command::output()`
  captures it all rather than silently truncating.
- `relay`: `no_std` helper that re-spawns itself `depth` times via
  `Command::output()` before bottoming out by spawning `hello`, recreating
  a nested (not sequential) spawn chain -- the shape a real cargo -> rustc
  -> linker invocation actually has. Found (and, along with `cargolike`'s
  `nested_spawn_test`, verifies fixed) the pipe-slot-ceiling,
  stdio-redirection, and kernel-stack-cost bugs described above.
