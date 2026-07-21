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

- ABI v22 runs trusted `no_std` ELF programs through `rymos-user`.
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
  cwd/path handling, errno-style `last_error`, calibrated monotonic ticks
  (real nanoseconds since boot, not raw `rdtsc` cycles), real wall-clock time
  via the CMOS RTC, a real `sleep_nanos`, terminal size, and a `stdish`
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
  `std::env` (including argv, cwd, and `temp_dir`), `std::time::Instant` and
  `std::time::SystemTime` (both ordering *and* duration arithmetic, wall
  clock included), `std::thread::sleep`, and `std::random`-backed `HashMap`
  support all round-trip against the ABI -- not just stdio/alloc anymore.
  `std::process::Command` is real too now: `output()`/`.status()` work
  completely (RYMOS's synchronous spawn means the child's exit status and
  full stdout/stderr are already final by the time these read them), with
  one disclosed limitation -- `Command::spawn()` followed by writing to
  `Child::stdin` doesn't behave like a real OS, since the child has already
  run by the time `spawn()` returns (see Recently Closed). `stdreal`, the
  real-`std` test program, now has a repeatable one-command build pipeline
  (`RYMOS_TARGET_MODE=std`) instead of being hand-built. Still stubbed: real
  errno detail beyond the basic `ERR_*` set already mapped.

See `docs/rust-port-roadmap.md` for the detailed cargo/rustc port sequence.

## Recently Closed

- **Real per-process control block (category 2, stage 1 of the scheduler
  work)**: the reverted deferred-spawn attempt's postmortem (below) named
  the actual blocker for real concurrent execution: 18 flat `static mut
  APP_*` globals (console, bootfs, args, pid, process index, fds, pipes,
  std fds, cwd, env, last error, heap/mmap bump pointers) holding all
  per-process ABI state, swapped in and out around each synchronous spawn
  boundary via `AppStateSnapshot`/`app_snapshot`/`app_restore`/
  `app_restore_after_spawn` -- the same machinery that had a confirmed,
  fixed stack-corruption bug earlier this session under deep nested spawns.
  That snapshot dance is gone now: per-process state lives directly on
  `Process` itself (a real control block), addressed through one
  `CURRENT_PROCESS_INDEX` global. A context switch is now just "change
  which index is current," not a copy of a large struct -- the actual
  precondition the plan called for before any scheduler (cooperative or
  preemptive) could be safe. `PROCESS_TABLE` grew one extra reserved slot
  (`PROCESS_COUNT + 1`) as a permanent "idle/no process" index, so every
  existing `index >= PROCESS_COUNT` sentinel check throughout the file
  stayed correct unchanged -- real spawns still only ever allocate
  `0..PROCESS_COUNT`. `APP_CONSOLE`/`APP_BOOTFS`/`APP_FDS`/`APP_PIPES`
  deliberately stayed flat globals rather than per-process fields: the
  first two are genuinely process-independent (one physical console/boot
  image for the whole kernel), and the latter two are already a de facto
  shared table today (only one process ever runs at a time) -- moving them
  is a separate, larger lift (real per-process fd/pipe tables with
  duplication-on-spawn semantics, not attempted here) tracked as a known,
  disclosed scope boundary rather than silently limited.

  Deliberately mechanical and behavior-preserving -- no scheduler yet,
  spawn is still fully synchronous. Verified live in QEMU against the full
  existing regression suite (hello, rysh, `cmdapi`'s full check suite,
  `fswalk`, `heapstress`, `stdshim`, `cargolike` including its nested-spawn
  test, and a 3-level `relay` chain) with byte-identical output to before
  the refactor -- confirming the migration changed *where* state lives,
  not what the ABI does. Real concurrent execution (a scheduler with saved
  per-process CPU context, per-process kernel stacks, and eventually
  preemption) is the next, still-separate stage -- paused here deliberately
  rather than pushed through in one sitting, given the size and risk of
  what's left (hand-written context-switch assembly and, later, the first
  interrupt handler in this kernel that must resume rather than
  diagnose-and-halt).

- **Per-process kernel stacks + a real cooperative scheduler (category 2,
  stage 2 of the scheduler work)**: `spawn` no longer runs a child
  synchronously to completion as a nested Rust call on the kernel's one
  shared stack -- it now prepares the child's address space and its own
  per-pid kernel stack, marks it `Ready`, and returns immediately. Real
  execution happens via a hand-written `context_switch` (naked asm, SysV
  callee-saved regs + `rsp` only -- a fiber/ucontext-style save, not a full
  interrupt frame, since it only ever fires at a deliberate call site) plus
  a `switch_to` wrapper that also swaps `CR3`. `wait`/`abi_wait_any` are
  real blocking calls now: a caller whose target hasn't exited marks itself
  `Blocked` (a new `ProcessState` variant) with a `WaitTarget` and yields
  into the scheduler, which only reconsiders it once that pid actually
  exits (`wake_waiters_for`). A new per-pid kernel-stack address window
  (`KERNEL_STACK_BASE`, 2 MiB stride) reuses the heap/mmap pre-touched-PML4
  pattern, but only maps the top 32 pages of each stride -- a deliberate
  guard gap, since a stack (unlike heap/mmap's software bump-pointer
  checks) has no bound check of its own and this kernel has no hardware
  guard pages; without the gap, overflow would silently corrupt the next
  pid's stack instead of cleanly faulting.

  Two real bugs surfaced and were fixed before this was safe: a process
  cannot free the very stack it's currently executing on (`process_trampoline`
  unmapping its own stack hung the kernel right after its last print) --
  fixed with a deferred-reclaim queue drained by the *next* successful
  `context_switch` return, which by construction is always running on a
  different stack. And destroying a process's address space frees the
  physical page its own `CR3` still points at -- fixed by explicitly
  restoring `CR3` to the shared kernel PML4 immediately after teardown,
  closing a window where the CPU briefly ran on a dangling page-table root.

  This is also the second, successful attempt at deferred/async spawn --
  the first was reverted (see below) because several real callers spawned,
  immediately reverted stdio redirection or read a pipe, and assumed the
  old synchronous-completion behavior with no `wait()` in between. This
  time the callers were actually fixed instead of worked around: rysh's
  `spawn_redir`/`spawn_stdin`/`spawn_io`/`spawn_io_err` builtins and
  `rymos-user`'s `run_command_output`/`run_command_status` now `wait()` the
  child before reverting redirection, not after. Verified live in QEMU by
  replaying the exact scenario that hung the reverted attempt --
  `echoin` via rysh's `spawnstdin`/`spawnio`/`spawnioe` -- now completing
  cleanly with real captured pipe data instead of `echoin: stdin read
  failed`, plus the full regression suite (`cmdapi`'s complete check suite
  including `spawn-many + wait_any` reaping three concurrent children and
  zombie-status reaping, `fswalk`, `heapstress`, `stdshim`, `cargolike`
  including its nested-spawn test, and a 3-level `relay` chain) all passing
  unchanged. Top-level `run` and nested `spawn` now share one code path
  (`prepare_process`) -- previously only nested spawns got isolated address
  spaces, an asymmetry no longer safe to leave once spawns can genuinely
  overlap.

  Still cooperative, not preemptive: yields only happen at the two explicit
  call sites above (`wait`/`wait_any`, process exit) -- no timer interrupt
  exists yet, so two daemons still can't interleave mid-execution. The
  scheduler core (`pick_next_ready`, `wake_waiters_for`, state transitions)
  is written with IF-save/restore critical sections from the start even
  though nothing unmasks interrupts yet, so Stage 3's timer ISR can call
  into the same core without a rewrite. An audit of every
  `PHYS_ALLOCATOR`/`PROCESS_TABLE`/`NEXT_PID`/PFS-header touch point found
  none coinciding with either yield point, so no further critical sections
  were needed for this stage. Real preemption (a timer interrupt actually
  driving the reschedule decision, so two backgrounded programs can produce
  genuinely interleaved output) is the next, still-separate stage.

- **Timer interrupt plumbing, no rescheduling yet (category 2, stage 3a of
  the scheduler work)**: split out as its own checkpoint deliberately, since
  this is the highest-risk code in the whole scheduler/preemption plan --
  the first interrupt in this kernel that must *resume* the interrupted
  task, not diagnose-and-halt like every existing CPU exception handler.
  The 8259 PIC's power-on vector base (`0x08`) collides with CPU exception
  vectors 8-15 (8 = double fault), so `remap_pic` moves IRQ0-7 to vectors
  32-39 and IRQ8-15 to 40-47 before anything gets unmasked; only IRQ0 (the
  PIT) is left unmasked, every other line stays masked since nothing needs
  it yet. `IDT_LEN` grew from 32 to 48 to cover the newly-reachable vectors.
  PIT channel 0 is programmed (mode 3, ~100 Hz) for the periodic tick.

  `irq0_stub` is the new resuming code: it saves every general-purpose
  register (an interrupt can land mid-instruction with any of them live,
  unlike a normal call site where the compiler has already spilled what it
  needs), calls into Rust to bump a tick counter and send the End-Of-
  Interrupt, restores every register, and `iretq`s back into the
  interrupted task unchanged. Since the incoming stack alignment at an
  arbitrary interrupted instruction can't be assumed 16-byte aligned (unlike
  the existing exception stubs, which can just `and rsp, -16` destructively
  since they only ever diverge to a fatal halt), it snapshots `rsp` into
  `rbp` -- free to reuse once `rbp`'s real value is already saved on the
  stack -- aligns down for the SysV `call`, then restores the exact
  original `rsp` before popping everything back, correct regardless of what
  the interrupted code's stack looked like. Two benign default handlers
  (`irq_spurious_master_stub`/`irq_spurious_slave_stub`, pure inline asm, no
  Rust call) cover vectors 33-47 as defense in depth -- a spurious IRQ7/
  IRQ15 is a well-known hardware quirk that can fire even while masked, and
  this EOIs and resumes instead of hitting the fatal exception path. `sti`
  is called exactly once, in a new `enable_timer_interrupts`, only after the
  PIC remap, PIT programming, and all the IDT gates above are already in
  place -- the first and only `sti` anywhere in this kernel.

  Zero rescheduling logic is touched by this stage on purpose, so a failure
  would be unambiguous about which half broke (interrupt resume vs.
  scheduling decision). Verified live in QEMU: a new `timer ticks (IRQ0,
  ~100 Hz)` line on the existing `mem` command (which the boot script
  already runs three times) showed a real, monotonically increasing count
  (1 -> 1 -> 389 across one boot) with the full existing regression suite
  -- including Stage 2's `echoin`/rysh scenario, `cmdapi`'s spawn-many/
  wait_any and zombie-reaping checks, `cargolike`, and `relay` -- passing
  completely unchanged. Real preemption (the timer ISR actually calling
  into Stage 2's reschedule core, so two backgrounded programs can produce
  genuinely interleaved output) is Stage 3b, still ahead.

- **`relay`: a nested-spawn stress test that found and fixed three real,
  layered gaps `cargolike`'s flat, sequential tests couldn't see**:
  `cargolike`'s `repeated_spawn_test` only proved *sequential* spawns (one
  at a time, each fully finished before the next starts) don't leak
  process-table slots. A real cargo -> rustc -> linker chain is *nested*: a
  child spawns its own child before the outer `Command::output()` call
  returns. Since RYMOS's spawn runs synchronously to completion, a nested
  spawn keeps every enclosing level's `AppStateSnapshot` (the ambient
  fds/pipes/cwd/env state `spawn`/`Command` save-and-restore around a
  synchronous child) sitting on the kernel's single call stack until the
  whole chain unwinds -- a fundamentally different situation than any prior
  test exercised. Wrote `programs/relay`, a `no_std` helper that re-spawns
  itself `depth` times via `Command::output()` before bottoming out by
  spawning `hello`, and drove it both directly and through `cargolike`'s
  (`std`) `Command` to find out what actually happens. All three gaps found
  are now fixed; nested `Command` usage (verified 4 levels deep, through
  both `rymos-user`'s and `std`'s `Command`) is real.
  - **Pipe-slot exhaustion**: each `Command::output()`/`.status()` call
    holds 3 pipe slots open (stdin/stdout/stderr) for its *entire* duration,
    including however long a nested child takes -- so `APP_PIPE_COUNT`'s old
    value of 4 supported barely more than one link of nesting before the
    next link's pipe allocation failed with `ERR_NOSPC`. Raised to 12.
  - **The real, deeper bug: `dup2`-based stdio restore, not the pipe
    table**: raising `APP_PIPE_COUNT` high enough for a nested chain to
    actually *proceed* didn't fix anything by itself -- it exposed a second
    bug that looked, at first, like it lived in `app_restore_after_spawn`'s
    pipe-buffer merge (which only holds for one level of redirection). The
    real root cause was `reset_stdio` (present in both `rymos-user` and
    `sys::process::rymos`, `std`'s `Command` impl): after a `Command` call,
    it unconditionally reset `STDIN`/`STDOUT`/`STDERR` back to *the real
    console* (`dup2(STDOUT, STDOUT)`'s "same fd" case means exactly that,
    not "leave it alone"). That's correct only when the caller's own
    ambient stdio already was the console -- wrong for a nested call, whose
    caller's stdout is itself someone else's capturing pipe. Concretely:
    `relay(0)` undoing its *own* redirect (after spawning `hello`) blew away
    `relay(1)`'s redirect instead of restoring it, so everything `relay(0)`
    printed afterward went straight to the console instead of into
    `relay(1)`'s pipe -- confirmed directly against `relay` alone (bypassing
    `std` entirely): a nested chain reported a clean success exit code, but
    its captured output was truncated to just its first line, with `hello`'s
    real output leaking straight to the serial console instead of being
    captured. Fixed with a new ABI call, `std_fd` (ABI v23), that reports
    what a std fd *currently* resolves to; both `Command` implementations
    now save the real pre-redirect value (`save_stdio`) before their own
    redirection and restore exactly that afterward (`restore_stdio`),
    instead of assuming "console."
  - **A third, genuinely different bug surfaced once the first two were
    fixed**: with the redirect logic correct, a real 3-4 level nested
    `Command` chain still *hung* (not crashed) partway through unwinding the
    innermost spawn. Traced (via temporary debug prints in
    `run_ready_task`) to `app_restore_after_spawn` itself: it used to
    snapshot the *entire* live `APP_PIPES` array into a local
    (`child_pipes`) before merging, an extra full-size copy (each `AppPipe`
    embeds a whole `APP_PIPE_BUFFER_SIZE`-byte buffer) on top of the
    already-large `AppStateSnapshot` parameter, stacked at every level of a
    nested chain. This kernel has no stack guard page, so overflowing it
    silently corrupts adjacent memory instead of cleanly faulting -- a hang,
    not a diagnosed crash. Confirmed live: a 3-level chain hung right at
    this function's entry for the innermost spawn, the single deepest point
    of stack usage in the whole chain. Fixed by merging the live pipe data
    directly into the snapshot's own already-allocated `pipes` field instead
    of a second full-array copy -- same result, no extra large local.
  - Verified live in QEMU end to end: a 3-level nested `relay` chain
    (bypassing `std`) and a 4-level chain through `cargolike`'s `std`
    `Command` both complete cleanly, with every level's output (including
    `hello`'s real file contents) correctly propagated all the way up to the
    top-level capture -- not just "didn't crash," but byte-for-byte correct.

- **`cargolike`: a cargo-shaped smoke test that found three real gaps
  `stdreal` never exercised, all now fixed**: category 6 proved
  `std::process::Command` and a `stdreal` build pipeline worked end to end on
  one hand-written program, but flagged that this didn't prove cargo's actual
  patterns (many injected env vars per child, recursive directory walks with
  real mtimes, several sequential child invocations, large captured child
  output) were dependable. Wrote `programs/cargolike` (a real-`std` program,
  built the same `RYMOS_TARGET_MODE=std` way as `stdreal`) plus
  `programs/bigoutput` (a `no_std` helper that writes ~6.6 KB of stdout, well
  past any output `stdreal` had produced) and ran them live in QEMU to find
  out empirically rather than guess. Two of three hypotheses were confirmed
  as real, fixable gaps; one (process-table exhaustion across repeated
  spawns) was ruled out -- 8 sequential `Command::output()` calls in a row
  all succeeded with no leak, since the process table already reclaims a
  zombie slot once its parent has waited on it (category 2).
  - **Pipe buffer truncation**: `Command::output()` capturing `bigoutput`'s
    ~6.6 KB of stdout came back truncated to exactly 1024 bytes -- the ABI's
    per-pipe buffer (`APP_PIPE_BUFFER_SIZE`) was a fixed 1 KiB, far smaller
    than a real rustc invocation's diagnostics routinely are. Raised to 8
    KiB (`kernel/src/main.rs`) -- generous headroom without the static-memory
    or (since these buffers are embedded in a snapshot struct copied around
    stack-locally at `spawn`/redirect boundaries, not just a `static`) stack
    footprint concern a jump straight to 64 KiB would have carried. Verified
    live: the same capture now comes back at the real 6634 bytes (6600 from
    the loop plus a summary line), not truncated.
  - **Env var ceiling was reached almost immediately, and aborted the whole
    process, not just the `set_var` call**: RYMOS's env table
    (`APP_ENV_COUNT`) was a fixed 8 slots *total*, including the 6 the kernel
    seeds by default (`PATH`/`HOME`/`SHELL`/`USER`/`RYMOS_TARGET`/`TMPDIR`),
    leaving only 2 free -- nowhere near a real cargo child's dozen-plus
    (`CARGO_MANIFEST_DIR`/`OUT_DIR`/`TARGET`/`RUSTC`/`CARGO_PKG_*`/...).
    Confirmed live in QEMU that hitting the ceiling doesn't just return an
    error: `std::env::set_var` panics on any backend failure, and since this
    target builds `std` with `panic_abort` (no unwinding), that aborts the
    entire process via `abort_internal`'s `ud2` -- caught cleanly by
    category 3's CPU exception handler as a diagnosed halt rather than a
    silent crash, but a hard stop all the same. Raised `APP_ENV_COUNT` from
    8 to 64 slots; re-verified live that the *raised* ceiling still fails the
    same safe way once genuinely exhausted (pushed a test past it, to 128
    vars, and got the same clean exception-handler halt), rather than
    silently corrupting memory once the new, larger table is full.
  - **`FileAttr::modified()`/`created()` were stubbed `unsupported()` even
    though the data already existed**: `Stat` (from category 5) already
    carried `created_ticks`/`modified_ticks`, but nothing read them. The
    on-disk PFS values themselves turned out to be raw `rdtsc` cycle counts
    (`pfs_set_entry`/`pfs_touch_modified` in `kernel/src/main.rs`), not the
    same calibrated nanoseconds-since-boot unit `time_ticks`'s ABI call
    reports -- userspace only ever sees `time_ticks`/`time_unix_nanos`, never
    the kernel-internal `TSC_HZ`/`BOOT_TSC` needed to convert a raw cycle
    count back into real time, so storing raw cycles there was a dead end
    for libstd regardless of what stub code was written on top of it. Fixed
    at the source: PFS now stores `ns_since_boot()` (the same unit
    `time_ticks` already exposes) at every create/modify site instead of raw
    `read_tsc()`. `sys::fs::rymos::FileAttr::modified()`/`created()` then
    convert that to a real `SystemTime` via
    `time_unix_nanos() - time_ticks()` as the boot-time offset -- the same
    arithmetic the kernel does internally with `BOOT_UNIX_NANOS`, just
    computed from the userspace side of the ABI, which never sees that
    constant directly. `accessed()` deliberately stays `unsupported()`: `Stat`
    has no real access-time field at all, and there's no data to report
    without fabricating one. Verified live in QEMU:
    `fs::metadata(...).modified()` on a freshly-written PFS file returned a
    real Unix timestamp matching the actual date, not the previous
    `unsupported` error.

  `cargolike` (like `stdreal`) is deliberately kept out of
  `rymos-packages.toml`/`autoexec.bat`'s default flow -- it's a `std`-mode
  diagnostic program, not a shipped one.

- **Real `std::process::Command`, and a real build pipeline for `stdreal`**
  (category 6 -- the last major stretch before self-hosting becomes
  reachable): `std::process::Command` (spawning) was left deliberately
  unsupported after category 5, flagged as "a real design mismatch, not a
  bounded wiring task." Revisiting it found the mismatch was narrower than
  first assessed: RYMOS's `spawn`/`spawn_argv` run the child to completion
  *inline* before returning (no fork+exec, no real concurrency -- see
  category 2), and `rymos-user`'s own `Command` (used successfully
  throughout this project by `cmdapi`/`stdshim`) already proved that
  "snapshot the ambient stdio/cwd/env state, mutate it, spawn synchronously,
  restore it" is a correct pattern for that model. Porting the same pattern
  into `std`'s `sys::process` module, rather than inventing something new,
  turned out to be a bounded, well-defined task after all:
  - `sys::pipe::rymos` wraps the ABI's `pipe`/`read`/`write_fd`/`close` --
    real in-memory pipes, not a stub.
  - `sys::process::rymos` implements `Command`, `Stdio`, `Process`,
    `ExitStatus`, `ExitCode`. Stdio redirection dup2's the child's stdin/
    stdout/stderr onto the ambient std fds before spawning (exactly
    `rymos-user::Command`'s approach), snapshotting and restoring the
    original std fds, cwd, and env afterward regardless of success or
    failure. Env overrides (including a full `env_clear()`) snapshot every
    touched key's original value first so they can be restored precisely,
    not just approximately.
  - `Stdio::null()` has no real `/dev/null` to back it, so it uses a scratch
    pipe whose unused end is immediately dropped: reading an empty pipe
    with no writer returns `0` (EOF) immediately rather than blocking (see
    `kernel/src/main.rs`'s `abi_read`), so this behaves exactly like a null
    device in both directions without needing a dedicated null-device
    concept at the ABI level.
  - Because the child always finishes before `spawn` returns, `Process` can
    just store the already-known exit code directly -- `wait`/`try_wait`
    are table lookups, not real waits, matching how the ABI itself already
    works (`abi::wait` was always a lookup against an already-finished
    child, not a real wait, even before this).
  - The one real, disclosed limitation: `Command::spawn()` followed by
    writing to `Child::stdin` does **not** behave like a real OS. The child
    has already run (and already read whatever was in its stdin pipe, or
    read nothing) by the time `spawn()` returns, so anything written
    afterward never reaches it. This doesn't affect `output()`/`.status()`
    (the methods cargo/rustc invocation actually uses) since neither feeds
    stdin data by default -- it only affects the less-common pattern of
    spawning and then interactively writing to a child's stdin, which needs
    real concurrent execution to fix properly (category 2's still-open
    scheduler work), not more ABI wiring.

  Verified live in QEMU: `stdreal`'s `Command::output()` captured a real
  child's exit code (`0`) and 326 bytes of its actual stdout content (not
  garbage); `Command::status()` with an `env()` override succeeded, and the
  override was confirmed *not* to leak into the parent's own environment
  afterward -- both against the full existing regression suite, undisturbed.

  Separately, `stdreal` (the real-`std` test program) went from a hand-run
  sequence of manual commands to a real, repeatable build pipeline:
  `RYMOS_TARGET_MODE=std python3 scripts/rymos-sdk.py install stdreal` now
  builds it via the `rymos-fork` toolchain's `-Z build-std` path end to end.
  The script automates three things that were previously easy to get wrong
  by hand (and did, earlier this session): it always clears the stale
  `-Z build-std` target cache before building (the exact gotcha that
  produced a real, confirmed false crash in category 5 -- a genuine fix
  looked like it hadn't taken effect until the directory was removed); it
  recreates the `rust-lld` symlink the stage1 sysroot loses on every
  toolchain rebuild (`docs/dev-environment.md`'s "rust-lld gotcha"); and it
  strips debug symbols afterward (a `--release` real-`std` binary is still
  ~1.2 MB unstripped vs ~100 KB stripped). Deliberately kept out of
  `rymos-packages.toml`/`install-all`'s default flow -- `std` mode is never
  auto-selected, and only `stdreal` opts into it today, so contaminating the
  default no_std build path with it would be a regression, not progress.

- **Calibrated time, sleep, wall clock, and terminal size** (category 4):
  `time_ticks` used to be a raw `rdtsc` read with no defined relationship to
  real time -- fine for ordering, meaningless for duration math. Added
  `calibrate_tsc`, which measures `rdtsc` ticks per second once at boot by
  polling the legacy PIT's channel 2: arm a known countdown, read `rdtsc`
  before and after, see how many ticks elapsed while a known amount of real
  time passed. Entirely by polling -- no timer interrupt needed, consistent
  with this kernel deliberately having none beyond the CPU exception
  handlers category 3 added. Averaged across 4 rounds (the PIT's 16-bit
  counter caps one run at ~55 ms) to reduce jitter from trusting a single
  short sample. ABI v22's `time_ticks` now returns real calibrated
  nanoseconds since boot.

  Real wall-clock time exists now too: `time_unix_nanos` reads the CMOS RTC
  once at boot (BCD-or-binary, 12-or-24-hour, using the classic
  wait-for-UIP-then-double-read technique to avoid a torn read mid-tick),
  paired with the calibration's boot `rdtsc` snapshot so later reads are
  cheap (boot reading plus nanoseconds elapsed since, no repeated CMOS I/O).
  Converted to a Unix timestamp via Howard Hinnant's `days_from_civil`
  algorithm rather than a hand-rolled (and easy to get subtly wrong around
  leap years) month-length table. The CMOS year register is only two
  digits with no standardized century register, so this assumes the 21st
  century -- disclosed, good enough for a log timestamp, not a substitute
  for NTP.

  `sleep_nanos` busy-waits against the calibrated clock. Correct, not a
  placeholder: RYMOS has no scheduler to hand the CPU to during a sleep
  regardless, so spinning is the real implementation here, the same
  reasoning already applied to the exception handlers' halt loops.
  `term_size` reports the console's real row/column count instead of
  callers having to assume 80x25.

  All four are wired end to end -- kernel ABI, `rymos-user`
  (`time_ticks`/`time_unix_nanos`/`sleep_nanos`/`term_size`), and
  `toolchain/rust`'s `std`: `sys::time::rymos`'s `Instant` duration
  arithmetic and `SystemTime` are both genuinely real now (previously
  `Instant`'s duration math was honestly `None` and `SystemTime` reused the
  panicking `unsupported` stub, specifically because no calibration existed
  yet -- see category 5). `std::thread::sleep` is real too: sleeping the
  *current* (only) thread of execution needs no multi-threading support,
  so reusing `sys::thread::unsupported`'s `Thread`/`available_parallelism`/
  etc. (genuinely correct -- RYMOS is single-threaded per process) while
  overriding just `sleep` was the right split, not an all-or-nothing choice.

  Verified live in QEMU: `stdshim` sleeps 10 ms and confirms elapsed ticks
  advanced by at least that much; a new wall-clock print showed a real,
  plausible Unix timestamp. `stdreal` (the genuine `std` test program from
  category 5) sleeps 15 ms via `std::thread::sleep` and measures ~15.14 ms
  via real `Instant::checked_duration_since`; `SystemTime::now()
  .duration_since(UNIX_EPOCH)` reported 20,652 days since epoch --
  independently checked, that's mid-2026, matching the actual date, not
  just "a number that didn't crash."

  Assessed, scoped down deliberately: "terminal/TTY behavior beyond the
  current console stream" turned out to be the vaguest of category 4's
  three items, with no concrete downstream consumer identified yet (unlike
  tick calibration, which category 5 had already surfaced a real,
  documented need for). Did the cheap, clearly-real, bounded piece (a
  terminal size query) rather than speculatively building out raw/cooked
  mode switching or ANSI/VT100 escape handling with no consumer in sight --
  left for whenever something actually needs it.

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
     register/stack context -- done: see Recently Closed. `spawn` genuinely
     enqueues and returns; a hand-written `context_switch` plus per-pid
     kernel stacks (with a guard gap) make `wait`/`wait_any` real blocking
     calls. Still cooperative, not preemptive -- yields only happen at
     `wait`/`wait_any` and process exit, so two daemons still can't
     interleave mid-execution. That needs a timer interrupt actually
     driving the reschedule decision, still ahead.
   - add relocatable/PIE program loading (unrelated to the isolation work
     above, which keeps every process at the same virtual address)
   - add real `exec`
   - support blocking wait once concurrency exists -- done: see Recently
     Closed (`wait`/`wait_any` now really block on a still-running child
     instead of being pure table lookups)

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
   - calibrate ticks into real wall/monotonic time units -- done: PIT-based
     `calibrate_tsc` at boot; see Recently Closed
   - add sleep/timer behavior -- done: `sleep_nanos`, real
     `std::thread::sleep`; see Recently Closed. Still just busy-waiting, no
     timer interrupt -- correct for now (no scheduler to hand the CPU to
     either), would need revisiting only alongside real preemption
   - add terminal/TTY behavior beyond the current console stream -- narrowly
     done (real terminal size query); raw/cooked mode switching and
     ANSI/VT100 escape handling deliberately not attempted with no concrete
     consumer identified yet -- see Recently Closed
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
   - `std::time::Instant`/`SystemTime` -- done, ordering *and* duration
     arithmetic, wall clock included; see category 4's Recently Closed entry
   - `std::process::Command` (spawning) -- done: `sys::pipe::rymos` plus
     `sys::process::rymos` implement it using the same pattern
     `rymos-user::Command` already proved correct (dup2 stdio onto the
     ambient std fds, snapshot/restore cwd and env, spawn synchronously).
     `output()`/`.status()` work completely; `std::process::id()` too. Real,
     disclosed limitation: `Command::spawn()` + writing to `Child::stdin`
     afterward doesn't work like a real OS (the child already ran by then)
     -- needs real concurrent execution to fix, not more ABI wiring; see
     category 6's Recently Closed entry
   - real thread support if it's ever needed (today `no_threads` is correct
     for RYMOS's single-threaded-per-process reality, but note this in case
     that reality changes -- see Process model, item 2)
   - cross-compile small `std` CLI programs that actually touch `fs`/`env` --
     done: see `stdreal` in Recently Closed, now with a real one-command
     build pipeline (`RYMOS_TARGET_MODE=std`) instead of a hand-built test

6. Cargo and rustc:
   - `std::process::Command` and a repeatable `stdreal` build pipeline --
     done: see Recently Closed. These were flagged as the last major
     stretch before self-hosting becomes reachable; both are closed now,
     but that means the plumbing works end to end on one hand-written test
     program, not that cargo's real dependency graph/build scripts or
     rustc's own scale are dependable yet -- the items below are still real,
     not just formalities
   - run cargo-like helper programs first -- done: `cargolike`/`bigoutput`
     found and closed three real gaps (pipe-buffer truncation, an env-var
     ceiling that aborted the whole process instead of just failing the
     call, and unwired file mtimes); see Recently Closed. Confirmed *not* an
     issue: repeated sequential spawns (8 in a row) don't leak process-table
     slots. `relay` then found nested (not just sequential) spawns had three
     real, layered bugs -- pipe-slot exhaustion, a `dup2`-based stdio-restore
     bug that silently dropped a nested child's output to the console
     instead of its caller's pipe, and a kernel-stack-cost bug that hung
     (not crashed) a deep chain -- all three now fixed and verified 4 levels
     deep through both `rymos-user`'s and `std`'s `Command`; see Recently
     Closed
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
- small real `std` Rust programs: working today, with real breadth, not
  just "can this work at all": `std` compiles, links, and *runs* for
  `x86_64-rymos` via a forked `rust-lang/rust` (`toolchain/rust`), the same
  way other small OSes (Redox, Hermit, ...) got their own PAL support, and
  `fs`/`env`/`args`/`cwd`/`temp_dir`/`Instant`/`SystemTime`/`sleep`/`random`/
  `process::Command` are all real now (see Recently Closed) -- verified live
  in QEMU with a `stdreal` program touching all of them, including spawning
  a real child via `Command::output()`/`.status()`, plus
  `println!`/`Vec`/`HashMap`/iterators. `stdreal` also has a real,
  repeatable build pipeline now (`RYMOS_TARGET_MODE=std`), not a hand-built
  one-off. What's left at the `std` layer: `Command::spawn()` +
  interactive `Child::stdin` writes (needs real concurrent execution, not
  more wiring -- see category 2), and broader errno detail beyond the basic
  `ERR_*` set already mapped.
- `cargo`: still far, but the two blockers flagged as "the last major
  stretch" are closed: `std::process::Command` is real, and `stdreal` has a
  repeatable build pipeline. A first cargo-shaped smoke test (`cargolike`)
  also ran live and found three real gaps a hand-written `stdreal`-style
  program alone hadn't exercised -- a 1 KiB pipe buffer that silently
  truncated large captured child output, an 8-slot env table that aborted
  the whole process (not just the failing call) once a real cargo child's
  env var count would exceed it, and file mtimes that were never wired up
  even though the kernel already tracked them -- all three now fixed (see
  Recently Closed). A follow-up nested-spawn test (`relay`) found three more
  real, layered bugs that would have blocked a genuine cargo port outright
  (cargo invoking rustc, which itself shells out to a linker, is exactly a
  nested `Command` chain) -- pipe-slot exhaustion, a `dup2`-based
  stdio-restore bug that silently dropped a nested child's output to the
  console instead of its actual caller's pipe, and a kernel-stack-cost bug
  in `app_restore_after_spawn` that hung (not crashed) a deep chain. All
  three are now fixed and verified live: a 4-level nested `Command` chain
  through `cargolike`'s `std::process::Command` completes cleanly with every
  level's output correctly propagated to the top. What remains is the
  actual scale/reliability work category 2-4 still have open (real
  concurrent execution, unbounded directory growth, richer path
  normalization), plus running further, larger cargo-like helper programs to
  find out what else is needed beyond what `cargolike`/`relay` covered.
- `rustc`: very far; needs all of `cargo`'s foundations plus much stronger
  memory management, large files, many descriptors, and reliable child process
  execution.

That is why milestone 8 is a readiness milestone: it prevents us from losing
the map while we build the real road.
