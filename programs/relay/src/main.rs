#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use rymos_user as rt;

/// Cargo-shaped nested-spawn stress test. `cargolike`'s `repeated_spawn_test`
/// only ever proved *sequential* spawns don't leak process-table slots --
/// it never had more than one `AppStateSnapshot` (the ambient
/// fds/pipes/cwd/env state `spawn`/`Command` save-and-restore around a
/// synchronous child) alive on the kernel's call stack at once. A real
/// cargo -> rustc -> linker invocation chain is *nested*: a child spawns its
/// own child before the outer `Command::output()` call returns, which stacks
/// snapshots on top of each other for as long as the chain is deep. Relay
/// recreates that by re-spawning itself `depth` times via `Command::output()`
/// before bottoming out by spawning `hello`, so a single top-level call (see
/// `cargolike`'s `nested_spawn_test`) drives `depth` snapshots live at once --
/// exactly the scenario raising `APP_PIPE_BUFFER_SIZE`/`APP_ENV_COUNT`
/// (see `docs/self-hosting.md`'s Recently Closed) needed to stay stack-safe
/// under, not just correct for one snapshot at a time.
#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    let mut args_buffer = [0u8; 64];
    let args = rt::args(&mut args_buffer);
    let depth: u32 = core::str::from_utf8(args)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);

    rt::print("relay depth=");
    rt::print_usize(depth as usize);
    rt::write(b"\n");

    let output = if depth == 0 {
        rt::Command::new(b"hello").arg(b"readme.txt").output()
    } else {
        let next = format!("{}", depth - 1);
        rt::Command::new(b"relay").arg(next.as_bytes()).output()
    };

    match output {
        Ok(output) => {
            rt::print("relay depth=");
            rt::print_usize(depth as usize);
            rt::print(" child exit=");
            rt::print_usize(output.status.exit_code as usize);
            rt::print(" child_stdout=");
            rt::print_usize(output.stdout.len());
            rt::println(" B");
            rt::write(&output.stdout);
            if output.status.exit_code != 0 { 1 } else { 0 }
        }
        Err(code) => {
            rt::print("relay depth=");
            rt::print_usize(depth as usize);
            rt::print(" spawn FAILED code=");
            print_i32(code);
            rt::write(b"\n");
            1
        }
    }
}

fn print_i32(value: i32) {
    if value < 0 {
        rt::write(b"-");
        rt::print_usize(value.wrapping_neg() as usize);
    } else {
        rt::print_usize(value as usize);
    }
}
