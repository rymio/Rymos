#![no_main]
#![no_std]

use rymos_user as rt;

/// Category 2 stage 3b verification: proves the timer tick actually drives
/// a real scheduling decision, not just plumbing (stage 3a). Without a
/// preemptive reschedule on tick, two fire-and-forget `spawn`s only ever
/// run sequentially -- a plain busy-sleep-and-print loop has no voluntary
/// yield point, so the first spawned child would hog the CPU until it
/// exits before the second ever got a single instruction. Under real
/// preemption, both children's ticks should interleave in the serial log
/// instead of running back to back.
#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    let mut args_buffer = [0u8; 32];
    let args = rt::args(&mut args_buffer);
    let text = core::str::from_utf8(args).unwrap_or("").trim();

    if text.is_empty() {
        run_parent()
    } else {
        run_child(text)
    }
}

fn run_parent() -> i32 {
    rt::println("preemptest: spawning two fire-and-forget daemons");
    let a = rt::spawn(b"preemptest", b"A 200");
    let b = rt::spawn(b"preemptest", b"B 200");

    let (Ok(pid_a), Ok(pid_b)) = (a, b) else {
        rt::println("preemptest: spawn failed");
        return 1;
    };

    let status_a = rt::wait(pid_a);
    let status_b = rt::wait(pid_b);

    let ok = matches!(status_a, Some(s) if s.exit_code == 0)
        && matches!(status_b, Some(s) if s.exit_code == 0);
    rt::println(if ok {
        "preemptest: both daemons exited 0"
    } else {
        "preemptest: a daemon failed"
    });
    if ok { 0 } else { 1 }
}

fn run_child(text: &str) -> i32 {
    let mut parts = text.split_whitespace();
    let tag = parts.next().unwrap_or("?");
    let count: u32 = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);

    for i in 0..count {
        rt::print(tag);
        rt::print(":");
        rt::print_usize(i as usize);
        rt::write(b"\n");
        // Pure busy-wait, no voluntary yield of any kind -- only real
        // preemption lets the other daemon run any of its own iterations
        // during this.
        rt::sleep_nanos(1_000_000);
    }
    0
}
