#![no_main]
#![no_std]

extern crate alloc;

use rt::stdish;
use rymos_user as rt;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("stdshim: std compatibility smoke test");

    let start = stdish::time::Instant::now();
    if stdish::io::stdout_write_all(b"stdshim io stdout ok\n").is_err() {
        return fail(b"io stdout");
    }

    if stdish::env::set_var(b"STDISH_MODE", b"smoke").is_err() {
        return fail(b"env set");
    }
    let Ok(mode) = stdish::env::var(b"STDISH_MODE") else {
        return fail(b"env get");
    };
    if mode != b"smoke" {
        return fail(b"env value");
    }

    let temp = stdish::env::temp_dir();
    if stdish::fs::create_dir_all(&temp).is_err() {
        return fail(b"temp mkdir");
    }
    let root = stdish::path::join(&temp, b"stdshim");
    if stdish::fs::create_dir_all(&root).is_err() {
        return fail(b"root mkdir");
    }
    let file = stdish::path::join(&root, b"out.txt");
    if stdish::fs::write(&file, b"alpha").is_err() {
        return fail(b"fs write");
    }
    if stdish::fs::append(&file, b"+beta").is_err() {
        return fail(b"fs append");
    }
    let Ok(data) = stdish::fs::read(&file) else {
        return fail(b"fs read");
    };
    if data != b"alpha+beta" {
        return fail(b"fs data");
    }

    let Ok(metadata) = stdish::fs::metadata(&file) else {
        return fail(b"metadata");
    };
    rt::print("stdshim file size ");
    rt::print_usize(metadata.size);
    rt::write(b"\n");

    let Ok(cwd_before) = stdish::env::current_dir() else {
        return fail(b"cwd get");
    };
    if stdish::env::set_current_dir(&root).is_err() {
        return fail(b"cwd set");
    }
    let Ok(relative) = stdish::fs::read(b"./out.txt") else {
        return fail(b"relative read");
    };
    if relative != data {
        return fail(b"relative data");
    }
    let _ = stdish::env::set_current_dir(&cwd_before);

    let mut entries = 0usize;
    for entry in stdish::fs::read_dir(&root) {
        let Ok(entry) = entry else {
            return fail(b"read_dir entry");
        };
        rt::print("stdshim dir ");
        rt::write(&entry.name);
        rt::print(" size ");
        rt::print_usize(entry.metadata.size);
        rt::write(b"\n");
        entries += 1;
    }
    if entries == 0 {
        return fail(b"read_dir empty");
    }

    let output = stdish::process::Command::new(b"hello")
        .arg(b"readme.txt")
        .env(b"RYMOS_BUILD_MODE", b"stdshim-child")
        .output();
    let Ok(output) = output else {
        return fail(b"command output");
    };
    rt::print("stdshim child ");
    rt::print_usize(output.pid as usize);
    rt::print(" stdout ");
    rt::print_usize(output.stdout.len());
    rt::write(b" B\n");

    let missing = stdish::fs::metadata(b"pfs:stdshim/missing.txt");
    if missing.is_ok() || rt::last_error() != rt::ERR_NOENT {
        return fail(b"errno");
    }
    stdish::env::remove_var(b"STDISH_MODE").ok();

    rt::print("stdshim elapsed ticks ");
    rt::print_usize(start.elapsed_ticks() as usize);
    rt::write(b"\n");
    rt::println("stdshim: ok");
    0
}

fn fail(label: &[u8]) -> i32 {
    rt::print("stdshim failed: ");
    rt::write(label);
    rt::print(" errno ");
    print_i32(rt::last_error());
    rt::write(b"\n");
    1
}

fn print_i32(value: i32) {
    if value < 0 {
        rt::write(b"-");
        rt::print_usize(value.wrapping_neg() as usize);
    } else {
        rt::print_usize(value as usize);
    }
}
