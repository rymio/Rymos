#![no_main]
#![no_std]

use rymos_user as rt;

const ROOT: &[u8] = b"pfs:fswalk";
const SUB: &[u8] = b"pfs:fswalk/sub";
const FILE: &[u8] = b"pfs:fswalk/sub/data.txt";
const RENAMED: &[u8] = b"pfs:fswalk/sub/renamed.txt";

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("fswalk: filesystem smoke test");

    cleanup();
    if !ensure_dir(ROOT) || !ensure_dir(SUB) {
        return 1;
    }

    if !write_new_file(FILE, b"alpha") {
        return 1;
    }
    if !append_file(FILE, b"+beta") {
        return 1;
    }
    if rt::File::options()
        .write(true)
        .create_new(true)
        .open(FILE)
        .is_some()
    {
        rt::println("fswalk: create_new unexpectedly opened existing file");
        return 1;
    }
    if rt::last_error() != rt::ERR_EXIST {
        rt::print("fswalk: create_new errno ");
        print_i32(rt::last_error());
        rt::write(b"\n");
        return 1;
    }
    if !rt::rename(FILE, RENAMED) {
        rt::println("fswalk: rename failed");
        return 1;
    }
    if !rt::chdir(b"pfs:/fswalk/sub") {
        rt::println("fswalk: chdir pfs:/fswalk/sub failed");
        return 1;
    }
    if !check_file(b"./renamed.txt", b"alpha+beta") {
        return 1;
    }
    if !check_file(b"../sub/./renamed.txt", b"alpha+beta") {
        return 1;
    }

    print_stat(b".");
    print_list(b".");
    let _ = rt::chdir(b"/");

    if !check_long_path() {
        return 1;
    }
    if !check_sparse_write() {
        return 1;
    }
    if !check_many_fds() {
        return 1;
    }

    rt::println("fswalk: ok");
    0
}

/// RYMFS5 raised PFS_NAME_MAX from 30 to 96 bytes; this path is well past
/// the old limit.
const LONG_PATH: &[u8] =
    b"pfs:fswalk/sub/a-rather-long-nested-file-name-past-the-old-thirty-byte-limit.txt";

fn check_long_path() -> bool {
    if !write_new_file(LONG_PATH, b"long-path-ok") {
        return false;
    }
    if !check_file(LONG_PATH, b"long-path-ok") {
        return false;
    }
    let _ = rt::unlink(LONG_PATH);
    true
}

/// Seeks past the current end of the file and writes there, leaving a gap
/// nobody ever wrote. Confirms the gap reads back as zero (not leftover
/// disk contents) and the tail write landed at the right offset.
fn check_sparse_write() -> bool {
    const PATH: &[u8] = b"pfs:fswalk/sub/sparse.bin";
    let Some(mut file) = rt::File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(PATH)
    else {
        rt::println("fswalk: sparse create failed");
        return false;
    };
    if file.write(b"head").is_none() {
        rt::println("fswalk: sparse head write failed");
        return false;
    }
    if file.seek(20).is_none() {
        rt::println("fswalk: sparse seek past eof failed");
        return false;
    }
    if file.write(b"tail").is_none() {
        rt::println("fswalk: sparse tail write failed");
        return false;
    }
    drop(file);

    let Some(mut file) = rt::File::open(PATH) else {
        rt::println("fswalk: sparse reopen failed");
        return false;
    };
    let mut buffer = [0xFFu8; 32];
    let Some(data) = file.read(&mut buffer) else {
        rt::println("fswalk: sparse read failed");
        return false;
    };
    let ok = data.len() == 24
        && &data[0..4] == b"head"
        && data[4..20].iter().all(|&byte| byte == 0)
        && &data[20..24] == b"tail";
    if !ok {
        rt::print("fswalk: sparse content mismatch, len=");
        rt::print_usize(data.len());
        rt::write(b"\n");
        return false;
    }
    let _ = rt::unlink(PATH);
    true
}

/// APP_FD_COUNT was raised from 8 to 32; opens more than the old limit at
/// once to prove it actually works, not just that the constant changed.
fn check_many_fds() -> bool {
    const COUNT: usize = 20;
    let mut files: [Option<rt::File>; COUNT] = core::array::from_fn(|_| None);
    let mut ok = true;

    for i in 0..COUNT {
        let mut buffer = [0u8; 32];
        let path = fd_name(&mut buffer, i);
        let Some(mut file) = rt::File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
        else {
            rt::print("fswalk: many-fd open failed at ");
            rt::print_usize(i);
            rt::write(b"\n");
            ok = false;
            break;
        };
        if file.write(&[b'0' + (i % 10) as u8]).is_none() {
            rt::print("fswalk: many-fd write failed at ");
            rt::print_usize(i);
            rt::write(b"\n");
            ok = false;
            break;
        }
        files[i] = Some(file);
    }

    for slot in &mut files {
        *slot = None;
    }
    if !ok {
        return false;
    }

    for i in 0..COUNT {
        let mut buffer = [0u8; 32];
        let path = fd_name(&mut buffer, i);
        let _ = rt::unlink(path);
    }
    rt::print("fswalk: opened ");
    rt::print_usize(COUNT);
    rt::print(" concurrent file descriptors ok\n");
    true
}

fn fd_name(buffer: &mut [u8; 32], index: usize) -> &[u8] {
    let prefix = b"pfs:fswalk/sub/fd";
    let mut len = prefix.len();
    buffer[..len].copy_from_slice(prefix);
    buffer[len] = b'0' + (index / 10) as u8;
    buffer[len + 1] = b'0' + (index % 10) as u8;
    len += 2;
    let suffix = b".txt";
    buffer[len..len + suffix.len()].copy_from_slice(suffix);
    len += suffix.len();
    &buffer[..len]
}

fn cleanup() {
    let _ = rt::chdir(b"/");
    let _ = rt::unlink(RENAMED);
    let _ = rt::unlink(FILE);
    let _ = rt::unlink(SUB);
    let _ = rt::unlink(ROOT);
}

fn ensure_dir(path: &[u8]) -> bool {
    if rt::mkdir(path) {
        return true;
    }
    if let Some(stat) = rt::stat(path) {
        if stat.kind == rt::STAT_KIND_DIR {
            return true;
        }
    }
    rt::print("fswalk: mkdir failed: ");
    rt::write(path);
    rt::write(b"\n");
    false
}

fn write_new_file(path: &[u8], data: &[u8]) -> bool {
    let Some(mut file) = rt::File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
    else {
        rt::print("fswalk: create failed: ");
        rt::write(path);
        rt::write(b"\n");
        return false;
    };
    write_all(&mut file, data)
}

fn append_file(path: &[u8], data: &[u8]) -> bool {
    let Some(mut file) = rt::File::options().append(true).create(true).open(path) else {
        rt::print("fswalk: append open failed: ");
        rt::write(path);
        rt::write(b"\n");
        return false;
    };
    write_all(&mut file, data)
}

fn write_all(file: &mut rt::File, data: &[u8]) -> bool {
    match file.write(data) {
        Some(written) if written == data.len() => true,
        _ => {
            rt::println("fswalk: short write");
            false
        }
    }
}

fn check_file(path: &[u8], expected: &[u8]) -> bool {
    let Some(mut file) = rt::File::open(path) else {
        rt::print("fswalk: open failed: ");
        rt::write(path);
        rt::write(b"\n");
        return false;
    };
    let mut buffer = [0u8; 64];
    let Some(data) = file.read(&mut buffer) else {
        rt::print("fswalk: read failed: ");
        rt::write(path);
        rt::write(b"\n");
        return false;
    };
    if data != expected {
        rt::print("fswalk: content mismatch: ");
        rt::write(data);
        rt::write(b"\n");
        return false;
    }
    true
}

fn print_stat(path: &[u8]) {
    let Some(stat) = rt::stat(path) else {
        rt::print("fswalk: stat failed: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };
    rt::print("fswalk stat ");
    rt::write(path);
    rt::print(" kind ");
    rt::print_usize(stat.kind as usize);
    rt::print(" size ");
    rt::print_usize(stat.size);
    rt::print(" mode ");
    rt::print_usize(stat.mode as usize);
    rt::print(" created ");
    rt::print_usize(stat.created_ticks as usize);
    rt::print(" modified ");
    rt::print_usize(stat.modified_ticks as usize);
    rt::write(b"\n");
}

fn print_list(namespace: &[u8]) {
    let mut index = 0usize;
    loop {
        let mut name = [0u8; 64];
        let Some((entry, stat)) = rt::list(namespace, index, &mut name) else {
            break;
        };
        rt::print("fswalk list ");
        rt::write(entry);
        rt::print(" kind ");
        rt::print_usize(stat.kind as usize);
        rt::print(" size ");
        rt::print_usize(stat.size);
        rt::write(b"\n");
        index += 1;
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
