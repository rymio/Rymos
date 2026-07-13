#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use rymos_user as rt;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("allocdemo: liballoc smoke test");
    rt::print("pid: ");
    rt::print_usize(rt::pid() as usize);
    rt::write(b"\n");

    let mut numbers = Vec::new();
    for value in 0..128usize {
        numbers.push(value);
    }

    let mut message = String::from("vec-len=");
    push_usize(&mut message, numbers.len());
    message.push_str(" sum=");
    push_usize(&mut message, sum(&numbers));

    rt::println(&message);
    rt::print("mapped heap bytes: ");
    rt::print_usize(rt::heap_size());
    rt::write(b"\n");
    rt::print("heap base: ");
    rt::print_hex_usize(rt::heap_base());
    rt::write(b"\n");

    let _ = rt::mkdir(b"pfs:alloc");
    if let Some(mut file) = rt::File::create(b"pfs:alloc/report.txt") {
        let _ = file.write(message.as_bytes());
        let _ = file.write(b"\n");
        rt::println("allocdemo: wrote pfs:alloc/report.txt");
    } else {
        rt::println("allocdemo: pfs write skipped");
    }

    0
}

fn sum(numbers: &[usize]) -> usize {
    let mut total = 0usize;
    for number in numbers {
        total += *number;
    }
    total
}

fn push_usize(text: &mut String, mut value: usize) {
    let mut digits = [0u8; 20];
    let mut len = 0usize;

    if value == 0 {
        text.push('0');
        return;
    }

    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }

    while len > 0 {
        len -= 1;
        text.push(digits[len] as char);
    }
}
