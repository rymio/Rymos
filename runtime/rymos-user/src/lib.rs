#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::ptr::null_mut;

const PAGE_SIZE: usize = 4096;
const MIN_HEAP_CHUNK: usize = 64 * 1024;

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
    pub spawn: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    pub wait: extern "sysv64" fn(u32, *mut ProcessStatus) -> i32,
    pub mem_alloc_pages: extern "sysv64" fn(usize) -> u64,
    pub time_ticks: extern "sysv64" fn() -> u64,
    pub unlink: extern "sysv64" fn(*const u8, usize) -> i32,
    pub rename: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
}

static mut ABI: *const RymosAbi = core::ptr::null();

pub const OPEN_READ: u32 = 1;
pub const OPEN_WRITE: u32 = 2;
pub const OPEN_CREATE: u32 = 4;
pub const OPEN_TRUNCATE: u32 = 8;
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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat {
    pub kind: u32,
    pub fs: u32,
    pub size: usize,
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
    let _ = with_abi(|abi| (abi.write)(bytes.as_ptr(), bytes.len()));
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
    };
    let ok = with_abi(|abi| (abi.stat)(path.as_ptr(), path.len(), &mut stat))?;
    if ok == 0 { Some(stat) } else { None }
}

pub fn list<'a>(namespace: &[u8], index: usize, name: &'a mut [u8]) -> Option<(&'a [u8], Stat)> {
    let mut stat = Stat {
        kind: 0,
        fs: 0,
        size: 0,
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

pub fn spawn(name: &[u8], args: &[u8]) -> Result<u32, i32> {
    let pid = with_abi(|abi| (abi.spawn)(name.as_ptr(), name.len(), args.as_ptr(), args.len()))
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

pub fn mem_alloc_pages(page_count: usize) -> Option<usize> {
    let address = with_abi(|abi| (abi.mem_alloc_pages)(page_count))?;
    if address == 0 {
        None
    } else {
        Some(address as usize)
    }
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

impl Drop for File {
    fn drop(&mut self) {
        if self.fd >= 0 {
            let _ = close_fd(self.fd);
            self.fd = -1;
        }
    }
}

fn close_fd(fd: i32) -> bool {
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
