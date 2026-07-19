#![feature(restricted_std)]

//! A cargo-*shaped* smoke test -- not cargo itself, but exercising the
//! patterns a real cargo/rustc invocation actually leans on that
//! `programs/stdreal` never touched: many injected env vars (cargo sets
//! `CARGO_*`/`OUT_DIR`/`RUSTC`/etc. per child), recursive directory walks
//! with real mtimes (for incremental-rebuild decisions), several sequential
//! child invocations in a row (simulating compiling several source files),
//! *nested* child invocations (a child that itself spawns a child, like
//! cargo -> rustc -> linker, via `programs/relay`), and capturing a child's
//! *large* output (rustc's diagnostics are often many KB, not the few dozen
//! bytes `stdreal`'s test child produced).
//!
//! Built and run the same way as `stdreal` (see
//! `docs/dev-environment.md`/`RYMOS_TARGET_MODE=std`), and deliberately kept
//! out of `rymos-packages.toml`/`autoexec.bat` for the same reason.
//!
//! `envtest` is a separate opt-in argv mode (`run cargolike envtest`): it
//! deliberately pushes past the ABI's env-var ceiling, which -- confirmed
//! live, not just reasoned about -- panics via `std::env::set_var`, and
//! since this kernel builds `std` with `panic_abort` (no unwinding), that
//! *aborts the whole process*, which this kernel's exception handler turns
//! into a halt (see category 3). Kept separate from the main suite so a
//! known, deliberate failure doesn't stop the rest of the checks from
//! running and reporting first.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

fn main() {
    println!("cargolike: cargo-shaped smoke test");

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("envtest") {
        env_stress_test();
        return;
    }

    recursive_dir_and_mtime_test();
    repeated_spawn_test();
    nested_spawn_test();
    big_output_capture_test();

    println!("cargolike: ok (run `cargolike envtest` separately for the env-count check)");
}

/// Real cargo/build-script invocations set many env vars per child
/// (`CARGO_MANIFEST_DIR`, `OUT_DIR`, `TARGET`, `HOST`, `PROFILE`, `RUSTC`,
/// `CARGO`, `CARGO_PKG_*`, ...) -- easily a dozen or more. This originally
/// found a real gap live: RYMOS's env table was a fixed 8 slots total
/// *including* the 6 the kernel seeds by default
/// (`PATH`/`HOME`/`SHELL`/`USER`/`RYMOS_TARGET`/`TMPDIR`), leaving only 2
/// free -- nowhere near enough for a real cargo child, and since
/// `std::env::set_var` panics on failure (and this target builds `std` with
/// `panic_abort`, no unwinding), filling the table aborted the whole
/// process. The table is now 64 slots; this loop pushes well past that
/// (128) to confirm the *raised* ceiling still fails safely rather than
/// silently corrupting memory once truly exhausted.
fn env_stress_test() {
    println!("cargolike: envtest -- setting env vars until it panics/aborts");
    for index in 0..128 {
        let key = format!("CARGOLIKE_VAR_{index}");
        println!("cargolike: envtest setting {key}");
        unsafe {
            std::env::set_var(&key, "value");
        }
    }
    println!("cargolike: envtest FAILED -- expected a panic/abort before reaching here");
}

fn recursive_dir_and_mtime_test() {
    let root = "pfs:cargolike-tree";
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(format!("{root}/src/nested")).expect("create_dir_all");
    fs::write(format!("{root}/src/main.rs"), b"fn main() {}\n").expect("write main.rs");
    fs::write(format!("{root}/src/nested/lib.rs"), b"pub fn f() {}\n").expect("write lib.rs");
    fs::write(format!("{root}/Cargo.toml"), b"[package]\n").expect("write Cargo.toml");

    let mut visited = Vec::new();
    walk(Path::new(root), &mut visited);
    visited.sort();
    println!("cargolike: walked {} entries under {root}", visited.len());
    for path in &visited {
        println!("cargolike: entry {}", path.display());
    }

    match fs::metadata(format!("{root}/src/main.rs")).and_then(|m| m.modified()) {
        Ok(modified) => match modified.duration_since(UNIX_EPOCH) {
            Ok(since_epoch) => {
                println!("cargolike: main.rs modified {} s since epoch", since_epoch.as_secs())
            }
            Err(err) => println!("cargolike: main.rs modified duration_since FAILED: {err}"),
        },
        Err(err) => println!("cargolike: main.rs metadata().modified() FAILED: {err}"),
    }

    let _ = fs::remove_dir_all(root);
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        println!("cargolike: read_dir FAILED for {}", dir.display());
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => walk(&path, out),
            _ => out.push(path),
        }
    }
}

/// Simulates cargo invoking rustc once per source file: several sequential
/// `Command::output()` calls in a row, each spawning and reaping a real
/// child, checking the process table doesn't leak/exhaust across repeated
/// invocations (`PROCESS_COUNT` is a fixed 16 slots).
fn repeated_spawn_test() {
    const ROUNDS: usize = 8;
    let mut ok = 0usize;
    for round in 0..ROUNDS {
        match std::process::Command::new("hello").arg("readme.txt").output() {
            Ok(output) if output.status.success() => ok += 1,
            Ok(output) => {
                println!("cargolike: repeated_spawn round {round} bad status {:?}", output.status)
            }
            Err(err) => println!("cargolike: repeated_spawn round {round} FAILED: {err}"),
        }
    }
    println!("cargolike: repeated_spawn {ok}/{ROUNDS} rounds succeeded");
}

/// Cargo's own child process tree is nested, not flat -- cargo spawns
/// rustc, rustc can spawn a linker, and so on. `repeated_spawn_test` above
/// only ever proved *sequential* spawns are safe: at most one
/// `AppStateSnapshot` (the ambient fds/pipes/cwd/env state `spawn`/`Command`
/// save-and-restore around a synchronous child -- see `kernel/src/main.rs`)
/// was ever live at a time. `programs/relay` (re-spawns itself `depth` times
/// via `Command::output()` before bottoming out by spawning `hello`) drives
/// several *concurrently* live instead, the way a real cargo -> rustc ->
/// linker chain would.
///
/// This found two real, layered gaps live, and both are now fixed:
/// - Each `Command::output()` call holds 3 pipe slots open (stdin/stdout/
///   stderr) for its *entire* duration, including however long a nested
///   child takes. `APP_PIPE_COUNT`'s old value of 4 meant even this test's
///   own outer `Command::new("relay")` call plus one level of `relay`
///   nesting (2 concurrent links, 6 slots) failed immediately with
///   `ERR_NOSPC`. Raised to 12.
/// - Raising `APP_PIPE_COUNT` high enough for a nested chain to actually
///   *proceed* first exposed what looked like a second, deeper bug: a
///   nested chain reported success but its captured output was truncated to
///   just its first line (confirmed directly against `relay`, bypassing
///   `std` entirely), and through `std::process::Command` specifically,
///   hung outright. The real root cause turned out to be `dup2`-based
///   stdio restore, not the pipe table: `reset_stdio` (in both
///   `rymos-user` and `sys::process::rymos`) unconditionally reset
///   STDIN/STDOUT/STDERR back to *the real console* after a `Command` call,
///   which is correct only when the caller's own ambient stdio already was
///   the console -- wrong for a nested call, whose caller's stdout is
///   itself someone else's capturing pipe. `relay(0)` undoing its *own*
///   redirect (after spawning `hello`) was blowing away `relay(1)`'s
///   redirect instead of restoring it, so everything `relay(0)` printed
///   afterward went straight to the console instead of into `relay(1)`'s
///   pipe. Fixed with a new ABI call, `std_fd` (ABI v23), that reports what
///   a std fd *currently* resolves to, so both `Command` implementations
///   now save the real pre-redirect value and restore exactly that,
///   instead of assuming "console."
fn nested_spawn_test() {
    match std::process::Command::new("relay").arg("1").output() {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let relay_lines = text.matches("relay depth=").count();
            let hello_seen = text.contains("reading readme.txt");
            println!(
                "cargolike: nested_spawn depth=1 exit={:?} relay_lines={relay_lines}/4 hello_seen={hello_seen}",
                output.status
            );
            if !output.status.success() || relay_lines != 4 || !hello_seen {
                println!("cargolike: nested_spawn FAILED -- chain didn't complete/propagate correctly");
            }
        }
        Err(err) => println!("cargolike: nested_spawn Command FAILED: {err}"),
    }
}

/// `stdreal`'s `Command::output()` check only ever captured a few hundred
/// bytes. A real rustc invocation's stdout/diagnostics are routinely many
/// KB -- `bigoutput` deliberately writes ~6.6 KB (several times the ABI
/// pipe's current 1 KB buffer) to check whether `Command::output()`
/// actually captures all of it or silently truncates.
fn big_output_capture_test() {
    match std::process::Command::new("bigoutput").output() {
        Ok(output) => {
            let expected = 200 * 33; // LINE_COUNT * LINE.len() in programs/bigoutput
            println!(
                "cargolike: bigoutput captured {} bytes (expected {})",
                output.stdout.len(),
                expected
            );
            if output.stdout.len() < expected {
                println!("cargolike: bigoutput TRUNCATED -- pipe buffer is too small for this");
            }
        }
        Err(err) => println!("cargolike: bigoutput Command FAILED: {err}"),
    }
}
