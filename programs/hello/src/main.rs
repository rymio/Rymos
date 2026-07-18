#![no_main]
#![no_std]

use rymos_user as rt;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("hello from a Rust RYMOS program");
    rt::print("ABI version: ");
    rt::print_usize(rt::abi_version() as usize);
    rt::write(b"\n");
    rt::print("pid: ");
    rt::print_usize(rt::pid() as usize);
    rt::write(b"\n");

    let mut args_buffer = [0u8; 64];
    let args = rt::args(&mut args_buffer);
    rt::print("args: ");
    if args.is_empty() {
        rt::println("<none>");
    } else {
        rt::write(args);
        rt::write(b"\n");
    }
    print_argv();
    print_build_mode();

    let path = if args.is_empty() {
        b"readme.txt".as_slice()
    } else {
        first_word(args)
    };
    print_file(path);

    0
}

fn print_argv() {
    let count = rt::argv_count();
    rt::print("argv count: ");
    rt::print_usize(count);
    rt::write(b"\n");
    for index in 0..count {
        let mut buffer = [0u8; 64];
        if let Some(arg) = rt::argv(index, &mut buffer) {
            rt::print("argv[");
            rt::print_usize(index);
            rt::print("]: ");
            rt::write(arg);
            rt::write(b"\n");
        }
    }
}

fn print_build_mode() {
    let mut value = [0u8; 96];
    if let Some(mode) = rt::env_get(b"RYMOS_BUILD_MODE", &mut value) {
        rt::print("build mode: ");
        rt::write(mode);
        rt::write(b"\n");
    }
}

fn print_file(path: &[u8]) {
    let Some(size) = rt::file_size(path) else {
        rt::print("file not found: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };

    rt::print("reading ");
    rt::write(path);
    rt::print(" (");
    rt::print_usize(size);
    rt::println(" bytes)");

    let mut buffer = [0u8; 256];
    if let Some(data) = rt::file_read(path, &mut buffer) {
        rt::write(data);
        rt::write(b"\n");
    }
}

fn first_word(bytes: &[u8]) -> &[u8] {
    for index in 0..bytes.len() {
        if bytes[index] == b' ' || bytes[index] == b'\t' {
            return &bytes[..index];
        }
    }
    bytes
}
