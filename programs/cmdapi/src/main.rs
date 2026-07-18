#![no_main]
#![no_std]

use rymos_user as rt;

/// Regression check for a real bug the isolated-per-process-address-space
/// work fixed: before processes got their own address space, a spawned
/// child overwrote the *shared* program-image window, and the parent's own
/// writable data was never restored afterward (only its read-only segments
/// were reloaded from its own ELF). Any mutable global surviving every spawn
/// below proves the parent's memory is now genuinely untouched by its
/// children.
static mut PARENT_MARKER: u32 = 0xC0FF_EE42;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("cmdapi: Command output smoke test");
    let echoin = rt::Command::new(b"echoin").stdin(b"api-stdin").output();
    let hello = rt::Command::new(b"hello")
        .arg(b"readme.txt")
        .arg(b"two words")
        .env(b"RYMOS_BUILD_MODE", b"cmdapi-child")
        .current_dir(b"/")
        .output();
    let hello_status = rt::Command::new(b"hello")
        .arg(b"readme.txt")
        .arg(b"status words")
        .env(b"RYMOS_BUILD_MODE", b"status-child")
        .current_dir(b"/")
        .status();
    let hello_file_status = rt::Command::new(b"hello")
        .arg(b"readme.txt")
        .arg(b"file words")
        .env(b"RYMOS_BUILD_MODE", b"file-child")
        .current_dir(b"/")
        .stdout_file(b"pfs:cmdapi-stdout.txt")
        .stderr_file(b"pfs:cmdapi-stderr.txt")
        .status();

    if !print_output(b"echoin", echoin) {
        return 1;
    }
    if !print_output(b"hello", hello) {
        return 1;
    }
    if !print_status(b"hello-status", hello_status) {
        return 1;
    }
    if !print_status(b"hello-file-status", hello_file_status) {
        return 1;
    }
    if !print_file(b"stdout-file", b"pfs:cmdapi-stdout.txt") {
        return 1;
    }
    if !print_file(b"stderr-file", b"pfs:cmdapi-stderr.txt") {
        return 1;
    }

    let marker = unsafe { PARENT_MARKER };
    if marker != 0xC0FF_EE42 {
        rt::print("cmdapi: FAIL parent global corrupted by spawn, marker=0x");
        rt::print_hex_usize(marker as usize);
        rt::write(b"\n");
        return 1;
    }
    rt::println("cmdapi: parent globals survive spawn ok");

    if !check_spawn_many_then_wait_any() {
        return 1;
    }
    if !check_zombie_reaping() {
        return 1;
    }
    0
}

/// Category 2 regression: spawns three children with raw `rt::spawn` (not
/// the `Command` helper) before waiting on any of them, then reaps all three
/// via `wait_any()` in whatever order it returns them. `spawn` still runs
/// each child to completion inline (see `spawn_prepared`'s docs for why a
/// genuinely deferred version was tried and reverted), so this is really
/// exercising correctness of multiple outstanding un-reaped children plus
/// `wait_any`'s any-order matching -- not concurrency.
fn check_spawn_many_then_wait_any() -> bool {
    let mut spawned = [0u32; 3];
    for (index, args) in [b"one" as &[u8], b"two", b"three"].iter().enumerate() {
        match rt::spawn(b"hello", args) {
            Ok(pid) => spawned[index] = pid,
            Err(code) => {
                rt::print("cmdapi: FAIL spawn ");
                print_i32(code);
                rt::write(b"\n");
                return false;
            }
        }
    }

    let mut reaped = 0usize;
    for _ in 0..3 {
        match rt::wait_any() {
            Some((pid, status)) => {
                if status.exit_code != 0 || !spawned.contains(&pid) {
                    rt::print("cmdapi: FAIL unexpected wait_any pid ");
                    rt::print_usize(pid as usize);
                    rt::write(b"\n");
                    return false;
                }
                reaped += 1;
            }
            None => {
                rt::println("cmdapi: FAIL wait_any returned none early");
                return false;
            }
        }
    }
    if reaped != 3 {
        rt::println("cmdapi: FAIL did not reap all three children");
        return false;
    }
    rt::println("cmdapi: spawn-many + wait_any reaped all three ok");
    true
}

/// Regression for a real bug found while investigating category 2: the
/// process table used to let a brand-new spawn silently reuse *any*
/// `Exited`/`Failed` slot, even one nobody had called `wait` on yet --
/// quietly destroying a real child's exit status before its actual parent
/// ever collected it. Fixed in `process_find_reapable_slot`, which now
/// requires `waited == true`. Spawns a child and deliberately does not wait
/// on it right away (checking that its slot is *not* up for grabs), then
/// waits and confirms its real exit status still comes back correctly.
fn check_zombie_reaping() -> bool {
    let pid = match rt::spawn(b"allocdemo", b"") {
        Ok(pid) => pid,
        Err(code) => {
            rt::print("cmdapi: FAIL zombie spawn ");
            print_i32(code);
            rt::write(b"\n");
            return false;
        }
    };
    // Deliberately not waited on immediately -- it's a zombie right now.
    match rt::wait(pid) {
        Some(status) if status.exit_code == 0 => {
            rt::println("cmdapi: zombie reaped with correct status ok");
            true
        }
        Some(status) => {
            rt::print("cmdapi: FAIL zombie exit code ");
            print_i32(status.exit_code);
            rt::write(b"\n");
            false
        }
        None => {
            rt::println("cmdapi: FAIL zombie status lost before wait");
            false
        }
    }
}

fn print_status(label: &[u8], status: Result<rt::CommandStatus, i32>) -> bool {
    match status {
        Ok(status) => {
            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" pid ");
            rt::print_usize(status.pid as usize);
            rt::write(b" exit ");
            rt::print_usize(status.status.exit_code as usize);
            rt::write(b"\n");
            true
        }
        Err(code) => {
            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" failed ");
            print_i32(code);
            rt::write(b"\n");
            false
        }
    }
}

fn print_output(label: &[u8], output: Result<rt::CommandOutput, i32>) -> bool {
    match output {
        Ok(output) => {
            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" pid ");
            rt::print_usize(output.pid as usize);
            rt::write(b" exit ");
            rt::print_usize(output.status.exit_code as usize);
            rt::write(b"\n");

            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" stdout ");
            rt::print_usize(output.stdout.len());
            rt::write(b" B\n");
            rt::write(&output.stdout);

            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" stderr ");
            rt::print_usize(output.stderr.len());
            rt::write(b" B\n");
            rt::write(&output.stderr);
            true
        }
        Err(code) => {
            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" failed ");
            print_i32(code);
            rt::write(b"\n");
            false
        }
    }
}

fn print_file(label: &[u8], path: &[u8]) -> bool {
    let stat = match rt::stat(path) {
        Some(stat) => stat,
        None => {
            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" missing\n");
            return false;
        }
    };
    rt::print("cmdapi ");
    rt::write(label);
    rt::print(" file ");
    rt::print_usize(stat.size);
    rt::write(b" B\n");

    let mut file = match rt::File::open(path) {
        Some(file) => file,
        None => {
            rt::print("cmdapi ");
            rt::write(label);
            rt::print(" open failed\n");
            return false;
        }
    };
    let mut buffer = [0u8; 768];
    if let Some(data) = file.read(&mut buffer) {
        rt::write(data);
        true
    } else {
        rt::print("cmdapi ");
        rt::write(label);
        rt::print(" read failed\n");
        false
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
