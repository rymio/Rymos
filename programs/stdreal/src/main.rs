#![feature(restricted_std)]

//! Real `std::fs`/`std::env`/`std::time`/`std::process` smoke test -- not a
//! `no_std` RYMOS program (no `rymos-user` dependency, no `rymos_main`).
//! Builds only via the special forked-toolchain `-Z build-std` invocation
//! documented in `docs/dev-environment.md` (category 6), not through the
//! normal `scripts/rymos-sdk.py` flow, so it's deliberately left out of
//! `rymos-packages.toml`/`autoexec.bat`.

use std::collections::HashMap;
use std::time::Instant;

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
    let later = Instant::now();
    println!("stdreal Instant ordering (later >= start): {}", later >= start);

    let mut map = HashMap::new();
    map.insert("a", 1);
    map.insert("b", 2);
    println!("stdreal HashMap sum: {}", map.values().sum::<i32>());

    println!("stdreal: ok");
}
