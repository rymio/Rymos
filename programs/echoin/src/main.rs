#![no_main]
#![no_std]

use rymos_user as rt;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    let mut buffer = [0u8; 128];
    rt::print("echoin pid ");
    rt::print_usize(rt::pid() as usize);
    rt::write(b"\n");
    let _ = rt::fd_write(rt::STDERR, b"echoin stderr ready\n");

    let Some(data) = rt::fd_read(rt::STDIN, &mut buffer) else {
        rt::print("echoin: stdin read failed\n");
        return 1;
    };

    rt::print("echoin stdin ");
    if data.is_empty() {
        rt::print("<empty>");
    } else {
        rt::write(data);
    }
    rt::write(b"\n");
    0
}
