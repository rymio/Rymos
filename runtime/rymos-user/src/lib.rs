#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::ptr::null_mut;

const PAGE_SIZE: usize = 4096;
const MIN_HEAP_CHUNK: usize = 64 * 1024;
pub const COMMAND_ARGS_MAX: usize = 64;

struct BumpAllocator {
    base: UnsafeCell<usize>,
    next: UnsafeCell<usize>,
    end: UnsafeCell<usize>,
    allocated: UnsafeCell<usize>,
}

unsafe impl Sync for BumpAllocator {}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {
    base: UnsafeCell::new(0),
    next: UnsafeCell::new(0),
    end: UnsafeCell::new(0),
    allocated: UnsafeCell::new(0),
};

#[repr(C)]
pub struct RymosAbi {
    pub version: u32,
    pub write: extern "sysv64" fn(*const u8, usize),
    pub pid: extern "sysv64" fn() -> u32,
    pub args: extern "sysv64" fn(*mut u8, usize) -> usize,
    pub read_line: extern "sysv64" fn(*mut u8, usize) -> usize,
    pub file_size: extern "sysv64" fn(*const u8, usize) -> isize,
    pub file_read: extern "sysv64" fn(*const u8, usize, *mut u8, usize) -> isize,
    pub open: extern "sysv64" fn(*const u8, usize, u32) -> i32,
    pub read: extern "sysv64" fn(i32, *mut u8, usize) -> isize,
    pub write_fd: extern "sysv64" fn(i32, *const u8, usize) -> isize,
    pub seek: extern "sysv64" fn(i32, usize) -> isize,
    pub close: extern "sysv64" fn(i32) -> i32,
    pub stat: extern "sysv64" fn(*const u8, usize, *mut Stat) -> i32,
    pub list: extern "sysv64" fn(*const u8, usize, usize, *mut u8, usize, *mut Stat) -> isize,
    pub mkdir: extern "sysv64" fn(*const u8, usize) -> i32,
    pub env_get: extern "sysv64" fn(*const u8, usize, *mut u8, usize) -> isize,
    pub env_list: extern "sysv64" fn(usize, *mut u8, usize, *mut u8, usize) -> isize,
    pub env_set: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub env_remove: extern "sysv64" fn(*const u8, usize) -> i32,
    pub spawn: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub spawn_argv: extern "sysv64" fn(*const u8, usize, *const ArgSlice, usize) -> i32,
    pub wait: extern "sysv64" fn(u32, *mut ProcessStatus) -> i32,
    pub wait_any: extern "sysv64" fn(*mut ProcessStatus) -> i32,
    pub mem_alloc_pages: extern "sysv64" fn(usize) -> u64,
    pub mem_map_pages: extern "sysv64" fn(usize, u32) -> u64,
    pub mem_unmap_pages: extern "sysv64" fn(u64, usize) -> i32,
    pub time_ticks: extern "sysv64" fn() -> u64,
    pub unlink: extern "sysv64" fn(*const u8, usize) -> i32,
    pub rename: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub cwd: extern "sysv64" fn(*mut u8, usize) -> isize,
    pub chdir: extern "sysv64" fn(*const u8, usize) -> i32,
    pub last_error: extern "sysv64" fn() -> i32,
    pub pipe: extern "sysv64" fn(*mut i32, *mut i32) -> i32,
    pub dup2: extern "sysv64" fn(i32, i32) -> i32,
    pub argv_count: extern "sysv64" fn() -> usize,
    pub argv_get: extern "sysv64" fn(usize, *mut u8, usize) -> isize,
}

static mut ABI: *const RymosAbi = core::ptr::null();

pub const OPEN_READ: u32 = 1;
pub const OPEN_WRITE: u32 = 2;
pub const OPEN_CREATE: u32 = 4;
pub const OPEN_TRUNCATE: u32 = 8;
pub const OPEN_APPEND: u32 = 16;
pub const OPEN_CREATE_NEW: u32 = 32;
pub const MEM_MAP_GUARD: u32 = 1;
pub const STDIN: i32 = 0;
pub const STDOUT: i32 = 1;
pub const STDERR: i32 = 2;
pub const STAT_KIND_FILE: u32 = 1;
pub const STAT_KIND_DIR: u32 = 2;
pub const STAT_FS_BOOTFS: u32 = 1;
pub const STAT_FS_PFS: u32 = 2;
pub const PROCESS_EMPTY: u32 = 0;
pub const PROCESS_READY: u32 = 1;
pub const PROCESS_RUNNING: u32 = 2;
pub const PROCESS_EXITED: u32 = 3;
pub const PROCESS_FAILED: u32 = 4;
pub const ERR_OK: i32 = 0;
pub const ERR_NOENT: i32 = 2;
pub const ERR_IO: i32 = 5;
pub const ERR_EXIST: i32 = 17;
pub const ERR_NOTDIR: i32 = 20;
pub const ERR_ISDIR: i32 = 21;
pub const ERR_INVAL: i32 = 22;
pub const ERR_NOSPC: i32 = 28;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat {
    pub kind: u32,
    pub fs: u32,
    pub size: usize,
    pub created_ticks: u64,
    pub modified_ticks: u64,
    pub mode: u32,
}

pub const MODE_READ: u32 = 0b001;
pub const MODE_WRITE: u32 = 0b010;
pub const MODE_EXEC: u32 = 0b100;

impl Stat {
    pub fn is_dir(&self) -> bool {
        self.kind == STAT_KIND_DIR
    }

    pub fn is_file(&self) -> bool {
        self.kind == STAT_KIND_FILE
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn readonly(&self) -> bool {
        self.mode & MODE_WRITE == 0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessStatus {
    pub state: u32,
    pub exit_code: i32,
}

pub struct File {
    fd: i32,
}

pub struct OpenOptions {
    flags: u32,
}

pub struct CommandOutput {
    pub pid: u32,
    pub status: ProcessStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct CommandStatus {
    pub pid: u32,
    pub status: ProcessStatus,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArgSlice {
    pub ptr: *const u8,
    pub len: usize,
}

pub struct Command<'a> {
    name: &'a [u8],
    args: Vec<u8>,
    argv: Vec<Vec<u8>>,
    stdin: &'a [u8],
    cwd: Option<&'a [u8]>,
    env: Vec<CommandEnv<'a>>,
    stdout_file: Option<&'a [u8]>,
    stderr_file: Option<&'a [u8]>,
    error: i32,
}

struct CommandEnv<'a> {
    key: &'a [u8],
    value: Option<&'a [u8]>,
}

struct EnvRestore {
    key: Vec<u8>,
    existed: bool,
    value: Vec<u8>,
}

impl<'a> Command<'a> {
    pub fn new(name: &'a [u8]) -> Self {
        Self {
            name,
            args: Vec::new(),
            argv: Vec::new(),
            stdin: b"",
            cwd: None,
            env: Vec::new(),
            stdout_file: None,
            stderr_file: None,
            error: ERR_OK,
        }
    }

    pub fn args(mut self, args: &[u8]) -> Self {
        self.args.clear();
        self.argv.clear();
        if args.len() > COMMAND_ARGS_MAX {
            self.error = ERR_NOSPC;
        } else {
            self.args.extend_from_slice(args);
        }
        self
    }

    pub fn args_raw(self, args: &[u8]) -> Self {
        self.args(args)
    }

    pub fn arg(mut self, arg: &[u8]) -> Self {
        if self.error != ERR_OK {
            return self;
        }
        let separator = usize::from(!self.args.is_empty());
        if self.args.len() + separator + arg.len() > COMMAND_ARGS_MAX {
            self.error = ERR_NOSPC;
            return self;
        }
        if separator == 1 {
            self.args.push(b' ');
        }
        self.args.extend_from_slice(arg);
        let mut stored = Vec::new();
        stored.extend_from_slice(arg);
        self.argv.push(stored);
        self
    }

    pub fn stdin(mut self, stdin: &'a [u8]) -> Self {
        self.stdin = stdin;
        self
    }

    pub fn current_dir(mut self, cwd: &'a [u8]) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn env(mut self, key: &'a [u8], value: &'a [u8]) -> Self {
        self.env.push(CommandEnv {
            key,
            value: Some(value),
        });
        self
    }

    pub fn env_remove(mut self, key: &'a [u8]) -> Self {
        self.env.push(CommandEnv { key, value: None });
        self
    }

    pub fn stdout_file(mut self, path: &'a [u8]) -> Self {
        self.stdout_file = Some(path);
        self
    }

    pub fn stderr_file(mut self, path: &'a [u8]) -> Self {
        self.stderr_file = Some(path);
        self
    }

    pub fn output(self) -> Result<CommandOutput, i32> {
        if self.error != ERR_OK {
            return Err(self.error);
        }
        run_command_output(
            self.name, &self.args, &self.argv, self.stdin, self.cwd, &self.env,
        )
    }

    pub fn status(self) -> Result<CommandStatus, i32> {
        if self.error != ERR_OK {
            return Err(self.error);
        }
        run_command_status(
            self.name,
            &self.args,
            &self.argv,
            self.stdin,
            self.cwd,
            &self.env,
            self.stdout_file,
            self.stderr_file,
        )
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let next = self.next.get();
        let end_ptr = self.end.get();
        let current = unsafe { *next };
        let heap_end = unsafe { *end_ptr };
        let aligned = align_up(current, layout.align());
        let Some(end) = aligned.checked_add(layout.size()) else {
            return null_mut();
        };
        if end <= heap_end {
            unsafe {
                *next = end;
            }
            return aligned as *mut u8;
        }

        let Some(chunk_size) = layout.size().checked_add(layout.align()) else {
            return null_mut();
        };
        let chunk_size = core::cmp::max(chunk_size, MIN_HEAP_CHUNK);
        let pages = align_up(chunk_size, PAGE_SIZE) / PAGE_SIZE;
        let Some(base) = mem_alloc_pages(pages) else {
            return null_mut();
        };
        let chunk_end = base + pages * PAGE_SIZE;
        let aligned = align_up(base, layout.align());
        let Some(end) = aligned.checked_add(layout.size()) else {
            return null_mut();
        };
        if end > chunk_end {
            return null_mut();
        }
        unsafe {
            if *self.base.get() == 0 {
                *self.base.get() = base;
            }
            *next = end;
            *end_ptr = chunk_end;
            *self.allocated.get() += pages * PAGE_SIZE;
        }
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

unsafe extern "Rust" {
    fn rymos_main() -> i32;
}

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(abi: *const RymosAbi) -> i32 {
    unsafe {
        ABI = abi;
        rymos_main()
    }
}

pub fn abi_version() -> u32 {
    with_abi(|abi| abi.version).unwrap_or(0)
}

pub fn pid() -> u32 {
    with_abi(|abi| (abi.pid)()).unwrap_or(0)
}

pub fn write(bytes: &[u8]) {
    let _ = with_abi(|abi| {
        if (abi.write_fd)(STDOUT, bytes.as_ptr(), bytes.len()) < 0 {
            (abi.write)(bytes.as_ptr(), bytes.len());
        }
    });
}

pub fn print(text: &str) {
    write(text.as_bytes());
}

pub fn println(text: &str) {
    print(text);
    write(b"\n");
}

pub fn print_usize(mut value: usize) {
    let mut digits = [0u8; 20];
    let mut len = 0usize;

    if value == 0 {
        write(b"0");
        return;
    }

    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }

    while len > 0 {
        len -= 1;
        write(&digits[len..len + 1]);
    }
}

pub fn print_hex_usize(value: usize) {
    write(b"0x");
    for shift in (0..usize::BITS).step_by(4).rev() {
        let digit = ((value >> shift) & 0x0F) as u8;
        let byte = if digit < 10 {
            b'0' + digit
        } else {
            b'A' + (digit - 10)
        };
        write(&[byte]);
    }
}

pub fn args(buffer: &mut [u8]) -> &[u8] {
    let len = with_abi(|abi| (abi.args)(buffer.as_mut_ptr(), buffer.len())).unwrap_or(0);
    &buffer[..core::cmp::min(len, buffer.len())]
}

pub fn argv_count() -> usize {
    with_abi(|abi| (abi.argv_count)()).unwrap_or(0)
}

pub fn argv<'a>(index: usize, buffer: &'a mut [u8]) -> Option<&'a [u8]> {
    let len = with_abi(|abi| (abi.argv_get)(index, buffer.as_mut_ptr(), buffer.len()))?;
    if len < 0 {
        None
    } else {
        Some(&buffer[..core::cmp::min(len as usize, buffer.len())])
    }
}

pub fn read_line(buffer: &mut [u8]) -> &[u8] {
    let len = with_abi(|abi| (abi.read_line)(buffer.as_mut_ptr(), buffer.len())).unwrap_or(0);
    &buffer[..core::cmp::min(len, buffer.len())]
}

pub fn file_size(path: &[u8]) -> Option<usize> {
    let size = with_abi(|abi| (abi.file_size)(path.as_ptr(), path.len()))?;
    if size < 0 { None } else { Some(size as usize) }
}

pub fn file_read<'a>(path: &[u8], buffer: &'a mut [u8]) -> Option<&'a [u8]> {
    let read = with_abi(|abi| {
        (abi.file_read)(path.as_ptr(), path.len(), buffer.as_mut_ptr(), buffer.len())
    })?;
    if read < 0 {
        None
    } else {
        Some(&buffer[..core::cmp::min(read as usize, buffer.len())])
    }
}

pub fn stat(path: &[u8]) -> Option<Stat> {
    let mut stat = Stat {
        kind: 0,
        fs: 0,
        size: 0,
        created_ticks: 0,
        modified_ticks: 0,
        mode: 0,
    };
    let ok = with_abi(|abi| (abi.stat)(path.as_ptr(), path.len(), &mut stat))?;
    if ok == 0 { Some(stat) } else { None }
}

pub fn list<'a>(namespace: &[u8], index: usize, name: &'a mut [u8]) -> Option<(&'a [u8], Stat)> {
    let mut stat = Stat {
        kind: 0,
        fs: 0,
        size: 0,
        created_ticks: 0,
        modified_ticks: 0,
        mode: 0,
    };
    let len = with_abi(|abi| {
        (abi.list)(
            namespace.as_ptr(),
            namespace.len(),
            index,
            name.as_mut_ptr(),
            name.len(),
            &mut stat,
        )
    })?;
    if len < 0 {
        None
    } else {
        Some((&name[..core::cmp::min(len as usize, name.len())], stat))
    }
}

pub fn mkdir(path: &[u8]) -> bool {
    with_abi(|abi| (abi.mkdir)(path.as_ptr(), path.len()) == 0).unwrap_or(false)
}

pub fn env_get<'a>(key: &[u8], value: &'a mut [u8]) -> Option<&'a [u8]> {
    let len =
        with_abi(|abi| (abi.env_get)(key.as_ptr(), key.len(), value.as_mut_ptr(), value.len()))?;
    if len < 0 {
        None
    } else {
        Some(&value[..core::cmp::min(len as usize, value.len())])
    }
}

pub fn env_list<'a>(
    index: usize,
    key: &'a mut [u8],
    value: &'a mut [u8],
) -> Option<(&'a [u8], &'a [u8])> {
    let packed = with_abi(|abi| {
        (abi.env_list)(
            index,
            key.as_mut_ptr(),
            key.len(),
            value.as_mut_ptr(),
            value.len(),
        )
    })?;
    if packed < 0 {
        return None;
    }
    let key_len = ((packed as usize) >> 32) & 0xFFFF_FFFF;
    let value_len = (packed as usize) & 0xFFFF_FFFF;
    Some((
        &key[..core::cmp::min(key_len, key.len())],
        &value[..core::cmp::min(value_len, value.len())],
    ))
}

pub fn env_set(key: &[u8], value: &[u8]) -> bool {
    with_abi(|abi| (abi.env_set)(key.as_ptr(), key.len(), value.as_ptr(), value.len()) == 0)
        .unwrap_or(false)
}

pub fn env_remove(key: &[u8]) -> bool {
    with_abi(|abi| (abi.env_remove)(key.as_ptr(), key.len()) == 0).unwrap_or(false)
}

pub fn spawn(name: &[u8], args: &[u8]) -> Result<u32, i32> {
    let pid = with_abi(|abi| (abi.spawn)(name.as_ptr(), name.len(), args.as_ptr(), args.len()))
        .unwrap_or(-1);
    if pid < 0 { Err(pid) } else { Ok(pid as u32) }
}

pub fn spawn_argv(name: &[u8], argv: &[ArgSlice]) -> Result<u32, i32> {
    let pid =
        with_abi(|abi| (abi.spawn_argv)(name.as_ptr(), name.len(), argv.as_ptr(), argv.len()))
            .unwrap_or(-1);
    if pid < 0 { Err(pid) } else { Ok(pid as u32) }
}

pub fn wait(pid: u32) -> Option<ProcessStatus> {
    let mut status = ProcessStatus {
        state: PROCESS_EMPTY,
        exit_code: 0,
    };
    let ok = with_abi(|abi| (abi.wait)(pid, &mut status))?;
    if ok == 0 { Some(status) } else { None }
}

pub fn wait_any() -> Option<(u32, ProcessStatus)> {
    let mut status = ProcessStatus {
        state: PROCESS_EMPTY,
        exit_code: 0,
    };
    let pid = with_abi(|abi| (abi.wait_any)(&mut status))?;
    if pid < 0 {
        None
    } else {
        Some((pid as u32, status))
    }
}

pub fn command_output(name: &[u8], args: &[u8], stdin: &[u8]) -> Result<CommandOutput, i32> {
    Command::new(name).args(args).stdin(stdin).output()
}

pub mod stdish {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Error {
        pub code: i32,
    }

    pub type Result<T> = core::result::Result<T, Error>;

    impl Error {
        pub fn last() -> Self {
            Self { code: last_error() }
        }

        pub const fn from_code(code: i32) -> Self {
            Self { code }
        }
    }

    pub mod fs {
        use super::*;

        pub type File = crate::File;
        pub type OpenOptions = crate::OpenOptions;
        pub type Metadata = crate::Stat;

        pub fn metadata(path: &[u8]) -> Result<Metadata> {
            crate::stat(path).ok_or_else(Error::last)
        }

        pub fn exists(path: &[u8]) -> bool {
            crate::stat(path).is_some()
        }

        pub fn create_dir(path: &[u8]) -> Result<()> {
            if crate::mkdir(path) {
                Ok(())
            } else {
                Err(Error::last())
            }
        }

        pub fn create_dir_all(path: &[u8]) -> Result<()> {
            let mut current = Vec::new();
            let mut rest = path;
            if rest.starts_with(b"pfs:") {
                current.extend_from_slice(b"pfs:");
                rest = &rest[4..];
                while rest.starts_with(b"/") {
                    rest = &rest[1..];
                }
            }
            for component in Components::new(rest) {
                if component == b"." {
                    continue;
                }
                if component == b".." {
                    return Err(Error::from_code(ERR_INVAL));
                }
                if !current.is_empty() && !current.ends_with(b":") {
                    current.push(b'/');
                }
                current.extend_from_slice(component);
                match crate::stat(&current) {
                    Some(stat) if stat.kind == STAT_KIND_DIR => {}
                    Some(_) => return Err(Error::from_code(ERR_NOTDIR)),
                    None => {
                        if !crate::mkdir(&current) {
                            return Err(Error::last());
                        }
                    }
                }
            }
            Ok(())
        }

        pub fn read(path: &[u8]) -> Result<Vec<u8>> {
            let mut file = crate::File::open(path).ok_or_else(Error::last)?;
            read_to_end(&mut file)
        }

        pub fn write(path: &[u8], data: &[u8]) -> Result<()> {
            let mut file = crate::File::create(path).ok_or_else(Error::last)?;
            write_all(&mut file, data)
        }

        pub fn append(path: &[u8], data: &[u8]) -> Result<()> {
            let mut file = crate::File::append(path).ok_or_else(Error::last)?;
            write_all(&mut file, data)
        }

        pub fn remove_file(path: &[u8]) -> Result<()> {
            if crate::unlink(path) {
                Ok(())
            } else {
                Err(Error::last())
            }
        }

        pub fn rename(from: &[u8], to: &[u8]) -> Result<()> {
            if crate::rename(from, to) {
                Ok(())
            } else {
                Err(Error::last())
            }
        }

        pub fn read_dir(path: &[u8]) -> ReadDir<'_> {
            ReadDir { path, index: 0 }
        }

        pub fn read_to_end(file: &mut crate::File) -> Result<Vec<u8>> {
            let mut data = Vec::new();
            loop {
                let mut buffer = [0u8; 256];
                let chunk = file.read(&mut buffer).ok_or_else(Error::last)?;
                if chunk.is_empty() {
                    break;
                }
                data.extend_from_slice(chunk);
            }
            Ok(data)
        }

        pub fn write_all(file: &mut crate::File, mut data: &[u8]) -> Result<()> {
            while !data.is_empty() {
                let written = file.write(data).ok_or_else(Error::last)?;
                if written == 0 || written > data.len() {
                    return Err(Error::from_code(ERR_IO));
                }
                data = &data[written..];
            }
            Ok(())
        }

        pub struct DirEntry {
            pub name: Vec<u8>,
            pub metadata: Metadata,
        }

        pub struct ReadDir<'a> {
            path: &'a [u8],
            index: usize,
        }

        impl<'a> Iterator for ReadDir<'a> {
            type Item = Result<DirEntry>;

            fn next(&mut self) -> Option<Self::Item> {
                let mut name = [0u8; 64];
                let Some((entry, metadata)) = crate::list(self.path, self.index, &mut name) else {
                    return None;
                };
                self.index += 1;
                let mut owned = Vec::new();
                owned.extend_from_slice(entry);
                Some(Ok(DirEntry {
                    name: owned,
                    metadata,
                }))
            }
        }

        struct Components<'a> {
            bytes: &'a [u8],
            index: usize,
        }

        impl<'a> Components<'a> {
            fn new(bytes: &'a [u8]) -> Self {
                Self { bytes, index: 0 }
            }
        }

        impl<'a> Iterator for Components<'a> {
            type Item = &'a [u8];

            fn next(&mut self) -> Option<Self::Item> {
                while self.index < self.bytes.len() && self.bytes[self.index] == b'/' {
                    self.index += 1;
                }
                if self.index >= self.bytes.len() {
                    return None;
                }
                let start = self.index;
                while self.index < self.bytes.len() && self.bytes[self.index] != b'/' {
                    self.index += 1;
                }
                Some(&self.bytes[start..self.index])
            }
        }
    }

    pub mod env {
        use super::*;

        pub fn var(key: &[u8]) -> Result<Vec<u8>> {
            let mut value = [0u8; 128];
            let value = crate::env_get(key, &mut value).ok_or_else(Error::last)?;
            let mut owned = Vec::new();
            owned.extend_from_slice(value);
            Ok(owned)
        }

        pub fn set_var(key: &[u8], value: &[u8]) -> Result<()> {
            if crate::env_set(key, value) {
                Ok(())
            } else {
                Err(Error::last())
            }
        }

        pub fn remove_var(key: &[u8]) -> Result<()> {
            if crate::env_remove(key) {
                Ok(())
            } else {
                Err(Error::last())
            }
        }

        pub fn vars() -> Vars {
            Vars { index: 0 }
        }

        pub fn current_dir() -> Result<Vec<u8>> {
            let mut dir = [0u8; 96];
            let dir = crate::cwd(&mut dir).ok_or_else(Error::last)?;
            let mut owned = Vec::new();
            owned.extend_from_slice(dir);
            Ok(owned)
        }

        pub fn set_current_dir(path: &[u8]) -> Result<()> {
            if crate::chdir(path) {
                Ok(())
            } else {
                Err(Error::last())
            }
        }

        pub fn temp_dir() -> Vec<u8> {
            var(b"TMPDIR").unwrap_or_else(|_| {
                let mut dir = Vec::new();
                dir.extend_from_slice(b"pfs:tmp");
                dir
            })
        }

        pub struct Vars {
            index: usize,
        }

        impl Iterator for Vars {
            type Item = (Vec<u8>, Vec<u8>);

            fn next(&mut self) -> Option<Self::Item> {
                let mut key = [0u8; 32];
                let mut value = [0u8; 128];
                let Some((key, value)) = crate::env_list(self.index, &mut key, &mut value) else {
                    return None;
                };
                self.index += 1;
                let mut owned_key = Vec::new();
                let mut owned_value = Vec::new();
                owned_key.extend_from_slice(key);
                owned_value.extend_from_slice(value);
                Some((owned_key, owned_value))
            }
        }
    }

    pub mod io {
        use super::*;

        pub fn stdin_read(buffer: &mut [u8]) -> Result<&[u8]> {
            crate::fd_read(STDIN, buffer).ok_or_else(Error::last)
        }

        pub fn stdout_write_all(data: &[u8]) -> Result<()> {
            fd_write_all(STDOUT, data)
        }

        pub fn stderr_write_all(data: &[u8]) -> Result<()> {
            fd_write_all(STDERR, data)
        }

        pub fn fd_write_all(fd: i32, mut data: &[u8]) -> Result<()> {
            while !data.is_empty() {
                let written = crate::fd_write(fd, data).ok_or_else(Error::last)?;
                if written == 0 || written > data.len() {
                    return Err(Error::from_code(ERR_IO));
                }
                data = &data[written..];
            }
            Ok(())
        }
    }

    pub mod process {
        pub use crate::{Command, CommandOutput as Output, CommandStatus as ExitStatus};

        use super::*;

        pub fn id() -> u32 {
            crate::pid()
        }

        pub fn wait_any() -> Result<(u32, crate::ProcessStatus)> {
            crate::wait_any().ok_or_else(Error::last)
        }
    }

    pub mod time {
        #[derive(Clone, Copy)]
        pub struct Instant {
            ticks: u64,
        }

        impl Instant {
            pub fn now() -> Self {
                Self {
                    ticks: crate::time_ticks(),
                }
            }

            pub fn ticks(&self) -> u64 {
                self.ticks
            }

            pub fn elapsed_ticks(&self) -> u64 {
                crate::time_ticks().saturating_sub(self.ticks)
            }
        }

        pub fn ticks() -> u64 {
            crate::time_ticks()
        }
    }

    pub mod path {
        use super::*;

        pub fn join(base: &[u8], child: &[u8]) -> Vec<u8> {
            if child.starts_with(b"pfs:") || child.starts_with(b"/") {
                let mut out = Vec::new();
                out.extend_from_slice(child);
                return out;
            }
            let mut out = Vec::new();
            out.extend_from_slice(base);
            if !out.is_empty() && !out.ends_with(b":") && !out.ends_with(b"/") {
                out.push(b'/');
            }
            out.extend_from_slice(child);
            out
        }

        pub fn file_name(path: &[u8]) -> &[u8] {
            let mut index = path.len();
            while index > 0 {
                index -= 1;
                if path[index] == b'/' {
                    return &path[index + 1..];
                }
            }
            if path.starts_with(b"pfs:") {
                &path[4..]
            } else {
                path
            }
        }

        pub fn display_lossy(path: &[u8]) -> String {
            let mut out = String::new();
            for byte in path {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    out.push(*byte as char);
                } else {
                    out.push('?');
                }
            }
            out
        }
    }
}

fn run_command_output(
    name: &[u8],
    args: &[u8],
    argv: &[Vec<u8>],
    stdin: &[u8],
    command_cwd: Option<&[u8]>,
    env_overrides: &[CommandEnv],
) -> Result<CommandOutput, i32> {
    let mut saved_cwd_buffer = [0u8; 64];
    let saved_cwd_len = match command_cwd {
        Some(_) => {
            let current = cwd(&mut saved_cwd_buffer).ok_or_else(last_error)?;
            current.len()
        }
        None => 0,
    };

    if let Some(next_cwd) = command_cwd {
        if !chdir(next_cwd) {
            return Err(last_error());
        }
    }

    let env_restores = match apply_env_overrides(env_overrides) {
        Ok(restores) => restores,
        Err(code) => {
            restore_cwd(command_cwd, &saved_cwd_buffer[..saved_cwd_len]);
            return Err(code);
        }
    };

    let (stdin_read, stdin_write) = match pipe() {
        Some(pipe) => pipe,
        None => {
            let error = last_error();
            restore_command_context(
                command_cwd,
                &saved_cwd_buffer[..saved_cwd_len],
                &env_restores,
            );
            return Err(error);
        }
    };
    let (stdout_read, stdout_write) = match pipe() {
        Some(pipe) => pipe,
        None => {
            let error = last_error();
            close_many(&[stdin_read, stdin_write]);
            restore_command_context(
                command_cwd,
                &saved_cwd_buffer[..saved_cwd_len],
                &env_restores,
            );
            return Err(error);
        }
    };
    let (stderr_read, stderr_write) = match pipe() {
        Some(pipe) => pipe,
        None => {
            let error = last_error();
            close_many(&[stdin_read, stdin_write, stdout_read, stdout_write]);
            restore_command_context(
                command_cwd,
                &saved_cwd_buffer[..saved_cwd_len],
                &env_restores,
            );
            return Err(error);
        }
    };

    let _ = fd_write(stdin_write, stdin);
    if !dup2(stdin_read, STDIN) || !dup2(stdout_write, STDOUT) || !dup2(stderr_write, STDERR) {
        let error = last_error();
        reset_stdio();
        close_many(&[
            stdin_read,
            stdin_write,
            stdout_read,
            stdout_write,
            stderr_read,
            stderr_write,
        ]);
        restore_command_context(
            command_cwd,
            &saved_cwd_buffer[..saved_cwd_len],
            &env_restores,
        );
        return Err(error);
    }

    let spawn_result = spawn_command(name, args, argv);
    reset_stdio();
    restore_command_context(
        command_cwd,
        &saved_cwd_buffer[..saved_cwd_len],
        &env_restores,
    );
    let pid = match spawn_result {
        Ok(pid) => pid,
        Err(code) => {
            close_many(&[
                stdin_read,
                stdin_write,
                stdout_read,
                stdout_write,
                stderr_read,
                stderr_write,
            ]);
            return Err(code);
        }
    };

    let status = match wait(pid) {
        Some(status) => status,
        None => {
            close_many(&[
                stdin_read,
                stdin_write,
                stdout_read,
                stdout_write,
                stderr_read,
                stderr_write,
            ]);
            return Err(ERR_NOENT);
        }
    };
    let stdout = read_all_available(stdout_read);
    let stderr = read_all_available(stderr_read);
    close_many(&[
        stdin_read,
        stdin_write,
        stdout_read,
        stdout_write,
        stderr_read,
        stderr_write,
    ]);

    Ok(CommandOutput {
        pid,
        status,
        stdout,
        stderr,
    })
}

fn run_command_status(
    name: &[u8],
    args: &[u8],
    argv: &[Vec<u8>],
    stdin: &[u8],
    command_cwd: Option<&[u8]>,
    env_overrides: &[CommandEnv],
    stdout_file: Option<&[u8]>,
    stderr_file: Option<&[u8]>,
) -> Result<CommandStatus, i32> {
    let mut saved_cwd_buffer = [0u8; 64];
    let saved_cwd_len = match command_cwd {
        Some(_) => {
            let current = cwd(&mut saved_cwd_buffer).ok_or_else(last_error)?;
            current.len()
        }
        None => 0,
    };

    if let Some(next_cwd) = command_cwd {
        if !chdir(next_cwd) {
            return Err(last_error());
        }
    }

    let env_restores = match apply_env_overrides(env_overrides) {
        Ok(restores) => restores,
        Err(code) => {
            restore_cwd(command_cwd, &saved_cwd_buffer[..saved_cwd_len]);
            return Err(code);
        }
    };

    let mut stdin_fds = None;
    if !stdin.is_empty() {
        let (stdin_read, stdin_write) = match pipe() {
            Some(pipe) => pipe,
            None => {
                let error = last_error();
                restore_command_context(
                    command_cwd,
                    &saved_cwd_buffer[..saved_cwd_len],
                    &env_restores,
                );
                return Err(error);
            }
        };
        let _ = fd_write(stdin_write, stdin);
        if !dup2(stdin_read, STDIN) {
            let error = last_error();
            reset_stdio();
            close_many(&[stdin_read, stdin_write]);
            restore_command_context(
                command_cwd,
                &saved_cwd_buffer[..saved_cwd_len],
                &env_restores,
            );
            return Err(error);
        }
        stdin_fds = Some((stdin_read, stdin_write));
    }

    let mut stdout_target = None;
    if let Some(path) = stdout_file {
        let file = match File::create(path) {
            Some(file) => file,
            None => {
                let error = last_error();
                reset_stdio();
                if let Some((stdin_read, stdin_write)) = stdin_fds {
                    close_many(&[stdin_read, stdin_write]);
                }
                restore_command_context(
                    command_cwd,
                    &saved_cwd_buffer[..saved_cwd_len],
                    &env_restores,
                );
                return Err(error);
            }
        };
        if !dup2(file.fd, STDOUT) {
            let error = last_error();
            reset_stdio();
            if let Some((stdin_read, stdin_write)) = stdin_fds {
                close_many(&[stdin_read, stdin_write]);
            }
            restore_command_context(
                command_cwd,
                &saved_cwd_buffer[..saved_cwd_len],
                &env_restores,
            );
            return Err(error);
        }
        stdout_target = Some(file);
    }

    let mut stderr_target = None;
    if let Some(path) = stderr_file {
        let file = match File::create(path) {
            Some(file) => file,
            None => {
                let error = last_error();
                reset_stdio();
                if let Some((stdin_read, stdin_write)) = stdin_fds {
                    close_many(&[stdin_read, stdin_write]);
                }
                restore_command_context(
                    command_cwd,
                    &saved_cwd_buffer[..saved_cwd_len],
                    &env_restores,
                );
                return Err(error);
            }
        };
        if !dup2(file.fd, STDERR) {
            let error = last_error();
            reset_stdio();
            if let Some((stdin_read, stdin_write)) = stdin_fds {
                close_many(&[stdin_read, stdin_write]);
            }
            restore_command_context(
                command_cwd,
                &saved_cwd_buffer[..saved_cwd_len],
                &env_restores,
            );
            return Err(error);
        }
        stderr_target = Some(file);
    }

    let spawn_result = spawn_command(name, args, argv);
    reset_stdio();
    restore_command_context(
        command_cwd,
        &saved_cwd_buffer[..saved_cwd_len],
        &env_restores,
    );

    // `spawn()` only enqueues the child now -- it may not actually run until
    // `wait()` below gets around to it (see the kernel's `run_ready_task`).
    // The redirected stdin pipe and stdout/stderr files must stay open until
    // then, so their teardown happens after `wait()`, not right after spawn.
    let pid = match spawn_result {
        Ok(pid) => pid,
        Err(code) => {
            if let Some((stdin_read, stdin_write)) = stdin_fds {
                close_many(&[stdin_read, stdin_write]);
            }
            drop(stdout_target);
            drop(stderr_target);
            return Err(code);
        }
    };

    let status = match wait(pid) {
        Some(status) => status,
        None => {
            if let Some((stdin_read, stdin_write)) = stdin_fds {
                close_many(&[stdin_read, stdin_write]);
            }
            drop(stdout_target);
            drop(stderr_target);
            return Err(ERR_NOENT);
        }
    };

    if let Some((stdin_read, stdin_write)) = stdin_fds {
        close_many(&[stdin_read, stdin_write]);
    }
    drop(stdout_target);
    drop(stderr_target);

    Ok(CommandStatus { pid, status })
}

fn spawn_command(name: &[u8], args: &[u8], argv: &[Vec<u8>]) -> Result<u32, i32> {
    if argv.is_empty() {
        return spawn(name, args);
    }
    let mut slices = Vec::new();
    for arg in argv {
        slices.push(ArgSlice {
            ptr: arg.as_ptr(),
            len: arg.len(),
        });
    }
    spawn_argv(name, &slices)
}

fn restore_cwd(command_cwd: Option<&[u8]>, saved_cwd: &[u8]) {
    if command_cwd.is_some() && !saved_cwd.is_empty() {
        let _ = chdir(saved_cwd);
    }
}

fn restore_command_context(command_cwd: Option<&[u8]>, saved_cwd: &[u8], env: &[EnvRestore]) {
    restore_env(env);
    restore_cwd(command_cwd, saved_cwd);
}

fn apply_env_overrides(overrides: &[CommandEnv]) -> Result<Vec<EnvRestore>, i32> {
    let mut restores = Vec::new();
    for override_env in overrides {
        let mut restore = EnvRestore {
            key: Vec::new(),
            existed: false,
            value: Vec::new(),
        };
        restore.key.extend_from_slice(override_env.key);

        let mut current = [0u8; 128];
        if let Some(value) = env_get(override_env.key, &mut current) {
            restore.existed = true;
            restore.value.extend_from_slice(value);
        }

        let ok = match override_env.value {
            Some(value) => env_set(override_env.key, value),
            None => env_remove(override_env.key),
        };
        if !ok {
            let error = last_error();
            restore_env(&restores);
            return Err(error);
        }
        restores.push(restore);
    }
    Ok(restores)
}

fn restore_env(restores: &[EnvRestore]) {
    for restore in restores.iter().rev() {
        if restore.existed {
            let _ = env_set(&restore.key, &restore.value);
        } else {
            let _ = env_remove(&restore.key);
        }
    }
}

fn read_all_available(fd: i32) -> Vec<u8> {
    let mut output = Vec::new();
    loop {
        let mut chunk = [0u8; 128];
        let Some(data) = fd_read(fd, &mut chunk) else {
            break;
        };
        if data.is_empty() {
            break;
        }
        output.extend_from_slice(data);
        if data.len() < chunk.len() {
            break;
        }
    }
    output
}

fn reset_stdio() {
    let _ = dup2(STDIN, STDIN);
    let _ = dup2(STDOUT, STDOUT);
    let _ = dup2(STDERR, STDERR);
}

fn close_many(fds: &[i32]) {
    for fd in fds {
        if *fd >= 0 {
            let _ = close_fd(*fd);
        }
    }
}

pub fn mem_alloc_pages(page_count: usize) -> Option<usize> {
    let address = with_abi(|abi| (abi.mem_alloc_pages)(page_count))?;
    if address == 0 {
        None
    } else {
        Some(address as usize)
    }
}

pub fn mem_map_pages(page_count: usize, flags: u32) -> Option<usize> {
    let address = with_abi(|abi| (abi.mem_map_pages)(page_count, flags))?;
    if address == 0 {
        None
    } else {
        Some(address as usize)
    }
}

pub fn mem_unmap_pages(address: usize, page_count: usize) -> bool {
    with_abi(|abi| (abi.mem_unmap_pages)(address as u64, page_count) == 0).unwrap_or(false)
}

pub fn time_ticks() -> u64 {
    with_abi(|abi| (abi.time_ticks)()).unwrap_or(0)
}

pub fn unlink(path: &[u8]) -> bool {
    with_abi(|abi| (abi.unlink)(path.as_ptr(), path.len()) == 0).unwrap_or(false)
}

pub fn rename(old_path: &[u8], new_path: &[u8]) -> bool {
    with_abi(|abi| {
        (abi.rename)(
            old_path.as_ptr(),
            old_path.len(),
            new_path.as_ptr(),
            new_path.len(),
        ) == 0
    })
    .unwrap_or(false)
}

pub fn cwd<'a>(buffer: &'a mut [u8]) -> Option<&'a [u8]> {
    let len = with_abi(|abi| (abi.cwd)(buffer.as_mut_ptr(), buffer.len()))?;
    if len < 0 {
        None
    } else {
        Some(&buffer[..core::cmp::min(len as usize, buffer.len())])
    }
}

pub fn chdir(path: &[u8]) -> bool {
    with_abi(|abi| (abi.chdir)(path.as_ptr(), path.len()) == 0).unwrap_or(false)
}

pub fn last_error() -> i32 {
    with_abi(|abi| (abi.last_error)()).unwrap_or(ERR_INVAL)
}

pub fn pipe() -> Option<(i32, i32)> {
    let mut read_fd = -1;
    let mut write_fd = -1;
    let ok = with_abi(|abi| (abi.pipe)(&mut read_fd, &mut write_fd))?;
    if ok == 0 {
        Some((read_fd, write_fd))
    } else {
        None
    }
}

pub fn dup2(old_fd: i32, new_fd: i32) -> bool {
    with_abi(|abi| (abi.dup2)(old_fd, new_fd) == new_fd).unwrap_or(false)
}

pub fn fd_read<'a>(fd: i32, buffer: &'a mut [u8]) -> Option<&'a [u8]> {
    let read = with_abi(|abi| (abi.read)(fd, buffer.as_mut_ptr(), buffer.len()))?;
    if read < 0 {
        None
    } else {
        Some(&buffer[..core::cmp::min(read as usize, buffer.len())])
    }
}

pub fn fd_write(fd: i32, bytes: &[u8]) -> Option<usize> {
    let written = with_abi(|abi| (abi.write_fd)(fd, bytes.as_ptr(), bytes.len()))?;
    if written < 0 {
        None
    } else {
        Some(written as usize)
    }
}

impl File {
    pub fn open(path: &[u8]) -> Option<Self> {
        Self::open_mode(path, OPEN_READ)
    }

    pub fn create(path: &[u8]) -> Option<Self> {
        Self::open_mode(path, OPEN_READ | OPEN_WRITE | OPEN_CREATE | OPEN_TRUNCATE)
    }

    pub fn append(path: &[u8]) -> Option<Self> {
        Self::open_mode(path, OPEN_WRITE | OPEN_CREATE | OPEN_APPEND)
    }

    pub fn options() -> OpenOptions {
        OpenOptions::new()
    }

    pub fn open_mode(path: &[u8], flags: u32) -> Option<Self> {
        let fd = with_abi(|abi| (abi.open)(path.as_ptr(), path.len(), flags))?;
        if fd < 0 { None } else { Some(Self { fd }) }
    }

    pub fn read<'a>(&mut self, buffer: &'a mut [u8]) -> Option<&'a [u8]> {
        let read = with_abi(|abi| (abi.read)(self.fd, buffer.as_mut_ptr(), buffer.len()))?;
        if read < 0 {
            None
        } else {
            Some(&buffer[..core::cmp::min(read as usize, buffer.len())])
        }
    }

    pub fn write(&mut self, bytes: &[u8]) -> Option<usize> {
        let written = with_abi(|abi| (abi.write_fd)(self.fd, bytes.as_ptr(), bytes.len()))?;
        if written < 0 {
            None
        } else {
            Some(written as usize)
        }
    }

    pub fn seek(&mut self, offset: usize) -> Option<usize> {
        let offset = with_abi(|abi| (abi.seek)(self.fd, offset))?;
        if offset < 0 {
            None
        } else {
            Some(offset as usize)
        }
    }

    pub fn close(mut self) -> bool {
        let ok = close_fd(self.fd);
        self.fd = -1;
        ok
    }
}

impl OpenOptions {
    pub fn new() -> Self {
        Self { flags: 0 }
    }

    pub fn read(mut self, read: bool) -> Self {
        self.set_flag(OPEN_READ, read);
        self
    }

    pub fn write(mut self, write: bool) -> Self {
        self.set_flag(OPEN_WRITE, write);
        self
    }

    pub fn append(mut self, append: bool) -> Self {
        self.set_flag(OPEN_APPEND, append);
        if append {
            self.set_flag(OPEN_WRITE, true);
        }
        self
    }

    pub fn create(mut self, create: bool) -> Self {
        self.set_flag(OPEN_CREATE, create);
        self
    }

    pub fn create_new(mut self, create_new: bool) -> Self {
        self.set_flag(OPEN_CREATE_NEW, create_new);
        if create_new {
            self.set_flag(OPEN_CREATE, true);
        }
        self
    }

    pub fn truncate(mut self, truncate: bool) -> Self {
        self.set_flag(OPEN_TRUNCATE, truncate);
        self
    }

    pub fn open(self, path: &[u8]) -> Option<File> {
        File::open_mode(path, self.flags)
    }

    fn set_flag(&mut self, flag: u32, enabled: bool) {
        if enabled {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if self.fd >= 0 {
            let _ = close_fd(self.fd);
            self.fd = -1;
        }
    }
}

pub fn close_fd(fd: i32) -> bool {
    with_abi(|abi| (abi.close)(fd) == 0).unwrap_or(false)
}

pub fn heap_size() -> usize {
    unsafe { *ALLOCATOR.allocated.get() }
}

pub fn heap_base() -> usize {
    unsafe { *ALLOCATOR.base.get() }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn with_abi<T>(f: impl FnOnce(&RymosAbi) -> T) -> Option<T> {
    unsafe { ABI.as_ref().map(f) }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println("program panic");
    loop {
        core::hint::spin_loop();
    }
}
