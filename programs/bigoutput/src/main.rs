#![no_main]
#![no_std]

use rymos_user as rt;

/// Deliberately prints well past the ABI pipe buffer's current size (see
/// `kernel/src/main.rs`'s `APP_PIPE_BUFFER_SIZE`) -- used by `cargolike` to
/// check whether `Command::output()` correctly captures large child output
/// the way a real rustc invocation's diagnostics routinely would, or
/// silently truncates/errors once the pipe fills up.
const LINE: &[u8] = b"bigoutput filler line 0123456789\n";
const LINE_COUNT: usize = 200; // 200 * 33 B ~= 6.6 KB, several times the 1 KB pipe buffer

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    for _ in 0..LINE_COUNT {
        rt::write(LINE);
    }
    rt::print("bigoutput: wrote ");
    rt::print_usize(LINE_COUNT * LINE.len());
    rt::write(b" bytes total\n");
    0
}
