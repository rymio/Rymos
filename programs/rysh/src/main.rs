#![no_main]
#![no_std]

use rymos_user as rt;

const SCRIPT_SIZE: usize = 1024;
const MAX_VARS: usize = 8;
const NAME_SIZE: usize = 16;
const VALUE_SIZE: usize = 64;

struct VarStore {
    vars: [Var; MAX_VARS],
}

#[derive(Clone, Copy)]
struct Var {
    used: bool,
    name: [u8; NAME_SIZE],
    name_len: usize,
    value: [u8; VALUE_SIZE],
    value_len: usize,
}

impl Var {
    const fn empty() -> Self {
        Self {
            used: false,
            name: [0; NAME_SIZE],
            name_len: 0,
            value: [0; VALUE_SIZE],
            value_len: 0,
        }
    }
}

impl VarStore {
    const fn new() -> Self {
        Self {
            vars: [Var::empty(); MAX_VARS],
        }
    }

    fn set(&mut self, name: &[u8], value: &[u8]) {
        if name.is_empty() {
            return;
        }
        let index = self
            .find(name)
            .unwrap_or_else(|| self.free_slot().unwrap_or(0));
        let var = &mut self.vars[index];
        var.used = true;
        var.name_len = copy_bytes(name, &mut var.name);
        var.value_len = copy_bytes(value, &mut var.value);
    }

    fn get(&self, name: &[u8]) -> Option<&[u8]> {
        let index = self.find(name)?;
        let var = &self.vars[index];
        Some(&var.value[..var.value_len])
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        for index in 0..self.vars.len() {
            let var = &self.vars[index];
            if var.used && same_bytes(&var.name[..var.name_len], name) {
                return Some(index);
            }
        }
        None
    }

    fn free_slot(&self) -> Option<usize> {
        for index in 0..self.vars.len() {
            if !self.vars[index].used {
                return Some(index);
            }
        }
        None
    }
}

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    let mut args_buffer = [0u8; 96];
    let args = rt::args(&mut args_buffer);
    let script_path = first_word(args).unwrap_or(b"build/demo.rym");

    rt::print("rysh: ");
    rt::write(script_path);
    rt::write(b"\n");

    let mut script = [0u8; SCRIPT_SIZE];
    let Some(data) = rt::file_read(script_path, &mut script) else {
        rt::print("rysh: script not found: ");
        rt::write(script_path);
        rt::write(b"\n");
        return 1;
    };

    let mut store = VarStore::new();
    run_script(data, args, &mut store);
    0
}

fn run_script(script: &[u8], args: &[u8], store: &mut VarStore) {
    let mut start = 0usize;
    let mut line_no = 1usize;
    for index in 0..=script.len() {
        if index == script.len() || script[index] == b'\n' {
            let line = trim_cr(trim(&script[start..index]));
            run_line(line, line_no, args, store);
            start = index + 1;
            line_no += 1;
        }
    }
}

fn run_line(line: &[u8], line_no: usize, args: &[u8], store: &mut VarStore) {
    if line.is_empty() || line[0] == b'#' {
        return;
    }

    let (command, rest) = split_word(line);
    match command {
        b"print" => {
            print_expanded(rest, args, store);
            rt::write(b"\n");
        }
        b"write" => {
            print_expanded(rest, args, store);
        }
        b"pid" => {
            rt::print("pid ");
            rt::print_usize(rt::pid() as usize);
            rt::write(b"\n");
        }
        b"args" => {
            rt::print("args ");
            if args.is_empty() {
                rt::print("<none>");
            } else {
                rt::write(args);
            }
            rt::write(b"\n");
        }
        b"set" => {
            let (name, value) = split_word(rest);
            store.set(name, value);
        }
        b"get" => {
            if let Some(value) = store.get(rest) {
                rt::write(value);
                rt::write(b"\n");
            }
        }
        b"cat" => {
            cat(rest);
        }
        b"writefile" => {
            let (path, text) = split_word(rest);
            write_file(path, text);
        }
        b"stat" => {
            stat_path(rest);
        }
        b"listdir" => {
            list_dir(rest);
        }
        b"mkdir" => {
            mkdir(rest);
        }
        b"fillfile" => {
            let (path, count) = split_word(rest);
            fill_file(path, count);
        }
        b"countfile" => {
            count_file(rest);
        }
        b"env" => {
            env();
        }
        b"getenv" => {
            getenv(rest);
        }
        b"spawn" => {
            let (name, args) = split_word(rest);
            spawn(name, args);
        }
        b"wait" => {
            wait_process(rest);
        }
        b"time" => {
            time_ticks();
        }
        b"stdio" => {
            stdio();
        }
        b"rm" => {
            unlink(rest);
        }
        b"rename" => {
            let (old_path, new_path) = split_word(rest);
            rename(old_path, new_path);
        }
        _ => {
            rt::print("rysh: unknown command at line ");
            rt::print_usize(line_no);
            rt::print(": ");
            rt::write(command);
            rt::write(b"\n");
        }
    }
}

fn unlink(path: &[u8]) {
    if !rt::unlink(path) {
        rt::print("rysh: rm failed: ");
        rt::write(path);
        rt::write(b"\n");
    }
}

fn rename(old_path: &[u8], new_path: &[u8]) {
    if !rt::rename(old_path, new_path) {
        rt::print("rysh: rename failed: ");
        rt::write(old_path);
        rt::print(" ");
        rt::write(new_path);
        rt::write(b"\n");
    }
}

fn time_ticks() {
    rt::print("ticks ");
    rt::print_usize(rt::time_ticks() as usize);
    rt::write(b"\n");
}

fn stdio() {
    let _ = rt::fd_write(rt::STDOUT, b"stdout fd1 ok\n");
    let _ = rt::fd_write(rt::STDERR, b"stderr fd2 ok\n");
}

fn spawn(name: &[u8], args: &[u8]) {
    match rt::spawn(name, args) {
        Ok(pid) => {
            rt::print("spawned pid ");
            rt::print_usize(pid as usize);
            rt::write(b"\n");
        }
        Err(-2) => {
            rt::print("rysh: spawn waits for isolated app loading\n");
        }
        Err(code) => {
            rt::print("rysh: spawn failed ");
            print_i32(code);
            rt::write(b"\n");
        }
    }
}

fn wait_process(pid: &[u8]) {
    let Some(pid) = parse_u32(pid) else {
        rt::print("rysh: invalid pid\n");
        return;
    };
    let Some(status) = rt::wait(pid) else {
        rt::print("rysh: wait failed: ");
        rt::print_usize(pid as usize);
        rt::write(b"\n");
        return;
    };
    rt::print("pid ");
    rt::print_usize(pid as usize);
    rt::print(" ");
    rt::print(process_state_name(status.state));
    rt::print(" exit ");
    print_i32(status.exit_code);
    rt::write(b"\n");
}

fn process_state_name(state: u32) -> &'static str {
    match state {
        rt::PROCESS_READY => "ready",
        rt::PROCESS_RUNNING => "running",
        rt::PROCESS_EXITED => "exited",
        rt::PROCESS_FAILED => "failed",
        _ => "empty",
    }
}

fn env() {
    let mut index = 0usize;
    loop {
        let mut key = [0u8; 32];
        let mut value = [0u8; 96];
        let Some((key, value)) = rt::env_list(index, &mut key, &mut value) else {
            break;
        };
        rt::write(key);
        rt::print("=");
        rt::write(value);
        rt::write(b"\n");
        index += 1;
    }
}

fn getenv(key: &[u8]) {
    let mut value = [0u8; 96];
    let Some(value) = rt::env_get(key, &mut value) else {
        rt::print("rysh: env not found: ");
        rt::write(key);
        rt::write(b"\n");
        return;
    };
    rt::write(key);
    rt::print("=");
    rt::write(value);
    rt::write(b"\n");
}

fn fill_file(path: &[u8], count: &[u8]) {
    let Some(total) = parse_usize(count) else {
        rt::print("rysh: invalid count\n");
        return;
    };
    let Some(mut file) = rt::File::create(path) else {
        rt::print("rysh: open failed: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };

    let mut written_total = 0usize;
    let mut chunk = [0u8; 256];
    while written_total < total {
        let count = core::cmp::min(chunk.len(), total - written_total);
        for index in 0..count {
            chunk[index] = b'A' + ((written_total + index) % 26) as u8;
        }
        let Some(written) = file.write(&chunk[..count]) else {
            rt::print("rysh: write failed: ");
            rt::write(path);
            rt::write(b"\n");
            return;
        };
        if written != count {
            rt::print("rysh: short write: ");
            rt::write(path);
            rt::write(b"\n");
            return;
        }
        written_total += written;
    }

    rt::print("wrote ");
    rt::print_usize(written_total);
    rt::write(b"\n");
}

fn count_file(path: &[u8]) {
    let Some(mut file) = rt::File::open(path) else {
        rt::print("rysh: file not found: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };
    let mut total = 0usize;
    let mut buffer = [0u8; 512];
    loop {
        let Some(data) = file.read(&mut buffer) else {
            rt::print("rysh: read failed: ");
            rt::write(path);
            rt::write(b"\n");
            return;
        };
        if data.is_empty() {
            break;
        }
        total += data.len();
    }
    rt::print("count ");
    rt::write(path);
    rt::print(" ");
    rt::print_usize(total);
    rt::write(b"\n");
}

fn mkdir(path: &[u8]) {
    if !rt::mkdir(path) {
        rt::print("rysh: mkdir failed: ");
        rt::write(path);
        rt::write(b"\n");
    }
}

fn stat_path(path: &[u8]) {
    let Some(stat) = rt::stat(path) else {
        rt::print("rysh: stat failed: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };
    rt::write(path);
    rt::print(" ");
    print_stat(stat);
    rt::write(b"\n");
}

fn list_dir(namespace: &[u8]) {
    let mut index = 0usize;
    loop {
        let mut name = [0u8; 64];
        let Some((entry_name, stat)) = rt::list(namespace, index, &mut name) else {
            break;
        };
        rt::write(entry_name);
        rt::print(" ");
        print_stat(stat);
        rt::write(b"\n");
        index += 1;
    }
}

fn print_stat(stat: rt::Stat) {
    if stat.fs == rt::STAT_FS_PFS {
        rt::print("pfs ");
    } else if stat.fs == rt::STAT_FS_BOOTFS {
        rt::print("bootfs ");
    } else {
        rt::print("fs? ");
    }
    if stat.kind == rt::STAT_KIND_DIR {
        rt::print("dir ");
    } else {
        rt::print("file ");
    }
    rt::print_usize(stat.size);
    rt::print(" B");
}

fn write_file(path: &[u8], text: &[u8]) {
    let Some(mut file) = rt::File::create(path) else {
        rt::print("rysh: open failed: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };
    let Some(written) = file.write(text) else {
        rt::print("rysh: write failed: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };
    if written != text.len() {
        rt::print("rysh: short write: ");
        rt::write(path);
        rt::write(b"\n");
    }
}

fn print_expanded(mut bytes: &[u8], args: &[u8], store: &VarStore) {
    while !bytes.is_empty() {
        if bytes[0] == b'$' {
            let name_end = var_name_end(&bytes[1..]) + 1;
            let name = &bytes[1..name_end];
            if same_bytes(name, b"pid") {
                rt::print_usize(rt::pid() as usize);
            } else if same_bytes(name, b"args") {
                rt::write(args);
            } else if let Some(value) = store.get(name) {
                rt::write(value);
            }
            bytes = &bytes[name_end..];
        } else {
            rt::write(&bytes[..1]);
            bytes = &bytes[1..];
        }
    }
}

fn cat(path: &[u8]) {
    let mut buffer = [0u8; 512];
    let Some(mut file) = rt::File::open(path) else {
        rt::print("rysh: file not found: ");
        rt::write(path);
        rt::write(b"\n");
        return;
    };

    let mut wrote_any = false;
    loop {
        let Some(data) = file.read(&mut buffer) else {
            rt::print("rysh: read failed: ");
            rt::write(path);
            rt::write(b"\n");
            return;
        };
        if data.is_empty() {
            break;
        }
        rt::write(data);
        wrote_any = true;
    }
    if wrote_any {
        rt::write(b"\n");
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

fn split_word(bytes: &[u8]) -> (&[u8], &[u8]) {
    let bytes = trim(bytes);
    for index in 0..bytes.len() {
        if bytes[index] == b' ' || bytes[index] == b'\t' {
            return (&bytes[..index], trim(&bytes[index + 1..]));
        }
    }
    (bytes, b"")
}

fn first_word(bytes: &[u8]) -> Option<&[u8]> {
    let bytes = trim(bytes);
    if bytes.is_empty() {
        return None;
    }
    Some(split_word(bytes).0)
}

fn parse_usize(bytes: &[u8]) -> Option<usize> {
    let bytes = trim(bytes);
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0usize;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?;
        value = value.checked_add((byte - b'0') as usize)?;
    }
    Some(value)
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    let value = parse_usize(bytes)?;
    if value > u32::MAX as usize {
        None
    } else {
        Some(value as u32)
    }
}

fn var_name_end(bytes: &[u8]) -> usize {
    for index in 0..bytes.len() {
        let byte = bytes[index];
        let valid = byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
        if !valid {
            return index;
        }
    }
    bytes.len()
}

fn trim(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = bytes.split_first() {
        if first == b' ' || first == b'\t' {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = bytes.split_last() {
        if last == b' ' || last == b'\t' {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

fn trim_cr(bytes: &[u8]) -> &[u8] {
    if bytes.ends_with(b"\r") {
        &bytes[..bytes.len() - 1]
    } else {
        bytes
    }
}

fn copy_bytes(source: &[u8], dest: &mut [u8]) -> usize {
    let len = core::cmp::min(source.len(), dest.len());
    dest[..len].copy_from_slice(&source[..len]);
    len
}

fn same_bytes(left: &[u8], right: &[u8]) -> bool {
    left == right
}
