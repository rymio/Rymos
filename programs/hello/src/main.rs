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

    let path = if args.is_empty() {
        b"readme.txt".as_slice()
    } else {
        first_word(args)
    };
    print_file(path);

    0
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
