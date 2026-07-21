#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use rymos_user as rt;

/// Category 2 stage 4's verification tool: a program meant to be started
/// via rysh's `daemon` builtin and left running in the background while
/// the shell keeps doing other things. Prints "<tag>:<i>" periodically,
/// each line built into one buffer before a single `write` call -- unlike
/// `preemptest` (which deliberately used several separate ABI calls per
/// line to prove raw scheduler interleaving), a real backgrounded program
/// should buffer its own output the same way real Unix programs do, so one
/// line survives another process's writes landing in between (see
/// `Console::write_bytes`'s docs for exactly what the kernel does and
/// doesn't guarantee here).
#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    let mut args_buffer = [0u8; 32];
    let args = rt::args(&mut args_buffer);
    let text = core::str::from_utf8(args).unwrap_or("").trim();
    let mut parts = text.split_whitespace();
    let tag = parts.next().unwrap_or("tick");
    let count: u32 = parts.next().and_then(|value| value.parse().ok()).unwrap_or(20);
    let sleep_ms: u64 = parts.next().and_then(|value| value.parse().ok()).unwrap_or(10);

    for i in 0..count {
        rt::print(&format!("{tag}:{i}\n"));
        rt::sleep_nanos(sleep_ms * 1_000_000);
    }
    0
}
