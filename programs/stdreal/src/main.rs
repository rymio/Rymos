#![feature(restricted_std)]

//! Real `std::fs`/`std::env`/`std::time`/`std::process` smoke test -- not a
//! `no_std` RYMOS program (no `rymos-user` dependency, no `rymos_main`).
//! Builds only via the special forked-toolchain `-Z build-std` invocation
//! documented in `docs/dev-environment.md` (category 6), not through the
//! normal `scripts/rymos-sdk.py` flow, so it's deliberately left out of
//! `rymos-packages.toml`/`autoexec.bat`.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() {
    println!("stdreal: real std::fs/env/time/process smoke test");

    println!("stdreal pid: {}", std::process::id());

    let args: Vec<String> = std::env::args().collect();
    println!("stdreal argv count: {}", args.len());
    for (index, arg) in args.iter().enumerate() {
        println!("stdreal argv[{index}]: {arg}");
    }

    unsafe {
        std::env::set_var("STDREAL_VAR", "hello-from-std-env");
    }
    match std::env::var("STDREAL_VAR") {
        Ok(value) => println!("stdreal env get: {value}"),
        Err(err) => println!("stdreal env get FAILED: {err}"),
    }
    let mut saw_it = false;
    for (key, value) in std::env::vars() {
        if key == "STDREAL_VAR" {
            saw_it = true;
            println!("stdreal env iterate found: {key}={value}");
        }
    }
    if !saw_it {
        println!("stdreal env iterate FAILED: did not find STDREAL_VAR");
    }
    unsafe {
        std::env::remove_var("STDREAL_VAR");
    }
    println!("stdreal env after remove: {:?}", std::env::var("STDREAL_VAR"));

    let cwd = std::env::current_dir();
    println!("stdreal cwd: {cwd:?}");
    println!("stdreal temp_dir: {:?}", std::env::temp_dir());

    let path = "pfs:stdreal-test.txt";
    match std::fs::write(path, b"hello from real std::fs\n") {
        Ok(()) => println!("stdreal fs::write ok"),
        Err(err) => println!("stdreal fs::write FAILED: {err}"),
    }
    match std::fs::read_to_string(path) {
        Ok(contents) => print!("stdreal fs::read_to_string: {contents}"),
        Err(err) => println!("stdreal fs::read_to_string FAILED: {err}"),
    }
    match std::fs::exists(path) {
        Ok(true) => println!("stdreal fs::exists ok"),
        Ok(false) => println!("stdreal fs::exists FAILED: reported missing"),
        Err(err) => println!("stdreal fs::exists FAILED: {err}"),
    }

    let dir = "pfs:stdreal-dir";
    let _ = std::fs::create_dir(dir);
    let nested = format!("{dir}/nested.txt");
    let _ = std::fs::write(&nested, b"nested\n");
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => println!("stdreal read_dir entry: {:?}", entry.file_name()),
                    Err(err) => println!("stdreal read_dir entry FAILED: {err}"),
                }
            }
        }
        Err(err) => println!("stdreal read_dir FAILED: {err}"),
    }
    let _ = std::fs::remove_file(&nested);
    let _ = std::fs::remove_dir(dir);
    let _ = std::fs::remove_file(path);

    let start = Instant::now();
    std::thread::sleep(Duration::from_millis(15));
    let later = Instant::now();
    println!("stdreal Instant ordering (later >= start): {}", later >= start);
    match later.checked_duration_since(start) {
        Some(elapsed) => println!("stdreal Instant elapsed: {elapsed:?} (slept 15ms)"),
        None => println!("stdreal Instant elapsed FAILED: checked_duration_since returned None"),
    }

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since_epoch) => println!(
            "stdreal SystemTime since epoch: {} s ({} days)",
            since_epoch.as_secs(),
            since_epoch.as_secs() / 86_400
        ),
        Err(err) => println!("stdreal SystemTime FAILED: {err}"),
    }

    let mut map = HashMap::new();
    map.insert("a", 1);
    map.insert("b", 2);
    println!("stdreal HashMap sum: {}", map.values().sum::<i32>());

    // Real std::process::Command: RYMOS's ABI runs a spawned child to
    // completion before returning (see sys::process::rymos's docs), so by
    // the time `output()`/`status()` get control back, the child's exit
    // status and full stdout/stderr are already final -- exactly what these
    // two methods need, unlike interactive `Child::stdin` writes.
    match std::process::Command::new("hello").arg("readme.txt").output() {
        Ok(output) => {
            println!(
                "stdreal Command::output status={:?} stdout_len={}",
                output.status.code(),
                output.stdout.len()
            );
        }
        Err(err) => println!("stdreal Command::output FAILED: {err}"),
    }

    match std::process::Command::new("hello")
        .arg("readme.txt")
        .env("STDREAL_CHILD_VAR", "child-env-value")
        .status()
    {
        Ok(status) => println!("stdreal Command::status success={}", status.success()),
        Err(err) => println!("stdreal Command::status FAILED: {err}"),
    }

    // The env override above must not leak into this (parent) process.
    match std::env::var("STDREAL_CHILD_VAR") {
        Ok(value) => println!("stdreal Command env leaked FAILED: {value}"),
        Err(_) => println!("stdreal Command env override correctly did not leak"),
    }

    println!("stdreal: ok");
}
