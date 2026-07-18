#![no_main]
#![no_std]

use rymos_user as rt;

const PAGE_SIZE: usize = 4096;

/// Manual diagnostic tool, not part of the automated boot regression: it
/// deliberately crashes (that's the point) to prove the kernel's exception
/// handlers turn a fault into a clear serial diagnostic instead of a silent
/// QEMU reset. Run directly with `run faultcheck [mode]`; never wire it into
/// autoexec.bat, since a passing run halts the machine on purpose.
///
/// Modes: `guard-before` (default), `guard-after`, `divide`, `ud`. There is
/// also a `null` mode, kept deliberately even though it's a documented
/// non-fault: address 0 turned out to already be mapped (part of RYMOS's
/// low-memory identity mapping, not guarded), so a null-pointer write today
/// silently corrupts whatever's physically at address 0 instead of faulting
/// -- a real, separate gap from anything the exception handlers fixed here,
/// left for future memory-layout hardening rather than papered over.
#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    let mut buffer = [0u8; 32];
    let mode = rt::args(&mut buffer);

    rt::println("faultcheck: about to deliberately fault -- this should print a diagnostic and halt, not silently reboot");

    if mode == b"null" {
        rt::println("faultcheck: mode=null (writing through a null pointer -- known non-fault, see doc comment)");
        unsafe {
            (0 as *mut u64).write_volatile(0);
        }
    } else if mode == b"divide" {
        // Rust's own `/` always checks for a zero divisor and panics through
        // the ordinary panic handler before it ever reaches hardware -- to
        // actually exercise the CPU's #DE exception (and this kernel's new
        // handler for it) this has to issue a raw `div` instruction directly,
        // bypassing that check entirely.
        rt::println("faultcheck: mode=divide (raw hardware divide-by-zero, bypassing Rust's own check)");
        unsafe {
            core::arch::asm!(
                "xor edx, edx",
                "xor ecx, ecx",
                "div ecx",
                out("eax") _,
                out("edx") _,
                options(nostack),
            );
        }
    } else if mode == b"ud" {
        rt::println("faultcheck: mode=ud (raw invalid opcode)");
        unsafe {
            core::arch::asm!("ud2", options(nostack, noreturn));
        }
    } else if mode == b"guard-after" {
        rt::println("faultcheck: mode=guard-after (touching the trailing guard page)");
        touch_guard(true);
    } else {
        rt::println("faultcheck: mode=guard-before (touching the leading guard page)");
        touch_guard(false);
    }

    rt::println("faultcheck: FAIL -- execution continued past the deliberate fault");
    1
}

fn touch_guard(after: bool) {
    let pages = 4usize;
    let Some(address) = rt::mem_map_pages(pages, rt::MEM_MAP_GUARD) else {
        rt::println("faultcheck: mem_map_pages failed");
        return;
    };
    let guard_addr = if after {
        address + pages * PAGE_SIZE
    } else {
        address - PAGE_SIZE
    };
    rt::print("faultcheck: touching guard page at ");
    rt::print_hex_usize(guard_addr);
    rt::write(b"\n");
    unsafe {
        (guard_addr as *mut u64).write_volatile(0xDEAD_BEEF);
    }
}
