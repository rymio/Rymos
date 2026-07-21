#![no_main]
#![no_std]

use core::arch::asm;
use core::cmp::min;
use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::ptr::{copy_nonoverlapping, write_bytes};

const VGA_BUFFER: *mut u8 = 0xB8000 as *mut u8;
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
const VGA_COLOR: u8 = 0x0A;
const TEXT_COLS_MAX: usize = 128;
const TEXT_ROWS_MAX: usize = 48;
const COM1: u16 = 0x3F8;
const KEYBOARD_DATA: u16 = 0x60;
const KEYBOARD_STATUS: u16 = 0x64;
const PIT_CHANNEL0_DATA: u16 = 0x40;
const PIT_CHANNEL2_DATA: u16 = 0x42;
const PIT_COMMAND: u16 = 0x43;
const PIT_INPUT_HZ: u64 = 1_193_182;
const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const PIC_EOI: u8 = 0x20;
const NMI_STATUS_CONTROL: u16 = 0x61;
const CMOS_INDEX: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
const INPUT_MAX: usize = 128;
const FILE_COUNT: usize = 12;
const FILE_NAME_MAX: usize = 16;
const FILE_DATA_MAX: usize = 192;
const PROCESS_COUNT: usize = 16;
const PROCESS_NAME_MAX: usize = 24;
const PROCESS_ARGS_MAX: usize = 64;
const PROCESS_ARGV_COUNT_MAX: usize = 8;
const PROCESS_ARGV_VALUE_MAX: usize = 64;
const PROCESS_HEAP_PAGE_MAX: usize = 1024;
const PROCESS_MAPPING_MAX: usize = 64;
const APP_CWD_MAX: usize = 64;
const ENV_COUNT: usize = 6;
const APP_ENV_COUNT: usize = 64;
const APP_ENV_KEY_MAX: usize = 24;
const APP_ENV_VALUE_MAX: usize = 96;
const APP_FD_COUNT: usize = 32;
const APP_FD_BASE: i32 = 3;
// Each `Command::output()`/`.status()` call holds 3 pipe slots (stdin,
// stdout, stderr) open for its *entire* duration -- including however long
// any child it spawns takes, since spawn runs synchronously to completion.
// A nested `Command` chain (e.g. cargo -> rustc -> linker) therefore needs
// 3 slots *per link still blocked*, not just 3 total: a realistic chain is
// already 2-3 links by the time the innermost one runs. 12 comfortably
// covers that with headroom. (`APP_PIPES` is a genuinely shared, ambient
// table -- not per-process -- so raising this no longer has any kernel
// stack cost per nested spawn level; that concern applied to the old
// `AppStateSnapshot` save/restore-around-spawn design, which category 2's
// scheduler work replaced with real per-process state on `Process` and a
// context switch that's just "change which index is current," not a copy.
// A previously-suspected second bug here -- nested redirection silently
// losing data or hanging once enough slots existed for a chain to actually
// proceed -- turned out to be a real but separate bug in `dup2`-based stdio
// restore, not in this pipe table; see `abi_std_fd`'s docs and
// `docs/self-hosting.md`'s Recently Closed.) Deeper nesting still hits this
// ceiling with a clean `ERR_NOSPC`, not a crash.
const APP_PIPE_COUNT: usize = 12;
const APP_PIPE_BUFFER_SIZE: usize = 8192;
const STDIN_FD: i32 = 0;
const STDOUT_FD: i32 = 1;
const STDERR_FD: i32 = 2;
const FD_READ: u32 = 1;
const FD_WRITE: u32 = 2;
const FD_CREATE: u32 = 4;
const FD_TRUNCATE: u32 = 8;
const FD_APPEND: u32 = 16;
const FD_CREATE_NEW: u32 = 32;
const STAT_KIND_FILE: u32 = 1;
const STAT_KIND_DIR: u32 = 2;
const STAT_FS_BOOTFS: u32 = 1;
const STAT_FS_PFS: u32 = 2;
const ERR_OK: i32 = 0;
const ERR_INVAL: i32 = 22;
const ERR_NOENT: i32 = 2;
const ERR_NOSPC: i32 = 28;
const ERR_NOTDIR: i32 = 20;
const ERR_ISDIR: i32 = 21;
const ERR_EXIST: i32 = 17;
const ERR_IO: i32 = 5;
const PFS_KIND_FILE: u8 = 1;
const PFS_KIND_DIR: u8 = 2;
// Entry layout (RYMFS5): kind(1) name_len(1) size(4) extent_count(1)
// extents[PFS_MAX_EXTENTS](8 each: start_sector u32 + sector_count u32)
// created_ticks(8) modified_ticks(8) mode(1) name(PFS_NAME_MAX). Files no
// longer need one giant contiguous run -- allocation can spread across up
// to PFS_MAX_EXTENTS separate runs, so free space fragmented by other files
// doesn't cause spurious "disk full" errors.
const PFS_MAX_EXTENTS: usize = 4;
const PFS_EXTENT_ENTRY_SIZE: usize = 8;
const PFS_EXTENT_COUNT_OFFSET: usize = 6;
const PFS_EXTENTS_OFFSET: usize = 7;
const PFS_EXTENTS_BYTES: usize = PFS_MAX_EXTENTS * PFS_EXTENT_ENTRY_SIZE;
const PFS_CREATED_OFFSET: usize = PFS_EXTENTS_OFFSET + PFS_EXTENTS_BYTES;
const PFS_MODIFIED_OFFSET: usize = PFS_CREATED_OFFSET + 8;
const PFS_MODE_OFFSET: usize = PFS_MODIFIED_OFFSET + 8;
const PFS_NAME_OFFSET: usize = PFS_MODE_OFFSET + 1;
// Raised from the original RYMFS3/4 values (30, 102) for longer nested paths
// and a bigger directory. Both are still a fixed compile-time ceiling, not
// true unbounded growth (that would mean moving the entry table itself onto
// growable disk extents, a bigger future redesign) -- but the on-disk header
// is read into a stack-local buffer in a few places (`read_header_silent`,
// `format`), so this can't grow arbitrarily without also moving that buffer
// off the stack. Verified this size boots and runs cleanly in QEMU.
const PFS_NAME_MAX: usize = 96;
const PFS_ENTRY_SIZE: usize = PFS_NAME_OFFSET + PFS_NAME_MAX;
const PFS_HEADER_SECTORS: u32 = {
    let bytes = 16 + PFS_ENTRY_COUNT * PFS_ENTRY_SIZE;
    ((bytes + 511) / 512) as u32
};
const PFS_HEADER_BYTES: usize = PFS_HEADER_SECTORS as usize * 512;
const PFS_ENTRY_COUNT: usize = 256;
const PFS_MODE_READ: u8 = 0b001;
const PFS_MODE_WRITE: u8 = 0b010;
const PFS_MODE_EXEC: u8 = 0b100;
const PFS_MODE_DEFAULT_FILE: u8 = PFS_MODE_READ | PFS_MODE_WRITE;
const PFS_MODE_DEFAULT_DIR: u8 = PFS_MODE_READ | PFS_MODE_WRITE | PFS_MODE_EXEC;
const BOOTFS_MODE: u32 = (PFS_MODE_READ | PFS_MODE_EXEC) as u32;
const PFS_SECTORS_PER_FILE: u32 = 524288;
const PFS_FILE_MAX: usize = PFS_SECTORS_PER_FILE as usize * 512;
const PFS_DATA_START: u32 = PFS_HEADER_SECTORS;
const PFS_DISK_SECTORS: u32 = 8_388_608;
const ATA_PRIMARY_IO: u16 = 0x1F0;
const ATA_PRIMARY_CTRL: u16 = 0x3F6;
const ATA_DATA_DRIVE: u8 = 1;
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 16;
const FB_FOREGROUND: u32 = 0x0000_FF00;
const FB_BACKGROUND: u32 = 0x0000_0000;
const PAGE_SIZE: u64 = 4096;
const PAGE_TABLE_ENTRIES: usize = 512;
const MAX_PHYS_RANGES: usize = 32;
const FREE_PAGE_STACK_MAX: usize = 4096;
const PAGE_PRESENT: u64 = 1;
const PAGE_WRITABLE: u64 = 1 << 1;
const PAGE_HUGE: u64 = 1 << 7;
const PAGE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const KERNEL_SCRATCH_BASE: u64 = 0xFFFF_8000_0000_0000;
const USER_HEAP_BASE: u64 = 0xFFFF_9000_0000_0000;
const USER_HEAP_STRIDE: u64 = 256 * 1024 * 1024;
const USER_HEAP_MAX_PAGES_PER_CALL: usize = 4096;
const USER_MMAP_BASE: u64 = 0xFFFF_A000_0000_0000;
const USER_MMAP_STRIDE: u64 = 1024 * 1024 * 1024;
const USER_MMAP_SIZE: u64 = 768 * 1024 * 1024;
const USER_MMAP_MAX_PAGES_PER_CALL: usize = 16384;
/// Per-pid kernel stack window (category 2's scheduler work), same
/// fixed-address-slice pattern as the heap/mmap windows above. Unlike
/// those, a stack has no software bump-pointer limit check -- every
/// `push`/`call` decrements RSP unconditionally, and this kernel has no
/// guard pages anywhere -- so each pid's window is deliberately made much
/// larger (`KERNEL_STACK_STRIDE`, one whole PD entry's span) than the
/// actual mapped stack (`KERNEL_STACK_PAGES`, mapped at the *top* of that
/// span, growing down): the unmapped lower portion of the same span is the
/// guard gap. An overflow big enough to eat through it hits a clean #PF
/// (already diagnosed-and-halted by the existing exception handler)
/// instead of silently corrupting the next pid's stack.
const KERNEL_STACK_BASE: u64 = 0xFFFF_B000_0000_0000;
const KERNEL_STACK_STRIDE: u64 = 2 * 1024 * 1024;
const KERNEL_STACK_PAGES: usize = 32;
const MEM_MAP_GUARD: u32 = 1;
const APP_LOAD_MIN: u64 = 0x200000;
const APP_LOAD_MAX: u64 = 0x9000000;
const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
const PT_LOAD: u32 = 1;

#[repr(C)]
pub struct BootInfo {
    framebuffer_base: u64,
    framebuffer_size: usize,
    horizontal_resolution: usize,
    vertical_resolution: usize,
    pixels_per_scan_line: usize,
    pixel_format: u32,
    initrd_base: u64,
    initrd_size: usize,
    memory_map_base: u64,
    memory_map_size: usize,
    memory_descriptor_size: usize,
}

#[repr(C)]
struct EfiMemoryDescriptor {
    typ: u32,
    physical_start: u64,
    virtual_start: u64,
    number_of_pages: u64,
    attribute: u64,
}

#[repr(C)]
struct RymosAbi {
    version: u32,
    write: extern "sysv64" fn(*const u8, usize),
    pid: extern "sysv64" fn() -> u32,
    args: extern "sysv64" fn(*mut u8, usize) -> usize,
    read_line: extern "sysv64" fn(*mut u8, usize) -> usize,
    file_size: extern "sysv64" fn(*const u8, usize) -> isize,
    file_read: extern "sysv64" fn(*const u8, usize, *mut u8, usize) -> isize,
    open: extern "sysv64" fn(*const u8, usize, u32) -> i32,
    read: extern "sysv64" fn(i32, *mut u8, usize) -> isize,
    write_fd: extern "sysv64" fn(i32, *const u8, usize) -> isize,
    seek: extern "sysv64" fn(i32, usize) -> isize,
    close: extern "sysv64" fn(i32) -> i32,
    stat: extern "sysv64" fn(*const u8, usize, *mut RymosStat) -> i32,
    list: extern "sysv64" fn(*const u8, usize, usize, *mut u8, usize, *mut RymosStat) -> isize,
    mkdir: extern "sysv64" fn(*const u8, usize) -> i32,
    env_get: extern "sysv64" fn(*const u8, usize, *mut u8, usize) -> isize,
    env_list: extern "sysv64" fn(usize, *mut u8, usize, *mut u8, usize) -> isize,
    env_set: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    env_remove: extern "sysv64" fn(*const u8, usize) -> i32,
    spawn: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    spawn_argv: extern "sysv64" fn(*const u8, usize, *const ArgSlice, usize) -> i32,
    wait: extern "sysv64" fn(u32, *mut RymosProcessStatus) -> i32,
    wait_any: extern "sysv64" fn(*mut RymosProcessStatus) -> i32,
    mem_alloc_pages: extern "sysv64" fn(usize) -> u64,
    mem_map_pages: extern "sysv64" fn(usize, u32) -> u64,
    mem_unmap_pages: extern "sysv64" fn(u64, usize) -> i32,
    time_ticks: extern "sysv64" fn() -> u64,
    unlink: extern "sysv64" fn(*const u8, usize) -> i32,
    rename: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    cwd: extern "sysv64" fn(*mut u8, usize) -> isize,
    chdir: extern "sysv64" fn(*const u8, usize) -> i32,
    last_error: extern "sysv64" fn() -> i32,
    pipe: extern "sysv64" fn(*mut i32, *mut i32) -> i32,
    dup2: extern "sysv64" fn(i32, i32) -> i32,
    argv_count: extern "sysv64" fn() -> usize,
    argv_get: extern "sysv64" fn(usize, *mut u8, usize) -> isize,
    time_unix_nanos: extern "sysv64" fn() -> u64,
    sleep_nanos: extern "sysv64" fn(u64),
    term_size: extern "sysv64" fn(*mut usize, *mut usize) -> i32,
    std_fd: extern "sysv64" fn(i32) -> i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RymosStat {
    kind: u32,
    fs: u32,
    size: usize,
    created_ticks: u64,
    modified_ticks: u64,
    mode: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RymosProcessStatus {
    state: u32,
    exit_code: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ArgSlice {
    ptr: *const u8,
    len: usize,
}

impl ArgSlice {
    const fn empty() -> Self {
        Self {
            ptr: core::ptr::null(),
            len: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct PageRange {
    next: u64,
    end: u64,
}

struct PhysPageAllocator {
    ranges: [PageRange; MAX_PHYS_RANGES],
    range_count: usize,
    total_pages: u64,
    allocated_pages: u64,
    free_stack: [u64; FREE_PAGE_STACK_MAX],
    free_count: usize,
}

#[repr(C)]
struct Elf64Header {
    ident: [u8; 16],
    typ: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[repr(C)]
struct Elf64ProgramHeader {
    typ: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum ProcessState {
    Empty = 0,
    Ready = 1,
    Running = 2,
    Exited = 3,
    Failed = 4,
    /// Voluntarily yielded inside `wait`/`wait_any` because its target
    /// hasn't exited yet (category 2's scheduler work). Never picked by
    /// `pick_next_ready` -- only `wake_waiters_for` moves a process back to
    /// `Ready`, when whatever it's waiting on actually exits.
    Blocked = 5,
}

/// What a `Blocked` process is waiting for -- see `abi_wait`/`abi_wait_any`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WaitTarget {
    None,
    Pid(u32),
    AnyChild,
}

/// A suspended task's resume point: just the stack pointer. Callee-saved
/// registers (`rbx`/`rbp`/`r12`-`r15`) live *on* that stack, pushed/popped
/// by `context_switch` itself -- the classic minimal fiber-switch design,
/// good enough here since a switch only ever happens at a deliberate call
/// site (the compiler has already made caller-saved registers dead by
/// then), not from an arbitrary interrupt. A real interrupt-driven
/// preemption (category 2, stage 3) needs a separate, *wider* saved-context
/// type covering every register, since an interrupt can land mid-instruction
/// with caller-saved registers still live -- not attempted here.
#[derive(Clone, Copy)]
struct CpuContext {
    rsp: u64,
}

impl CpuContext {
    const fn empty() -> Self {
        Self { rsp: 0 }
    }
}

#[derive(Clone, Copy)]
struct Process {
    pid: u32,
    parent_pid: u32,
    waited: bool,
    state: ProcessState,
    exit_code: i32,
    name: [u8; PROCESS_NAME_MAX],
    name_len: usize,
    args: [u8; PROCESS_ARGS_MAX],
    args_len: usize,
    argv: [[u8; PROCESS_ARGV_VALUE_MAX]; PROCESS_ARGV_COUNT_MAX],
    argv_lens: [usize; PROCESS_ARGV_COUNT_MAX],
    argv_count: usize,
    heap_base: u64,
    heap_pages: [u64; PROCESS_HEAP_PAGE_MAX],
    heap_page_count: usize,
    mappings: [ProcessMapping; PROCESS_MAPPING_MAX],
    /// Physical address of this process's private PML4, or 0 if it hasn't
    /// been given an isolated address space (falls back to the shared
    /// kernel PML4). See `create_process_address_space`.
    pml4_phys: u64,
    /// Real per-process ABI state (category 2's scheduler work) -- this
    /// process's own std-fd redirection, cwd, environment, last error, and
    /// heap/mmap bump-allocator progress. A spawned child's `std_fds`/`cwd`/
    /// `env` are copied from the parent's *current* values at `spawn()` time
    /// (see `spawn_prepared`) since `Command`-style helpers redirect stdio/
    /// cwd/env, spawn, and revert the redirection before ever waiting -- the
    /// child must see the redirection as it was at spawn time, not whatever
    /// the caller (or anything it does afterward) leaves these fields as
    /// since. `heap_base`/`heap_limit`/`mmap_limit` are pure functions of
    /// `pid` (see `app_set_heap_window`) and never change after being set
    /// once; `heap_next`/`mmap_next` are the actual bump-pointer progress
    /// and must persist across a context switch.
    std_fds: [i32; 3],
    cwd: [u8; APP_CWD_MAX],
    cwd_len: usize,
    env: [AppEnvVar; APP_ENV_COUNT],
    last_error: i32,
    heap_next: u64,
    heap_limit: u64,
    mmap_next: u64,
    mmap_limit: u64,
    /// Saved callee-saved registers + stack pointer for this process's own
    /// kernel stack (category 2's scheduler work) -- see `CpuContext` and
    /// `switch_to`. Meaningless while `state` is `Empty`/`Exited`/`Failed`.
    context: CpuContext,
    /// The ELF entry point to jump to the *first* time this process is
    /// switched into -- read by `process_trampoline`, since it runs on this
    /// process's own stack and can't see the spawning code's locals.
    entry_point: u64,
    /// What this process is waiting for, if `state == Blocked`. See
    /// `abi_wait`/`abi_wait_any`/`wake_waiters_for`.
    waiting_for: WaitTarget,
}

#[derive(Clone, Copy)]
struct ProcessMapping {
    used: bool,
    virt: u64,
    pages: usize,
}

#[derive(Clone, Copy)]
struct AppFd {
    open: bool,
    kind: AppFdKind,
    flags: u32,
    data: *const u8,
    len: usize,
    offset: usize,
    pfs_index: usize,
}

#[derive(Clone, Copy)]
struct AppPipe {
    used: bool,
    read_open: bool,
    write_open: bool,
    buffer: [u8; APP_PIPE_BUFFER_SIZE],
    read_offset: usize,
    len: usize,
}

#[derive(Clone, Copy)]
struct AppEnvVar {
    used: bool,
    deleted: bool,
    key: [u8; APP_ENV_KEY_MAX],
    key_len: usize,
    value: [u8; APP_ENV_VALUE_MAX],
    value_len: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppFdKind {
    Empty,
    BootFs,
    Pfs,
    PipeRead,
    PipeWrite,
}

impl AppFd {
    const fn empty() -> Self {
        Self {
            open: false,
            kind: AppFdKind::Empty,
            flags: 0,
            data: core::ptr::null(),
            len: 0,
            offset: 0,
            pfs_index: 0,
        }
    }
}

impl AppPipe {
    const fn empty() -> Self {
        Self {
            used: false,
            read_open: false,
            write_open: false,
            buffer: [0; APP_PIPE_BUFFER_SIZE],
            read_offset: 0,
            len: 0,
        }
    }
}

impl AppEnvVar {
    const fn empty() -> Self {
        Self {
            used: false,
            deleted: false,
            key: [0; APP_ENV_KEY_MAX],
            key_len: 0,
            value: [0; APP_ENV_VALUE_MAX],
            value_len: 0,
        }
    }
}

impl Process {
    const fn empty() -> Self {
        Self {
            pid: 0,
            parent_pid: 0,
            waited: false,
            state: ProcessState::Empty,
            exit_code: 0,
            name: [0; PROCESS_NAME_MAX],
            name_len: 0,
            args: [0; PROCESS_ARGS_MAX],
            args_len: 0,
            argv: [[0; PROCESS_ARGV_VALUE_MAX]; PROCESS_ARGV_COUNT_MAX],
            argv_lens: [0; PROCESS_ARGV_COUNT_MAX],
            argv_count: 0,
            heap_base: 0,
            heap_pages: [0; PROCESS_HEAP_PAGE_MAX],
            heap_page_count: 0,
            mappings: [ProcessMapping::empty(); PROCESS_MAPPING_MAX],
            pml4_phys: 0,
            std_fds: [STDIN_FD, STDOUT_FD, STDERR_FD],
            cwd: [0; APP_CWD_MAX],
            cwd_len: 0,
            env: [AppEnvVar::empty(); APP_ENV_COUNT],
            last_error: ERR_OK,
            heap_next: 0,
            heap_limit: 0,
            mmap_next: 0,
            mmap_limit: 0,
            context: CpuContext::empty(),
            entry_point: 0,
            waiting_for: WaitTarget::None,
        }
    }
}

impl ProcessMapping {
    const fn empty() -> Self {
        Self {
            used: false,
            virt: 0,
            pages: 0,
        }
    }
}

impl PageRange {
    const fn empty() -> Self {
        Self { next: 0, end: 0 }
    }
}

impl PhysPageAllocator {
    const fn empty() -> Self {
        Self {
            ranges: [PageRange::empty(); MAX_PHYS_RANGES],
            range_count: 0,
            total_pages: 0,
            allocated_pages: 0,
            free_stack: [0; FREE_PAGE_STACK_MAX],
            free_count: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::empty();
    }

    fn add_range(&mut self, start: u64, end: u64) {
        let start = align_up_u64(start, PAGE_SIZE);
        let end = align_down_u64(end, PAGE_SIZE);
        if start >= end || self.range_count >= MAX_PHYS_RANGES {
            return;
        }
        self.ranges[self.range_count] = PageRange { next: start, end };
        self.range_count += 1;
        self.total_pages += (end - start) / PAGE_SIZE;
    }

    fn alloc_page(&mut self) -> Option<u64> {
        if self.free_count > 0 {
            self.free_count -= 1;
            let page = self.free_stack[self.free_count];
            self.free_stack[self.free_count] = 0;
            self.allocated_pages += 1;
            return Some(page);
        }
        for index in 0..self.range_count {
            let range = &mut self.ranges[index];
            if range.next < range.end {
                let page = range.next;
                range.next += PAGE_SIZE;
                self.allocated_pages += 1;
                return Some(page);
            }
        }
        None
    }

    fn free_page(&mut self, page: u64) -> bool {
        if page & (PAGE_SIZE - 1) != 0 || self.free_count >= FREE_PAGE_STACK_MAX {
            return false;
        }
        self.free_stack[self.free_count] = page;
        self.free_count += 1;
        self.allocated_pages = self.allocated_pages.saturating_sub(1);
        true
    }

    fn free_pages(&self) -> u64 {
        self.total_pages.saturating_sub(self.allocated_pages)
    }
}

static mut APP_CONSOLE: *mut Console = core::ptr::null_mut();
static mut APP_BOOTFS: BootFs = BootFs::empty();
/// Index of whichever process is currently executing, into `PROCESS_TABLE`.
/// `PROCESS_COUNT` (one past the last real slot -- `PROCESS_TABLE` is sized
/// `PROCESS_COUNT + 1` for exactly this reason) is the reserved "idle/no
/// process" index: existing `index >= PROCESS_COUNT`-style sentinel checks
/// throughout this file stay correct unchanged, since real spawns only ever
/// allocate slots in `0..PROCESS_COUNT` (see `process_find_slot`/
/// `process_find_reapable_slot`). Per-process ABI state that used to be flat
/// globals (`args`/`pid`/`std_fds`/`cwd`/`env`/`last_error`/heap+mmap bump
/// pointers) now lives on `PROCESS_TABLE[CURRENT_PROCESS_INDEX]` itself --
/// see `Process`'s docs -- so a context switch is just changing this index,
/// not copying a large struct in and out of flat statics.
static mut CURRENT_PROCESS_INDEX: usize = PROCESS_COUNT;
static mut APP_FDS: [AppFd; APP_FD_COUNT] = [AppFd::empty(); APP_FD_COUNT];
static mut APP_PIPES: [AppPipe; APP_PIPE_COUNT] = [AppPipe::empty(); APP_PIPE_COUNT];
static mut PROCESS_TABLE: [Process; PROCESS_COUNT + 1] = [Process::empty(); PROCESS_COUNT + 1];
static mut NEXT_PID: u32 = 1;
static mut KERNEL_BOOT_INFO: *const BootInfo = core::ptr::null();
static mut PHYS_ALLOCATOR: PhysPageAllocator = PhysPageAllocator::empty();
static mut KERNEL_PML4_PHYS: u64 = 0;
/// `rdtsc` ticks per second, measured once at boot against the PIT (see
/// `calibrate_tsc`). Zero until calibration runs; every ns-conversion
/// function below falls back to treating raw ticks as nanoseconds (wrong,
/// but only until `calibrate_tsc` runs a few lines into `_start` -- nothing
/// reads time before that).
static mut TSC_HZ: u64 = 0;
/// `rdtsc` value captured immediately after calibration; every later time
/// reading is expressed as nanoseconds elapsed since this point.
static mut BOOT_TSC: u64 = 0;
/// Real Unix time (nanoseconds) read from the CMOS RTC at the same moment
/// `BOOT_TSC` was captured, or 0 if the RTC couldn't be read. Wall-clock
/// time is this plus nanoseconds elapsed since `BOOT_TSC`.
static mut BOOT_UNIX_NANOS: u64 = 0;
static mut NEXT_SCRATCH_VIRT: u64 = KERNEL_SCRATCH_BASE;
static ENV: [(&[u8], &[u8]); ENV_COUNT] = [
    (b"PATH", b"programs"),
    (b"HOME", b"/"),
    (b"SHELL", b"rysh"),
    (b"USER", b"root"),
    (b"RYMOS_TARGET", b"x86_64-rymos"),
    (b"TMPDIR", b"pfs:tmp"),
];
static RYMOS_ABI: RymosAbi = RymosAbi {
    version: 23,
    write: abi_write,
    pid: abi_pid,
    args: abi_args,
    read_line: abi_read_line,
    file_size: abi_file_size,
    file_read: abi_file_read,
    open: abi_open,
    read: abi_read,
    write_fd: abi_write_fd,
    seek: abi_seek,
    close: abi_close,
    stat: abi_stat,
    list: abi_list,
    mkdir: abi_mkdir,
    env_get: abi_env_get,
    env_list: abi_env_list,
    env_set: abi_env_set,
    env_remove: abi_env_remove,
    spawn: abi_spawn,
    spawn_argv: abi_spawn_argv,
    wait: abi_wait,
    wait_any: abi_wait_any,
    mem_alloc_pages: abi_mem_alloc_pages,
    mem_map_pages: abi_mem_map_pages,
    mem_unmap_pages: abi_mem_unmap_pages,
    time_ticks: abi_time_ticks,
    unlink: abi_unlink,
    rename: abi_rename,
    cwd: abi_cwd,
    chdir: abi_chdir,
    last_error: abi_last_error,
    pipe: abi_pipe,
    dup2: abi_dup2,
    argv_count: abi_argv_count,
    argv_get: abi_argv_get,
    time_unix_nanos: abi_time_unix_nanos,
    sleep_nanos: abi_sleep_nanos,
    term_size: abi_term_size,
    std_fd: abi_std_fd,
};

#[unsafe(no_mangle)]
pub extern "C" fn _start(boot_info: *const BootInfo) -> ! {
    let mut console = unsafe { Console::new(boot_info) };
    let mut fs = RamFs::new();
    let bootfs = unsafe { BootFs::new(boot_info) };

    unsafe {
        KERNEL_BOOT_INFO = boot_info;
        serial_init();
        init_physical_allocator(boot_info);
    }
    init_idt();

    unsafe {
        TSC_HZ = calibrate_tsc();
        // Read the RTC first (it can take a little real time -- see
        // `read_rtc_unix_seconds`'s UIP wait/double-read) so `BOOT_TSC`,
        // captured right after, lines up with the wall-clock reading it's
        // paired with as closely as possible.
        let unix_seconds = read_rtc_unix_seconds();
        BOOT_TSC = read_tsc();
        BOOT_UNIX_NANOS = unix_seconds.saturating_mul(1_000_000_000);
    }

    enable_timer_interrupts();

    console.clear();
    console.write_line("RYMOS minimal Rust kernel");
    console.write_line("_start reached");
    console.write_line("kernel shell online");
    console.write_line("");

    fs.seed();
    process_config_sys(&mut console, bootfs);
    run_script(&mut console, &mut fs, bootfs, b"autoexec.bat");
    shell_loop(&mut console, &mut fs, bootfs);
}

fn shell_loop(console: &mut Console, fs: &mut RamFs, bootfs: BootFs) -> ! {
    let mut input = [0u8; INPUT_MAX];
    let mut len = 0usize;

    prompt(console);

    loop {
        if let Some(byte) = read_input_byte() {
            match byte {
                b'\r' | b'\n' => {
                    console.new_line();
                    let command = &input[..len];
                    run_command(console, fs, bootfs, command);
                    len = 0;
                    input.fill(0);
                    prompt(console);
                }
                8 | 127 => {
                    if len > 0 {
                        len -= 1;
                        input[len] = 0;
                        console.backspace();
                    }
                }
                byte if byte.is_ascii_graphic() || byte == b' ' => {
                    if len + 1 < INPUT_MAX {
                        input[len] = byte;
                        len += 1;
                        console.write_byte(byte);
                    }
                }
                _ => {}
            }
        } else {
            spin_loop();
        }
    }
}

fn prompt(console: &mut Console) {
    console.write("rymos:/ $ ");
}

fn run_command(console: &mut Console, fs: &mut RamFs, bootfs: BootFs, input: &[u8]) {
    let input = trim(input);
    if input.is_empty() {
        return;
    }

    let (cmd, rest) = split_word(input);

    if eq(cmd, b"help") {
        console.write_line("commands:");
        console.write_line("  help                 show commands");
        console.write_line("  clear                clear screen");
        console.write_line("  about                kernel summary");
        console.write_line("  mem                  memory summary");
        console.write_line("  pagealloc            allocate one physical page");
        console.write_line("  paging               inspect active page tables");
        console.write_line("  vmclone              clone active PML4 and switch CR3");
        console.write_line("  ptalloc              allocate zeroed page-table page");
        console.write_line("  maptest              map and verify a scratch page");
        console.write_line("  video                show active video mode");
        console.write_line("  df                   filesystem usage");
        console.write_line("  fsformat             format persistent RYMFS disk");
        console.write_line("  pls                  list persistent files");
        console.write_line("  pread <file>         read persistent file");
        console.write_line("  pwrite <file> <txt>  write persistent file");
        console.write_line("  pdelete <file>       delete persistent file");
        console.write_line("  pmkdir <dir>         create persistent directory");
        console.write_line("  bootls               list boot filesystem");
        console.write_line("  bootcat <file>       read boot filesystem file");
        console.write_line("  run <program>        run bootfs Rust program");
        console.write_line("  ps                   list processes");
        console.write_line("  wait <pid>           show process exit status");
        console.write_line("  drivers              list kernel drivers");
        console.write_line("  dev                  list pseudo devices");
        console.write_line("  pci                  scan PCI config space");
        console.write_line("  pwd                  print working directory");
        console.write_line("  ls|list              list ramfs entries");
        console.write_line("  cat|read <file>      print file");
        console.write_line("  touch <file>         create empty file");
        console.write_line("  write <file> <text>  replace file");
        console.write_line("  append <file> <txt>  append to file");
        console.write_line("  echo|print <text>    print text");
        console.write_line("  cp|copy <src> <dst>  copy file");
        console.write_line("  mv|move <src> <dst>  move file");
        console.write_line("  rm|delete <file>     remove file");
        console.write_line("  mkdir <name>         create ramfs directory entry");
        console.write_line("  rmdir <name>         remove empty directory entry");
        console.write_line("  cd|goto [/]          show/change current dir");
        console.write_line("  reboot               reset via keyboard controller");
        console.write_line("  halt                 stop the CPU");
    } else if eq(cmd, b"clear") {
        console.clear();
    } else if eq(cmd, b"about") {
        console.write_line("RYMOS: Rust toy OS kernel");
        console.write_line("boot: UEFI loader from FAT32");
        console.write_line("kernel: freestanding x86_64 ELF");
        console.write_line("shell: in-kernel RAMFS shell");
    } else if eq(cmd, b"mem") {
        memory_summary(console);
    } else if eq(cmd, b"pagealloc") {
        pagealloc_command(console);
    } else if eq(cmd, b"paging") {
        paging_summary(console);
    } else if eq(cmd, b"vmclone") {
        vmclone_command(console);
    } else if eq(cmd, b"ptalloc") {
        ptalloc_command(console);
    } else if eq(cmd, b"maptest") {
        maptest_command(console);
    } else if eq(cmd, b"video") {
        console.video_info();
    } else if eq(cmd, b"df") {
        fs.df(console, bootfs);
        PersistentFs::df(console);
    } else if eq(cmd, b"fsformat") {
        PersistentFs::format(console);
    } else if eq(cmd, b"pls") {
        PersistentFs::ls(console);
    } else if eq(cmd, b"pread") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: pread <file>");
        } else {
            PersistentFs::read(console, name);
        }
    } else if eq(cmd, b"pwrite") {
        let (name, text) = split_word(rest);
        if name.is_empty() || text.is_empty() {
            console.write_line("usage: pwrite <file> <text>");
        } else {
            PersistentFs::write(console, name, text);
        }
    } else if eq(cmd, b"pdelete") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: pdelete <file>");
        } else {
            PersistentFs::delete(console, name);
        }
    } else if eq(cmd, b"pmkdir") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: pmkdir <dir>");
        } else {
            PersistentFs::mkdir(console, name);
        }
    } else if eq(cmd, b"bootls") {
        bootfs.ls(console);
    } else if eq(cmd, b"bootcat") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: bootcat <file>");
        } else {
            bootfs.cat(console, name);
        }
    } else if eq(cmd, b"run") {
        let (name, args) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: run <program>");
        } else {
            run_program(console, bootfs, name, args);
        }
    } else if eq(cmd, b"ps") {
        process_list(console);
    } else if eq(cmd, b"wait") {
        let (pid, _) = split_word(rest);
        if pid.is_empty() {
            console.write_line("usage: wait <pid>");
        } else {
            process_wait(console, pid);
        }
    } else if eq(cmd, b"drivers") {
        show_drivers(console);
    } else if eq(cmd, b"dev") {
        show_devices(console);
    } else if eq(cmd, b"pci") {
        pci_scan(console);
    } else if eq(cmd, b"pwd") {
        console.write_line("/");
    } else if eq(cmd, b"ls") || eq(cmd, b"list") {
        fs.ls(console);
    } else if eq(cmd, b"cat") || eq(cmd, b"read") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: read <file>");
        } else {
            fs.cat(console, name);
        }
    } else if eq(cmd, b"touch") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: touch <file>");
        } else {
            fs.touch(console, name, false);
        }
    } else if eq(cmd, b"write") {
        let (name, text) = split_word(rest);
        if name.is_empty() || text.is_empty() {
            console.write_line("usage: write <file> <text>");
        } else {
            fs.write(console, name, text);
        }
    } else if eq(cmd, b"append") {
        let (name, text) = split_word(rest);
        if name.is_empty() || text.is_empty() {
            console.write_line("usage: append <file> <text>");
        } else {
            fs.append(console, name, text);
        }
    } else if eq(cmd, b"echo") || eq(cmd, b"print") {
        console.write_bytes(rest);
        console.new_line();
    } else if eq(cmd, b"cp") || eq(cmd, b"copy") {
        let (src, rest) = split_word(rest);
        let (dst, _) = split_word(rest);
        if src.is_empty() || dst.is_empty() {
            console.write_line("usage: copy <src> <dst>");
        } else {
            fs.copy(console, src, dst);
        }
    } else if eq(cmd, b"mv") || eq(cmd, b"move") {
        let (src, rest) = split_word(rest);
        let (dst, _) = split_word(rest);
        if src.is_empty() || dst.is_empty() {
            console.write_line("usage: move <src> <dst>");
        } else {
            fs.move_entry(console, src, dst);
        }
    } else if eq(cmd, b"rm") || eq(cmd, b"delete") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: delete <file>");
        } else {
            fs.remove(console, name, false);
        }
    } else if eq(cmd, b"mkdir") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: mkdir <name>");
        } else {
            fs.touch(console, name, true);
        }
    } else if eq(cmd, b"rmdir") {
        let (name, _) = split_word(rest);
        if name.is_empty() {
            console.write_line("usage: rmdir <name>");
        } else {
            fs.remove(console, name, true);
        }
    } else if eq(cmd, b"cd") || eq(cmd, b"goto") {
        if rest.is_empty() || eq(rest, b"/") {
            console.write_line("/");
        } else {
            console.write_line("only / exists in ramfs shell");
        }
    } else if eq(cmd, b"reboot") {
        console.write_line("rebooting...");
        unsafe {
            outb(KEYBOARD_STATUS, 0xFE);
        }
    } else if eq(cmd, b"halt") {
        console.write_line("halted");
        loop {
            unsafe {
                core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
            }
        }
    } else {
        console.write("unknown command: ");
        console.write_bytes(cmd);
        console.new_line();
    }
}

struct Console {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    cells: [u8; TEXT_COLS_MAX * TEXT_ROWS_MAX],
    framebuffer_base: *mut u32,
    framebuffer_size: usize,
    framebuffer_width: usize,
    framebuffer_height: usize,
    pixels_per_scan_line: usize,
    pixel_format: u32,
}

impl Console {
    unsafe fn new(boot_info: *const BootInfo) -> Self {
        let Some(info) = (unsafe { boot_info.as_ref() }) else {
            return Self::without_framebuffer();
        };

        if info.framebuffer_base == 0
            || info.horizontal_resolution == 0
            || info.vertical_resolution == 0
            || info.pixels_per_scan_line == 0
        {
            return Self::without_framebuffer();
        }

        Self {
            row: 0,
            col: 0,
            rows: clamp_text_rows(info.vertical_resolution / FONT_HEIGHT),
            cols: clamp_text_cols(info.horizontal_resolution / FONT_WIDTH),
            cells: [b' '; TEXT_COLS_MAX * TEXT_ROWS_MAX],
            framebuffer_base: info.framebuffer_base as *mut u32,
            framebuffer_size: info.framebuffer_size,
            framebuffer_width: info.horizontal_resolution,
            framebuffer_height: info.vertical_resolution,
            pixels_per_scan_line: info.pixels_per_scan_line,
            pixel_format: info.pixel_format,
        }
    }

    const fn without_framebuffer() -> Self {
        Self {
            row: 0,
            col: 0,
            rows: VGA_HEIGHT,
            cols: VGA_WIDTH,
            cells: [b' '; TEXT_COLS_MAX * TEXT_ROWS_MAX],
            framebuffer_base: core::ptr::null_mut(),
            framebuffer_size: 0,
            framebuffer_width: 0,
            framebuffer_height: 0,
            pixels_per_scan_line: 0,
            pixel_format: 0,
        }
    }

    fn clear(&mut self) {
        self.clear_framebuffer();
        for row in 0..self.rows {
            for col in 0..self.cols {
                self.write_cell(row, col, b' ');
            }
        }
        self.row = 0;
        self.col = 0;
    }

    fn write_line(&mut self, text: &str) {
        self.write(text);
        self.new_line();
    }

    fn write(&mut self, text: &str) {
        self.write_bytes(text.as_bytes());
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_byte(*byte);
        }
    }

    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => {}
            byte => {
                if self.col >= self.cols {
                    self.new_line();
                }
                self.write_cell(self.row, self.col, byte);
                self.col += 1;
                unsafe {
                    serial_write_byte(byte);
                }
            }
        }
    }

    fn new_line(&mut self) {
        self.col = 0;
        if self.row + 1 >= self.rows {
            self.scroll();
        } else {
            self.row += 1;
        }
        unsafe {
            serial_write_byte(b'\r');
            serial_write_byte(b'\n');
        }
    }

    fn backspace(&mut self) {
        if self.col == 0 {
            return;
        }
        self.col -= 1;
        self.write_cell(self.row, self.col, b' ');
        unsafe {
            serial_write_byte(8);
            serial_write_byte(b' ');
            serial_write_byte(8);
        }
    }

    fn scroll(&mut self) {
        for row in 1..self.rows {
            for col in 0..self.cols {
                let byte = self.read_cell(row, col);
                self.write_cell(row - 1, col, byte);
            }
        }
        for col in 0..self.cols {
            self.write_cell(self.rows - 1, col, b' ');
        }
    }

    fn read_cell(&self, row: usize, col: usize) -> u8 {
        self.cells[row * TEXT_COLS_MAX + col]
    }

    fn write_cell(&mut self, row: usize, col: usize, byte: u8) {
        self.cells[row * TEXT_COLS_MAX + col] = byte;
        if row < VGA_HEIGHT && col < VGA_WIDTH {
            let offset = (row * VGA_WIDTH + col) * 2;
            unsafe {
                VGA_BUFFER.add(offset).write_volatile(byte);
                VGA_BUFFER.add(offset + 1).write_volatile(VGA_COLOR);
            }
        }
        self.draw_framebuffer_cell(row, col, byte);
    }

    fn clear_framebuffer(&self) {
        if self.framebuffer_base.is_null() {
            return;
        }

        let pixels = self.framebuffer_size / 4;
        for index in 0..pixels {
            unsafe {
                self.framebuffer_base
                    .add(index)
                    .write_volatile(FB_BACKGROUND);
            }
        }
    }

    fn draw_framebuffer_cell(&self, row: usize, col: usize, byte: u8) {
        if self.framebuffer_base.is_null() {
            return;
        }

        let x = col * FONT_WIDTH;
        let y = row * FONT_HEIGHT;
        if x + FONT_WIDTH >= self.framebuffer_width || y + FONT_HEIGHT >= self.framebuffer_height {
            return;
        }

        for py in 0..FONT_HEIGHT {
            for px in 0..FONT_WIDTH {
                self.write_pixel(x + px, y + py, FB_BACKGROUND);
            }
        }

        let glyph = glyph(byte);
        for glyph_y in 0..7 {
            let row_bits = glyph[glyph_y];
            for glyph_x in 0..5 {
                if row_bits & (1 << (4 - glyph_x)) != 0 {
                    let x0 = x + 1 + glyph_x;
                    let y0 = y + 2 + glyph_y * 2;
                    self.write_pixel(x0, y0, FB_FOREGROUND);
                    self.write_pixel(x0, y0 + 1, FB_FOREGROUND);
                }
            }
        }
    }

    fn write_pixel(&self, x: usize, y: usize, color: u32) {
        if x >= self.framebuffer_width || y >= self.framebuffer_height {
            return;
        }

        let index = y * self.pixels_per_scan_line + x;
        if index * 4 >= self.framebuffer_size {
            return;
        }

        unsafe {
            self.framebuffer_base.add(index).write_volatile(color);
        }
    }

    fn video_info(&mut self) {
        if self.framebuffer_base.is_null() {
            self.write_line("video: legacy VGA text fallback 80x25");
            return;
        }
        self.write("video: ");
        self.write_usize(self.framebuffer_width);
        self.write("x");
        self.write_usize(self.framebuffer_height);
        self.write(" pitch ");
        self.write_usize(self.pixels_per_scan_line);
        self.write(" text ");
        self.write_usize(self.cols);
        self.write("x");
        self.write_usize(self.rows);
        self.write(" format ");
        self.write_usize(self.pixel_format as usize);
        self.new_line();
    }
}

const fn clamp_text_cols(value: usize) -> usize {
    if value == 0 {
        VGA_WIDTH
    } else if value > TEXT_COLS_MAX {
        TEXT_COLS_MAX
    } else {
        value
    }
}

const fn clamp_text_rows(value: usize) -> usize {
    if value == 0 {
        VGA_HEIGHT
    } else if value > TEXT_ROWS_MAX {
        TEXT_ROWS_MAX
    } else {
        value
    }
}

#[derive(Clone, Copy)]
struct FileEntry {
    used: bool,
    is_dir: bool,
    name: [u8; FILE_NAME_MAX],
    name_len: usize,
    data: [u8; FILE_DATA_MAX],
    data_len: usize,
}

impl FileEntry {
    const fn empty() -> Self {
        Self {
            used: false,
            is_dir: false,
            name: [0; FILE_NAME_MAX],
            name_len: 0,
            data: [0; FILE_DATA_MAX],
            data_len: 0,
        }
    }
}

struct RamFs {
    entries: [FileEntry; FILE_COUNT],
}

#[derive(Clone, Copy)]
struct PfsExtent {
    start: u32,
    sectors: u32,
}

impl PfsExtent {
    const fn empty() -> Self {
        Self { start: 0, sectors: 0 }
    }
}

struct PersistentFs;

impl PersistentFs {
    fn format(console: &mut Console) {
        if !ata_present() {
            console.write_line("pfs: ATA data disk not found");
            return;
        }

        let mut header = [0u8; PFS_HEADER_BYTES];
        header[0..8].copy_from_slice(b"RYMFS5\0\0");
        header[8] = 5;
        header[9] = PFS_ENTRY_COUNT as u8;

        if Self::write_header(&header) {
            console.write_line("pfs: formatted");
        } else {
            console.write_line("pfs: format failed");
        }
    }

    fn ls(console: &mut Console) {
        let Some(header) = Self::read_header(console) else {
            return;
        };

        let mut found = false;
        for index in 0..PFS_ENTRY_COUNT {
            if pfs_entry_used(&header, index) {
                found = true;
                if pfs_entry_is_dir(&header, index) {
                    console.write("[pdir]  ");
                } else {
                    console.write("[pfile] ");
                }
                console.write_bytes(pfs_entry_name(&header, index));
                console.write("  ");
                console.write_usize(pfs_entry_size(&header, index));
                console.write_line(" B");
            }
        }

        if !found {
            console.write_line("pfs: empty");
        }
    }

    fn read(console: &mut Console, name: &[u8]) {
        let Some(header) = Self::read_header(console) else {
            return;
        };
        let Some(index) = pfs_find_entry(&header, name) else {
            console.write_line("pread: not found");
            return;
        };
        if pfs_entry_is_dir(&header, index) {
            console.write_line("pread: is a directory");
            return;
        }

        let size = pfs_entry_size(&header, index);
        let mut remaining = size;
        for logical_sector in 0..sectors_for_len(size) {
            if remaining == 0 {
                break;
            }
            let Some(lba) = pfs_entry_lba_for_sector(&header, index, logical_sector) else {
                console.write_line("pread: corrupt extents");
                return;
            };
            let mut sector = [0u8; 512];
            if !ata_read_sector(ATA_DATA_DRIVE, lba, &mut sector) {
                console.write_line("pread: disk read failed");
                return;
            }
            let count = min(remaining, 512);
            console.write_bytes(&sector[..count]);
            remaining -= count;
        }
        console.new_line();
    }

    fn write(console: &mut Console, name: &[u8], data: &[u8]) {
        if !valid_pfs_path(name) {
            console.write_line("pwrite: invalid name");
            return;
        }
        if data.len() > PFS_FILE_MAX {
            console.write_line("pwrite: file too large");
            return;
        }

        let Some(mut header) = Self::read_header(console) else {
            return;
        };
        if !pfs_parent_exists(&header, name) {
            console.write_line("pwrite: parent directory missing");
            return;
        }
        let index = match pfs_find_entry(&header, name) {
            Some(index) if pfs_entry_is_dir(&header, index) => {
                console.write_line("pwrite: is a directory");
                return;
            }
            Some(index) => index,
            None => match pfs_free_entry(&header) {
                Some(index) => index,
                None => {
                    console.write_line("pwrite: filesystem full");
                    return;
                }
            },
        };

        // Ensure the entry exists (possibly empty) before delegating to the
        // generic write path, which handles allocation (possibly across
        // several extents) and capacity growth uniformly with the ABI path.
        if !pfs_entry_used(&header, index) {
            pfs_set_entry(
                &mut header,
                index,
                name,
                0,
                PFS_KIND_FILE,
                &[],
                ns_since_boot(),
                PFS_MODE_DEFAULT_FILE,
            );
            if !Self::write_header(&header) {
                console.write_line("pwrite: header write failed");
                return;
            }
        }

        if !pfs_write_at(index, 0, data) {
            console.write_line("pwrite: disk full");
            return;
        }
        if pfs_update_size(index, data.len()) {
            console.write_line("ok");
        } else {
            console.write_line("pwrite: header write failed");
        }
    }

    fn mkdir(console: &mut Console, name: &[u8]) {
        let Some(mut header) = Self::read_header(console) else {
            return;
        };
        if !valid_pfs_path(name) {
            console.write_line("pmkdir: invalid directory");
            return;
        }
        if !pfs_parent_exists(&header, name) {
            console.write_line("pmkdir: parent directory missing");
            return;
        }
        if pfs_find_entry(&header, name).is_some() {
            console.write_line("pmkdir: exists");
            return;
        }
        let Some(index) = pfs_free_entry(&header) else {
            console.write_line("pmkdir: filesystem full");
            return;
        };
        pfs_set_entry(
            &mut header,
            index,
            name,
            0,
            PFS_KIND_DIR,
            &[],
            ns_since_boot(),
            PFS_MODE_DEFAULT_DIR,
        );
        if Self::write_header(&header) {
            console.write_line("ok");
        } else {
            console.write_line("pmkdir: header write failed");
        }
    }

    fn delete(console: &mut Console, name: &[u8]) {
        let Some(mut header) = Self::read_header(console) else {
            return;
        };
        match pfs_unlink_header(&mut header, name) {
            Ok(()) if Self::write_header(&header) => console.write_line("ok"),
            Ok(()) => console.write_line("pdelete: header write failed"),
            Err(-2) => console.write_line("pdelete: directory not empty"),
            Err(_) => console.write_line("pdelete: not found"),
        }
    }

    fn df(console: &mut Console) {
        if !ata_present() {
            console.write_line("pfs          no ATA data disk");
            return;
        }

        let Some(header) = Self::read_header_silent() else {
            console.write_line("pfs          unformatted");
            return;
        };

        let mut files = 0usize;
        let mut bytes = 0usize;
        for index in 0..PFS_ENTRY_COUNT {
            if pfs_entry_used(&header, index) && !pfs_entry_is_dir(&header, index) {
                files += 1;
                bytes += pfs_entry_size(&header, index);
            }
        }

        console.write("pfs          ");
        console.write_usize(files);
        console.write("/");
        console.write_usize(PFS_ENTRY_COUNT);
        console.write("          ");
        console.write_usize(bytes);
        console.write("/");
        console.write_usize((PFS_DISK_SECTORS - PFS_DATA_START) as usize * 512);
        console.new_line();
    }

    fn read_header(console: &mut Console) -> Option<[u8; PFS_HEADER_BYTES]> {
        if !ata_present() {
            console.write_line("pfs: ATA data disk not found");
            return None;
        }
        let Some(header) = Self::read_header_silent() else {
            console.write_line("pfs: unformatted; run fsformat");
            return None;
        };
        Some(header)
    }

    fn read_header_silent() -> Option<[u8; PFS_HEADER_BYTES]> {
        let mut header = [0u8; PFS_HEADER_BYTES];
        for sector_index in 0..PFS_HEADER_SECTORS {
            let start = sector_index as usize * 512;
            if !ata_read_sector(
                ATA_DATA_DRIVE,
                sector_index,
                (&mut header[start..start + 512]).try_into().ok()?,
            ) {
                return None;
            }
        }
        if &header[0..8] != b"RYMFS5\0\0" {
            return None;
        }
        Some(header)
    }

    fn write_header(header: &[u8; PFS_HEADER_BYTES]) -> bool {
        for sector_index in 0..PFS_HEADER_SECTORS {
            let start = sector_index as usize * 512;
            let mut sector = [0u8; 512];
            sector.copy_from_slice(&header[start..start + 512]);
            if !ata_write_sector(ATA_DATA_DRIVE, sector_index, &sector) {
                return false;
            }
        }
        true
    }
}

fn pfs_read_at(index: usize, offset: usize, dest: *mut u8, len: usize) -> bool {
    let Some(header) = PersistentFs::read_header_silent() else {
        return false;
    };
    if offset + len > PFS_FILE_MAX || !pfs_entry_used(&header, index) {
        return false;
    }

    let mut copied = 0usize;
    while copied < len {
        let absolute = offset + copied;
        let logical_sector = (absolute / 512) as u32;
        let sector_offset = absolute % 512;
        let count = min(len - copied, 512 - sector_offset);
        let Some(lba) = pfs_entry_lba_for_sector(&header, index, logical_sector) else {
            return false;
        };
        let mut sector = [0u8; 512];
        if !ata_read_sector(ATA_DATA_DRIVE, lba, &mut sector) {
            return false;
        }
        unsafe {
            copy_nonoverlapping(sector[sector_offset..].as_ptr(), dest.add(copied), count);
        }
        copied += count;
    }
    true
}

fn pfs_write_at(index: usize, offset: usize, data: &[u8]) -> bool {
    let new_size = offset + data.len();
    if new_size > PFS_FILE_MAX {
        return false;
    }

    let Some(old_size) = (match PersistentFs::read_header_silent() {
        Some(header) if pfs_entry_used(&header, index) => Some(pfs_entry_size(&header, index)),
        _ => None,
    }) else {
        return false;
    };

    if !pfs_ensure_file_capacity(index, new_size) {
        return false;
    }
    let Some(header) = PersistentFs::read_header_silent() else {
        return false;
    };

    // Sparse write: a seek-past-EOF-then-write leaves a gap that nobody
    // ever wrote. Freshly allocated sectors aren't implicitly zeroed (they're
    // just whatever a first-fit scan over other entries' extents found
    // free), so without this the gap would read back as leftover data from
    // whatever previously occupied those sectors instead of zero.
    if offset > old_size && !pfs_zero_range(&header, index, old_size, offset - old_size) {
        return false;
    }

    let mut copied = 0usize;
    while copied < data.len() {
        let absolute = offset + copied;
        let logical_sector = (absolute / 512) as u32;
        let sector_offset = absolute % 512;
        let count = min(data.len() - copied, 512 - sector_offset);
        let Some(lba) = pfs_entry_lba_for_sector(&header, index, logical_sector) else {
            return false;
        };
        let mut sector = [0u8; 512];
        if !ata_read_sector(ATA_DATA_DRIVE, lba, &mut sector) {
            return false;
        }
        sector[sector_offset..sector_offset + count].copy_from_slice(&data[copied..copied + count]);
        if !ata_write_sector(ATA_DATA_DRIVE, lba, &sector) {
            return false;
        }
        copied += count;
    }
    true
}

/// Zeros the logical byte range `[start, start + len)` of an entry's data,
/// used to make sparse writes (seek past EOF, then write) read back as
/// zero instead of stale disk contents. Skips the read for whole sectors
/// (only partial sectors at the range's edges need read-modify-write).
fn pfs_zero_range(header: &[u8; PFS_HEADER_BYTES], index: usize, start: usize, len: usize) -> bool {
    let mut written = 0usize;
    while written < len {
        let absolute = start + written;
        let logical_sector = (absolute / 512) as u32;
        let sector_offset = absolute % 512;
        let count = min(len - written, 512 - sector_offset);
        let Some(lba) = pfs_entry_lba_for_sector(header, index, logical_sector) else {
            return false;
        };
        if count == 512 {
            let sector = [0u8; 512];
            if !ata_write_sector(ATA_DATA_DRIVE, lba, &sector) {
                return false;
            }
        } else {
            let mut sector = [0u8; 512];
            if !ata_read_sector(ATA_DATA_DRIVE, lba, &mut sector) {
                return false;
            }
            sector[sector_offset..sector_offset + count].fill(0);
            if !ata_write_sector(ATA_DATA_DRIVE, lba, &sector) {
                return false;
            }
        }
        written += count;
    }
    true
}

fn pfs_update_size(index: usize, size: usize) -> bool {
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return false;
    };
    if !pfs_entry_used(&header, index) {
        return false;
    }
    let offset = pfs_entry_offset(index);
    header[offset + 2..offset + 6].copy_from_slice(&(size as u32).to_le_bytes());
    pfs_touch_modified(&mut header, index);
    PersistentFs::write_header(&header)
}

/// Grows `extents[..*count]` by `additional_sectors`, searching for free
/// space starting right after the current last extent (or from
/// `PFS_DATA_START` if there isn't one yet). Critically, if the free run
/// found continues immediately where the last extent left off, it *extends
/// that extent in place* instead of consuming a new slot -- without this,
/// incremental growth (e.g. many small sequential writes, each crossing a
/// sector boundary) would burn through all `PFS_MAX_EXTENTS` slots on
/// nothing but physically-contiguous sectors within a handful of writes,
/// long before the file is actually fragmented against other files.
fn pfs_grow_extents(
    header: &[u8; PFS_HEADER_BYTES],
    extents: &mut [PfsExtent; PFS_MAX_EXTENTS],
    count: &mut usize,
    additional_sectors: u32,
    skip_index: Option<usize>,
) -> bool {
    let mut remaining = additional_sectors;
    let mut search_from = if *count > 0 {
        let last = extents[*count - 1];
        last.start + last.sectors
    } else {
        PFS_DATA_START
    };

    while remaining > 0 {
        let Some((run_start, run_len)) = pfs_find_free_run(header, search_from, remaining, skip_index)
        else {
            return false;
        };
        if *count > 0 && run_start == extents[*count - 1].start + extents[*count - 1].sectors {
            extents[*count - 1].sectors += run_len;
        } else {
            if *count >= PFS_MAX_EXTENTS {
                return false;
            }
            extents[*count] = PfsExtent { start: run_start, sectors: run_len };
            *count += 1;
        }
        remaining -= run_len;
        search_from = run_start + run_len;
    }
    true
}

/// Grows an entry's allocation to cover at least `size` bytes. Unlike the
/// old single-extent design, growing never needs to copy existing data to a
/// new location: it just allocates additional sectors (coalescing into an
/// existing extent where possible, see `pfs_grow_extents`) for the
/// shortfall. Does not touch the entry's logical size field -- that's a
/// separate concern (see `pfs_update_size`), same contract as before.
fn pfs_ensure_file_capacity(index: usize, size: usize) -> bool {
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return false;
    };
    if !pfs_entry_used(&header, index) || pfs_entry_is_dir(&header, index) {
        return false;
    }

    let old_size = pfs_entry_size(&header, index);
    let old_sectors = pfs_entry_total_sectors(&header, index);
    let new_sectors = sectors_for_len(size);
    if new_sectors <= old_sectors {
        return true;
    }

    let mut extents = [PfsExtent::empty(); PFS_MAX_EXTENTS];
    let mut total_extents = pfs_entry_extent_count(&header, index);
    for slot in 0..total_extents {
        extents[slot] = pfs_entry_extent(&header, index, slot);
    }
    let additional = new_sectors - old_sectors;
    if !pfs_grow_extents(&header, &mut extents, &mut total_extents, additional, Some(index)) {
        return false;
    }

    let name = {
        let old_name = pfs_entry_name(&header, index);
        let mut name = [0u8; PFS_NAME_MAX];
        name[..old_name.len()].copy_from_slice(old_name);
        (name, old_name.len())
    };
    let kind = pfs_entry_kind(&header, index);
    let created = pfs_entry_created(&header, index);
    let mode = pfs_entry_mode(&header, index);
    pfs_set_entry(
        &mut header,
        index,
        &name.0[..name.1],
        old_size,
        kind,
        &extents[..total_extents],
        created,
        mode,
    );
    PersistentFs::write_header(&header)
}

/// Finds the first free contiguous run at or after `start_from`, capped to
/// at most `max_len` sectors (the caller may ask for less than the full
/// remaining free run so it can split allocation across several calls).
/// Scans every other entry's *entire extent list* for overlaps -- there's no
/// separate free-space bitmap; free space is always derived from whatever
/// the current entries say they're using, same philosophy as the original
/// single-extent allocator, just generalized to multiple extents per entry.
fn pfs_find_free_run(
    header: &[u8; PFS_HEADER_BYTES],
    start_from: u32,
    max_len: u32,
    skip_index: Option<usize>,
) -> Option<(u32, u32)> {
    let mut candidate = start_from;
    loop {
        if candidate >= PFS_DISK_SECTORS {
            return None;
        }
        let mut next_obstacle: Option<u32> = None;
        let mut blocked = false;
        for index in 0..PFS_ENTRY_COUNT {
            if Some(index) == skip_index || !pfs_entry_used(header, index) || pfs_entry_is_dir(header, index) {
                continue;
            }
            let extent_count = pfs_entry_extent_count(header, index);
            for slot in 0..extent_count {
                let extent = pfs_entry_extent(header, index, slot);
                if extent.sectors == 0 {
                    continue;
                }
                let used_end = extent.start + extent.sectors;
                if extent.start <= candidate && candidate < used_end {
                    candidate = used_end;
                    blocked = true;
                    break;
                }
                if extent.start > candidate && next_obstacle.is_none_or(|current| extent.start < current) {
                    next_obstacle = Some(extent.start);
                }
            }
            if blocked {
                break;
            }
        }
        if blocked {
            continue;
        }
        let end_bound = next_obstacle.unwrap_or(PFS_DISK_SECTORS).min(PFS_DISK_SECTORS);
        if end_bound <= candidate {
            return None;
        }
        let take = (end_bound - candidate).min(max_len);
        if take == 0 {
            return None;
        }
        return Some((candidate, take));
    }
}

fn sectors_for_len(len: usize) -> u32 {
    if len == 0 {
        0
    } else {
        ((len + 511) / 512) as u32
    }
}

fn pfs_entry_offset(index: usize) -> usize {
    16 + index * PFS_ENTRY_SIZE
}

fn process_spawn(name: &[u8], args: &[u8]) -> Option<usize> {
    let mut argv = [ArgSlice::empty(); PROCESS_ARGV_COUNT_MAX];
    let argv_count = raw_args_to_argv(args, &mut argv);
    let parent_pid = current_pid();
    process_spawn_with_argv(name, args, &argv[..argv_count], parent_pid)
}

fn process_spawn_with_argv(
    name: &[u8],
    args: &[u8],
    argv: &[ArgSlice],
    parent_pid: u32,
) -> Option<usize> {
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        let pid = NEXT_PID;
        NEXT_PID = NEXT_PID.wrapping_add(1);
        if NEXT_PID == 0 {
            NEXT_PID = 1;
        }

        if let Some(index) = process_find_slot(table, ProcessState::Empty)
            .or_else(|| process_find_reapable_slot(table))
        {
            let process = &mut (*table)[index];
            *process = Process::empty();
            process.pid = pid;
            process.parent_pid = parent_pid;
            process.state = ProcessState::Ready;
            process.name_len = min(name.len(), PROCESS_NAME_MAX);
            process.name[..process.name_len].copy_from_slice(&name[..process.name_len]);
            process.args_len = min(args.len(), PROCESS_ARGS_MAX);
            process.args[..process.args_len].copy_from_slice(&args[..process.args_len]);
            process.argv_count = min(argv.len(), PROCESS_ARGV_COUNT_MAX);
            for (arg_index, arg) in argv[..process.argv_count].iter().enumerate() {
                let arg_len = min(arg.len, PROCESS_ARGV_VALUE_MAX);
                process.argv_lens[arg_index] = arg_len;
                copy_nonoverlapping(arg.ptr, process.argv[arg_index].as_mut_ptr(), arg_len);
            }
            return Some(index);
        }
    }
    None
}

fn raw_args_to_argv(args: &[u8], argv: &mut [ArgSlice; PROCESS_ARGV_COUNT_MAX]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while count < PROCESS_ARGV_COUNT_MAX {
        let Some((token, next)) = next_arg_token(args, index) else {
            break;
        };
        argv[count] = ArgSlice {
            ptr: token.as_ptr(),
            len: token.len(),
        };
        count += 1;
        index = next;
    }
    count
}

unsafe fn process_find_slot(
    table: *mut [Process; PROCESS_COUNT + 1],
    state: ProcessState,
) -> Option<usize> {
    for index in 0..PROCESS_COUNT {
        if unsafe { (*table)[index].state } == state {
            return Some(index);
        }
    }
    None
}

/// A table slot that's safe to hand to a brand-new process: either never
/// used, or an `Exited`/`Failed` zombie that's *already been reaped*
/// (`waited == true`). An unwaited zombie must never be reused here --
/// `process_wait_by_pid`/`process_wait_any_child` are how a parent collects
/// its child's real exit status, and overwriting that slot before the parent
/// ever calls `wait` would silently destroy it instead of `wait` correctly
/// reporting "not found" (or, worse, blocking forever, once real blocking
/// wait exists). Filling the table with genuinely unreaped zombies should
/// fail spawn with `ERR_NOSPC`, matching real reaping semantics, not quietly
/// corrupt one of them.
unsafe fn process_find_reapable_slot(table: *mut [Process; PROCESS_COUNT + 1]) -> Option<usize> {
    for index in 0..PROCESS_COUNT {
        let process = unsafe { &(*table)[index] };
        if process.waited && matches!(process.state, ProcessState::Exited | ProcessState::Failed) {
            return Some(index);
        }
    }
    None
}

fn process_set_state(index: usize, state: ProcessState, exit_code: i32) {
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        (*table)[index].state = state;
        (*table)[index].exit_code = exit_code;
    }
}

/// Marks a top-level (console `run`-invoked) process `Failed` and already
/// reaped. There is no ABI-level parent that will ever call `wait`/`wait_any`
/// on a process started this way -- the console itself displayed the
/// failure -- so it must not be left as an unreapable zombie under
/// `process_find_reapable_slot`'s `waited` requirement the way a real
/// spawned child correctly is.
fn process_fail_unwaited(index: usize) {
    process_set_state(index, ProcessState::Failed, -1);
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        (*table)[index].waited = true;
    }
}

fn process_track_mapping(index: usize, virt: u64, pages: usize) -> bool {
    unsafe {
        if index >= PROCESS_COUNT {
            return false;
        }
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[index];
        for mapping in &mut process.mappings {
            if !mapping.used {
                *mapping = ProcessMapping {
                    used: true,
                    virt,
                    pages,
                };
                return true;
            }
        }
    }
    false
}

fn process_untrack_mapping(index: usize, virt: u64, pages: usize) -> bool {
    unsafe {
        if index >= PROCESS_COUNT {
            return false;
        }
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[index];
        for mapping in &mut process.mappings {
            if mapping.used && mapping.virt == virt && mapping.pages == pages {
                mapping.used = false;
                mapping.virt = 0;
                mapping.pages = 0;
                return true;
            }
        }
    }
    false
}

fn process_reclaim_mappings(index: usize) -> usize {
    unsafe {
        if index >= PROCESS_COUNT {
            return 0;
        }
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[index];
        let mut reclaimed = 0usize;
        for mapping in &mut process.mappings {
            if !mapping.used {
                continue;
            }
            reclaimed += unmap_user_pages(KERNEL_PML4_PHYS, mapping.virt, mapping.pages);
            *mapping = ProcessMapping::empty();
        }
        process.heap_base = 0;
        process.heap_pages.fill(0);
        process.heap_page_count = 0;
        reclaimed
    }
}

/// Frees the page-table *structure* pages backing `pid`'s heap and mmap
/// windows (the PT/PD pages `ensure_next_table` allocated while mapping
/// data into them) -- `process_reclaim_mappings` only frees the tracked data
/// pages, not the tables that pointed to them, so those structural pages
/// used to leak forever once a process exited (see `docs/self-hosting.md`).
///
/// Safe without a private-PML4-style walk (unlike
/// `destroy_process_address_space`) because PIDs are never reused and each
/// PID's window sits at a fixed, alignment-guaranteed address purely as a
/// function of its own PID:
/// - a heap window (`USER_HEAP_STRIDE` = 256 MiB) is exactly a quarter of one
///   shared PD's 1 GiB span, so it owns a clean, non-overlapping run of 128
///   PD entries (each pointing to one exclusively-owned PT) -- never a whole
///   PD, since 4 different PIDs' windows share that PD's other entries.
/// - a mmap window (`USER_MMAP_STRIDE` = 1 GiB) is exactly one whole PD span,
///   so the entire PD (and every PT it owns) belongs to this PID alone, and
///   the PD itself can be freed too.
/// Both stride values and both base addresses were checked to divide evenly
/// (`USER_HEAP_BASE`/`USER_MMAP_BASE` are 1 GiB-aligned), so this never frees
/// a table page another still-alive process's window also depends on.
fn reclaim_process_window_tables(pid: u32) {
    let Some(kernel_pml4) = (unsafe {
        let phys = KERNEL_PML4_PHYS;
        if phys == 0 { None } else { Some(phys) }
    }) else {
        return;
    };

    let heap_base = USER_HEAP_BASE + pid as u64 * USER_HEAP_STRIDE;
    if let Some(pdpt_phys) =
        table_entry_address(kernel_pml4 as *mut u64, pml4_index(heap_base))
    {
        if let Some(pd_phys) = table_entry_address(pdpt_phys as *mut u64, pdpt_index(heap_base)) {
            let start_pd = pd_index(heap_base);
            let end_pd = start_pd + (USER_HEAP_STRIDE / (2 * 1024 * 1024)) as usize;
            let pd = pd_phys as *mut u64;
            for pd_idx in start_pd..end_pd {
                if let Some(pt_phys) = table_entry_address(pd, pd_idx) {
                    free_phys_page(pt_phys);
                    unsafe {
                        pd.add(pd_idx).write_volatile(0);
                        invlpg(heap_base + (pd_idx - start_pd) as u64 * 2 * 1024 * 1024);
                    }
                }
            }
        }
    }

    let mmap_base = USER_MMAP_BASE + pid as u64 * USER_MMAP_STRIDE;
    if let Some(pdpt_phys) =
        table_entry_address(kernel_pml4 as *mut u64, pml4_index(mmap_base))
    {
        let pdpt_idx = pdpt_index(mmap_base);
        if let Some(pd_phys) = table_entry_address(pdpt_phys as *mut u64, pdpt_idx) {
            let pd = pd_phys as *mut u64;
            for pd_idx in 0..PAGE_TABLE_ENTRIES {
                if let Some(pt_phys) = table_entry_address(pd, pd_idx) {
                    free_phys_page(pt_phys);
                }
            }
            free_phys_page(pd_phys);
            unsafe {
                (pdpt_phys as *mut u64).add(pdpt_idx).write_volatile(0);
                invlpg(mmap_base);
            }
        }
    }
}

fn process_pid(index: usize) -> u32 {
    unsafe {
        let table = &raw const PROCESS_TABLE;
        (*table)[index].pid
    }
}

/// The pid of whichever process is currently executing (0 if none -- the
/// reserved idle slot's `Process::empty()` defaults to pid 0, matching the
/// old flat `APP_PID`'s "0 means no process" convention).
fn current_pid() -> u32 {
    unsafe { process_pid(CURRENT_PROCESS_INDEX) }
}

fn process_list(console: &mut Console) {
    console.write_line("pid  state    exit  name args");
    unsafe {
        let table = &raw const PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            let process = &(*table)[index];
            if matches!(process.state, ProcessState::Empty) {
                continue;
            }
            console.write_usize(process.pid as usize);
            console.write("    ");
            console.write(process_state_name(process.state));
            console.write("  ");
            console.write_i32(process.exit_code);
            console.write("     ");
            console.write_bytes(&process.name[..process.name_len]);
            if process.args_len > 0 {
                console.write(" ");
                console.write_bytes(&process.args[..process.args_len]);
            }
            console.new_line();
        }
    }
}

fn process_wait(console: &mut Console, pid_bytes: &[u8]) {
    if pid_bytes.is_empty() {
        if let Some((pid, status)) = process_wait_any_child(0) {
            print_process_status(console, pid, status);
        } else {
            console.write_line("wait: no child status");
        }
        return;
    }

    let Some(pid) = parse_u32(pid_bytes) else {
        console.write_line("wait: invalid pid");
        return;
    };

    if let Some(status) = process_wait_by_pid(pid) {
        print_process_status(console, pid, status);
        return;
    }

    console.write_line("wait: pid not found");
}

fn print_process_status(console: &mut Console, pid: u32, status: RymosProcessStatus) {
    console.write("pid ");
    console.write_usize(pid as usize);
    console.write(" ");
    console.write(process_state_name(process_state_from_u32(status.state)));
    console.write(" exit ");
    console.write_i32(status.exit_code);
    console.new_line();
}

fn process_wait_by_pid(pid: u32) -> Option<RymosProcessStatus> {
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            let process = &mut (*table)[index];
            if process.pid == pid
                && !process.waited
                && matches!(process.state, ProcessState::Exited | ProcessState::Failed)
            {
                process.waited = true;
                return Some(RymosProcessStatus {
                    state: process.state as u32,
                    exit_code: process.exit_code,
                });
            }
        }
    }
    None
}

fn process_wait_any_child(parent_pid: u32) -> Option<(u32, RymosProcessStatus)> {
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            let process = &mut (*table)[index];
            if process.parent_pid == parent_pid
                && !process.waited
                && matches!(process.state, ProcessState::Exited | ProcessState::Failed)
            {
                process.waited = true;
                return Some((
                    process.pid,
                    RymosProcessStatus {
                        state: process.state as u32,
                        exit_code: process.exit_code,
                    },
                ));
            }
        }
    }
    None
}

/// Table index of the process with this pid, if any (regardless of state).
fn find_process_index_by_pid(pid: u32) -> Option<usize> {
    unsafe {
        let table = &raw const PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            if (*table)[index].pid == pid && !matches!((*table)[index].state, ProcessState::Empty)
            {
                return Some(index);
            }
        }
    }
    None
}

fn process_state_name(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Empty => "empty",
        ProcessState::Ready => "ready",
        ProcessState::Running => "running",
        ProcessState::Exited => "exited",
        ProcessState::Failed => "failed",
        ProcessState::Blocked => "blocked",
    }
}

fn process_state_from_u32(state: u32) -> ProcessState {
    match state {
        1 => ProcessState::Ready,
        2 => ProcessState::Running,
        3 => ProcessState::Exited,
        4 => ProcessState::Failed,
        5 => ProcessState::Blocked,
        _ => ProcessState::Empty,
    }
}

unsafe fn init_physical_allocator(boot_info: *const BootInfo) {
    let allocator = unsafe { &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR) };
    allocator.reset();
    let Some(info) = (unsafe { boot_info.as_ref() }) else {
        return;
    };
    if info.memory_map_base == 0 || info.memory_map_size == 0 || info.memory_descriptor_size == 0 {
        return;
    }

    let descriptor_count = info.memory_map_size / info.memory_descriptor_size;
    let base = info.memory_map_base as *const u8;
    for index in 0..descriptor_count {
        let descriptor = unsafe {
            &*(base
                .add(index * info.memory_descriptor_size)
                .cast::<EfiMemoryDescriptor>())
        };
        if descriptor.typ != 7 {
            continue;
        }
        let start = descriptor.physical_start;
        let end = start + descriptor.number_of_pages * PAGE_SIZE;
        let start = if start < APP_LOAD_MAX {
            APP_LOAD_MAX
        } else {
            start
        };
        allocator.add_range(start, end);
    }
}

fn memory_summary(console: &mut Console) {
    console.write_line("memory:");
    console.write_line("  kernel linked at 0x100000");
    console.write_line("  VGA text buffer at 0xb8000");

    let Some(info) = (unsafe { KERNEL_BOOT_INFO.as_ref() }) else {
        console.write_line("  UEFI memory map: unavailable");
        return;
    };
    if info.memory_map_base == 0 || info.memory_map_size == 0 || info.memory_descriptor_size == 0 {
        console.write_line("  UEFI memory map: unavailable");
        return;
    }

    let descriptor_count = info.memory_map_size / info.memory_descriptor_size;
    let mut conventional_pages = 0u64;
    let mut highest_conventional_end = 0u64;
    let base = info.memory_map_base as *const u8;
    for index in 0..descriptor_count {
        let descriptor = unsafe {
            &*(base
                .add(index * info.memory_descriptor_size)
                .cast::<EfiMemoryDescriptor>())
        };
        let end = descriptor.physical_start + descriptor.number_of_pages * 4096;
        if descriptor.typ == 7 {
            conventional_pages += descriptor.number_of_pages;
            if end > highest_conventional_end {
                highest_conventional_end = end;
            }
        }
    }

    console.write("  UEFI memory map: ");
    console.write_usize(descriptor_count);
    console.write_line(" descriptors");
    console.write("  conventional free RAM: ");
    console.write_usize((conventional_pages * 4096 / 1024 / 1024) as usize);
    console.write_line(" MiB");
    console.write("  highest free RAM end: 0x");
    console.write_hex_u64(highest_conventional_end);
    console.new_line();

    unsafe {
        let allocator = &*core::ptr::addr_of!(PHYS_ALLOCATOR);
        console.write("  page allocator ranges: ");
        console.write_usize(allocator.range_count);
        console.new_line();
        console.write("  page allocator free: ");
        console.write_usize(allocator.free_pages() as usize);
        console.write(" pages / ");
        console.write_usize((allocator.free_pages() * PAGE_SIZE / 1024 / 1024) as usize);
        console.write_line(" MiB");
        console.write("  page allocator used: ");
        console.write_usize(allocator.allocated_pages as usize);
        console.write_line(" pages");
        console.write("  reusable free stack: ");
        console.write_usize(allocator.free_count);
        console.write_line(" pages");
    }

    unsafe {
        console.write("  timer ticks (IRQ0, ~100 Hz): ");
        console.write_usize(TIMER_TICKS as usize);
        console.new_line();
    }
}

fn pagealloc_command(console: &mut Console) {
    unsafe {
        let allocator = &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR);
        let Some(page) = allocator.alloc_page() else {
            console.write_line("pagealloc: out of pages");
            return;
        };
        console.write("pagealloc: 0x");
        console.write_hex_u64(page);
        console.new_line();
    }
}

fn paging_summary(console: &mut Console) {
    let cr3 = read_cr3();
    let pml4_phys = cr3 & 0x000F_FFFF_FFFF_F000;
    console.write_line("paging:");
    console.write("  cr3: 0x");
    console.write_hex_u64(cr3);
    console.new_line();
    console.write("  pml4: 0x");
    console.write_hex_u64(pml4_phys);
    console.new_line();
    let kernel_pml4 = unsafe { KERNEL_PML4_PHYS };
    if kernel_pml4 != 0 {
        console.write("  kernel-owned pml4: 0x");
        console.write_hex_u64(kernel_pml4);
        console.new_line();
    }

    let pml4 = pml4_phys as *const u64;
    let mut present = 0usize;
    let mut first = PAGE_TABLE_ENTRIES;
    let mut last = 0usize;
    let mut huge = 0usize;
    unsafe {
        for index in 0..PAGE_TABLE_ENTRIES {
            let entry = pml4.add(index).read_volatile();
            if entry & PAGE_PRESENT != 0 {
                present += 1;
                if first == PAGE_TABLE_ENTRIES {
                    first = index;
                }
                last = index;
                if entry & PAGE_HUGE != 0 {
                    huge += 1;
                }
            }
        }
    }

    console.write("  pml4 present entries: ");
    console.write_usize(present);
    if present > 0 {
        console.write(" (");
        console.write_usize(first);
        console.write("..");
        console.write_usize(last);
        console.write(")");
    }
    console.new_line();
    console.write("  pml4 huge entries: ");
    console.write_usize(huge);
    console.new_line();
    if kernel_pml4 == pml4_phys && kernel_pml4 != 0 {
        console.write_line("  mapper: kernel-owned PML4 active, lower-level mapping edits next");
    } else {
        console.write_line("  mapper: firmware PML4 active, run vmclone to take ownership");
    }
}

fn vmclone_command(console: &mut Console) {
    let old_cr3 = read_cr3();
    let old_pml4 = old_cr3 & 0x000F_FFFF_FFFF_F000;
    let Some(new_pml4) = ensure_kernel_pml4() else {
        console.write_line("vmclone: out of pages");
        return;
    };
    console.write("vmclone: 0x");
    console.write_hex_u64(old_pml4);
    console.write(" -> 0x");
    console.write_hex_u64(new_pml4);
    console.new_line();
}

fn ensure_kernel_pml4() -> Option<u64> {
    let kernel_pml4 = unsafe { KERNEL_PML4_PHYS };
    if kernel_pml4 != 0 {
        // Deliberately does NOT force CR3 back to `kernel_pml4` here. A
        // process running in its own private PML4 (see
        // `create_process_address_space`) shares every sub-table with
        // `kernel_pml4` except the program-image range, so heap/mmap
        // mappings created against `kernel_pml4` are visible through the
        // process's own PML4 too -- forcing CR3 back would instead yank the
        // currently-running process's private image mappings out from under
        // it mid-execution.
        return Some(kernel_pml4);
    }

    let new_pml4 = clone_active_pml4()?;
    unsafe {
        KERNEL_PML4_PHYS = new_pml4;
        write_cr3(new_pml4);
    }
    Some(new_pml4)
}

/// The PML4 the currently-running process should be restored to / is
/// running under: its own private one if `create_process_address_space` gave
/// it one, else the shared kernel PML4 (initializing it if this is the very
/// first process/mapping activity since boot).
fn current_pml4_or_kernel() -> Option<u64> {
    let index = unsafe { CURRENT_PROCESS_INDEX };
    if index < PROCESS_COUNT {
        let pml4 = unsafe {
            let table = &raw const PROCESS_TABLE;
            (*table)[index].pml4_phys
        };
        if pml4 != 0 {
            return Some(pml4);
        }
    }
    ensure_kernel_pml4()
}

fn ptalloc_command(console: &mut Console) {
    let Some(page) = alloc_zeroed_page() else {
        console.write_line("ptalloc: out of pages");
        return;
    };
    let zero = page_is_zeroed(page);
    console.write("ptalloc: 0x");
    console.write_hex_u64(page);
    console.write(" zeroed=");
    console.write_line(if zero { "yes" } else { "no" });
}

fn maptest_command(console: &mut Console) {
    if unsafe { KERNEL_PML4_PHYS } == 0 {
        console.write_line("maptest: run vmclone first");
        return;
    }
    let Some(phys) = alloc_zeroed_page() else {
        console.write_line("maptest: out of pages");
        return;
    };
    let virt = unsafe {
        let virt = NEXT_SCRATCH_VIRT;
        NEXT_SCRATCH_VIRT += PAGE_SIZE;
        virt
    };
    if !map_page(unsafe { KERNEL_PML4_PHYS }, virt, phys, PAGE_PRESENT | PAGE_WRITABLE) {
        console.write_line("maptest: map failed");
        return;
    }

    let marker = 0x5259_4D4F_535F_4D41u64;
    unsafe {
        (virt as *mut u64).write_volatile(marker);
    }
    let read_back = unsafe { (virt as *const u64).read_volatile() };
    console.write("maptest: virt 0x");
    console.write_hex_u64(virt);
    console.write(" -> phys 0x");
    console.write_hex_u64(phys);
    console.write(" readback=");
    if read_back == marker {
        console.write_line("ok");
    } else {
        console.write("bad 0x");
        console.write_hex_u64(read_back);
        console.new_line();
    }
}

fn map_page(pml4_phys: u64, virt: u64, phys: u64, flags: u64) -> bool {
    if pml4_phys == 0 || virt & (PAGE_SIZE - 1) != 0 || phys & (PAGE_SIZE - 1) != 0 {
        return false;
    }

    let pml4 = pml4_phys as *mut u64;
    let Some(pdpt_phys) = ensure_next_table(pml4, pml4_index(virt)) else {
        return false;
    };
    let pdpt = pdpt_phys as *mut u64;
    let Some(pd_phys) = ensure_next_table(pdpt, pdpt_index(virt)) else {
        return false;
    };
    let pd = pd_phys as *mut u64;
    let Some(pt_phys) = ensure_next_table(pd, pd_index(virt)) else {
        return false;
    };
    let pt = pt_phys as *mut u64;
    let index = pt_index(virt);
    unsafe {
        let entry = pt.add(index).read_volatile();
        if entry & PAGE_PRESENT != 0 {
            return false;
        }
        pt.add(index)
            .write_volatile((phys & PAGE_ADDR_MASK) | flags);
        invlpg(virt);
    }
    true
}

fn ensure_next_table(table: *mut u64, index: usize) -> Option<u64> {
    unsafe {
        let entry_ptr = table.add(index);
        let entry = entry_ptr.read_volatile();
        if entry & PAGE_PRESENT != 0 {
            if entry & PAGE_HUGE != 0 {
                return None;
            }
            return Some(entry & PAGE_ADDR_MASK);
        }
        let next = alloc_zeroed_page()?;
        entry_ptr.write_volatile((next & PAGE_ADDR_MASK) | PAGE_PRESENT | PAGE_WRITABLE);
        Some(next)
    }
}

fn unmap_page(pml4_phys: u64, virt: u64) -> Option<u64> {
    if pml4_phys == 0 || virt & (PAGE_SIZE - 1) != 0 {
        return None;
    }

    let pml4 = pml4_phys as *mut u64;
    let pdpt_phys = table_entry_address(pml4, pml4_index(virt))?;
    let pdpt = pdpt_phys as *mut u64;
    let pd_phys = table_entry_address(pdpt, pdpt_index(virt))?;
    let pd = pd_phys as *mut u64;
    let pt_phys = table_entry_address(pd, pd_index(virt))?;
    let pt = pt_phys as *mut u64;
    let index = pt_index(virt);
    unsafe {
        let entry_ptr = pt.add(index);
        let entry = entry_ptr.read_volatile();
        if entry & PAGE_PRESENT == 0 {
            return None;
        }
        let phys = entry & PAGE_ADDR_MASK;
        entry_ptr.write_volatile(0);
        invlpg(virt);
        Some(phys)
    }
}

fn map_user_pages(pml4_phys: u64, base: u64, page_count: usize) -> bool {
    for index in 0..page_count {
        let virt = base + index as u64 * PAGE_SIZE;
        let Some(phys) = alloc_zeroed_page() else {
            let _ = unmap_user_pages(pml4_phys, base, index);
            return false;
        };
        if !map_page(pml4_phys, virt, phys, PAGE_PRESENT | PAGE_WRITABLE) {
            let allocator = unsafe { &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR) };
            let _ = allocator.free_page(phys);
            let _ = unmap_user_pages(pml4_phys, base, index);
            return false;
        }
    }
    true
}

fn unmap_user_pages(pml4_phys: u64, base: u64, page_count: usize) -> usize {
    let allocator = unsafe { &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR) };
    let mut reclaimed = 0usize;
    for index in 0..page_count {
        let virt = base + index as u64 * PAGE_SIZE;
        if let Some(phys) = unmap_page(pml4_phys, virt) {
            if phys != 0 && allocator.free_page(phys) {
                reclaimed += 1;
            }
        }
    }
    reclaimed
}

fn page_mapped(pml4_phys: u64, virt: u64) -> bool {
    if pml4_phys == 0 || virt & (PAGE_SIZE - 1) != 0 {
        return false;
    }
    let Some(pdpt_phys) = table_entry_address(pml4_phys as *mut u64, pml4_index(virt)) else {
        return false;
    };
    let Some(pd_phys) = table_entry_address(pdpt_phys as *mut u64, pdpt_index(virt)) else {
        return false;
    };
    let Some(pt_phys) = table_entry_address(pd_phys as *mut u64, pd_index(virt)) else {
        return false;
    };
    table_entry_address(pt_phys as *mut u64, pt_index(virt)).is_some()
}

/// Like `map_user_pages`, but tolerant of pages that are already mapped --
/// needed because different `PT_LOAD` segments (e.g. a `PT_GNU_RELRO` slice
/// sharing a page with the preceding rodata segment) can legitimately share
/// a page-aligned page. Sharing is harmless here: page table entries are
/// always `PRESENT | WRITABLE` regardless of the ELF segment's nominal
/// permissions (this kernel doesn't enforce read-only/executable at the
/// page-table level yet), so a page mapped by an earlier segment is already
/// exactly as accessible as one this function would have mapped itself.
///
/// Doesn't roll back its own partial progress on failure -- the caller
/// (`load_program_elf_isolated`, via `spawn_prepared`) always cleans up
/// through `destroy_process_address_space`, which frees every page still
/// mapped in the process's private range regardless of which call put it
/// there.
fn map_image_pages(pml4_phys: u64, base: u64, page_count: usize) -> bool {
    for index in 0..page_count {
        let virt = base + index as u64 * PAGE_SIZE;
        if page_mapped(pml4_phys, virt) {
            continue;
        }
        let Some(phys) = alloc_zeroed_page() else {
            return false;
        };
        if !map_page(pml4_phys, virt, phys, PAGE_PRESENT | PAGE_WRITABLE) {
            let _ = free_phys_page(phys);
            return false;
        }
    }
    true
}

fn table_entry_address(table: *mut u64, index: usize) -> Option<u64> {
    unsafe {
        let entry = table.add(index).read_volatile();
        if entry & PAGE_PRESENT == 0 || entry & PAGE_HUGE != 0 {
            None
        } else {
            Some(entry & PAGE_ADDR_MASK)
        }
    }
}

fn clone_active_pml4() -> Option<u64> {
    let old_pml4 = read_cr3() & 0x000F_FFFF_FFFF_F000;
    let new_pml4 = alloc_zeroed_page()?;
    unsafe {
        copy_nonoverlapping(
            old_pml4 as *const u64,
            new_pml4 as *mut u64,
            PAGE_TABLE_ENTRIES,
        );
    }
    Some(new_pml4)
}

/// Gives a process its own private PML4 for the fixed program-image window
/// (`APP_LOAD_MIN..APP_LOAD_MAX`) while keeping everything else (kernel code,
/// heap/mmap windows, etc.) shared with the kernel's own page tables.
///
/// The image window falls under the same top-level PML4 entry and the same
/// PDPT entry as the kernel's own low-memory mappings (both are well under
/// 1 GiB), so a plain shallow PML4 clone isn't enough -- it would leave every
/// process pointing at the *same* PDPT/PD/PT chain for that region. Instead
/// this privatizes just the PDPT and PD covering the image window (copying
/// their entries first, so anything outside the image range -- like the
/// kernel's own PD entries -- keeps pointing at the original, shared page
/// tables) and clears the PD entries the image loader will fill in fresh.
///
/// `pid` is the *child's* PID, needed to pre-touch the top-level PML4 entries
/// for its heap/mmap windows (see `app_set_heap_window`'s address formula)
/// before cloning. A shallow PML4 clone only copies whatever top-level
/// entries already exist at clone time; if this is the first process ever to
/// use a given 512 GiB slice of the heap/mmap range, the entry for it
/// wouldn't exist yet in the kernel's own PML4, so the clone would capture a
/// blank slot -- and by the time this process later calls `mem_alloc_pages`
/// and the kernel's PML4 gains that entry, this process's *own* (already
/// cloned, separate physical page) copy has no way to find out. Pre-touching
/// first guarantees the entry exists in the shared kernel PML4 (and thus in
/// every future clone, including this one) before the copy happens.
fn create_process_address_space(pid: u32) -> Option<u64> {
    let kernel_pml4 = ensure_kernel_pml4()?;
    let heap_base = USER_HEAP_BASE + pid as u64 * USER_HEAP_STRIDE;
    let mmap_base = USER_MMAP_BASE + pid as u64 * USER_MMAP_STRIDE;
    let stack_base = KERNEL_STACK_BASE + pid as u64 * KERNEL_STACK_STRIDE;
    ensure_next_table(kernel_pml4 as *mut u64, pml4_index(heap_base))?;
    ensure_next_table(kernel_pml4 as *mut u64, pml4_index(mmap_base))?;
    // Pre-touch the stack window's top-level entry too, same as heap/mmap
    // above, *before* cloning the shared kernel PML4 below -- the clone is
    // a snapshot of the top-level entries as they exist right now, so
    // anything `create_process_stack` maps later (deeper page-table levels
    // only, not new top-level entries) stays visible through this
    // process's own private PML4 without needing to redo the clone.
    ensure_next_table(kernel_pml4 as *mut u64, pml4_index(stack_base))?;
    let new_pml4 = alloc_zeroed_page()?;
    unsafe {
        copy_nonoverlapping(
            kernel_pml4 as *const u64,
            new_pml4 as *mut u64,
            PAGE_TABLE_ENTRIES,
        );
    }

    let pml4_idx = pml4_index(APP_LOAD_MIN);
    let pdpt_idx = pdpt_index(APP_LOAD_MIN);
    let start_pd_index = pd_index(APP_LOAD_MIN);
    let end_pd_index = pd_index(APP_LOAD_MAX - PAGE_SIZE) + 1;

    let Some(shared_pdpt) = table_entry_address(new_pml4 as *mut u64, pml4_idx) else {
        free_phys_page(new_pml4);
        return None;
    };
    let Some(new_pdpt) = alloc_zeroed_page() else {
        free_phys_page(new_pml4);
        return None;
    };
    unsafe {
        copy_nonoverlapping(
            shared_pdpt as *const u64,
            new_pdpt as *mut u64,
            PAGE_TABLE_ENTRIES,
        );
    }

    let Some(shared_pd) = table_entry_address(new_pdpt as *mut u64, pdpt_idx) else {
        free_phys_page(new_pdpt);
        free_phys_page(new_pml4);
        return None;
    };
    let Some(new_pd) = alloc_zeroed_page() else {
        free_phys_page(new_pdpt);
        free_phys_page(new_pml4);
        return None;
    };
    unsafe {
        copy_nonoverlapping(
            shared_pd as *const u64,
            new_pd as *mut u64,
            PAGE_TABLE_ENTRIES,
        );
        let pd_ptr = new_pd as *mut u64;
        for pd_idx in start_pd_index..end_pd_index {
            pd_ptr.add(pd_idx).write_volatile(0);
        }
    }

    unsafe {
        (new_pdpt as *mut u64)
            .add(pdpt_idx)
            .write_volatile((new_pd & PAGE_ADDR_MASK) | PAGE_PRESENT | PAGE_WRITABLE);
        (new_pml4 as *mut u64)
            .add(pml4_idx)
            .write_volatile((new_pdpt & PAGE_ADDR_MASK) | PAGE_PRESENT | PAGE_WRITABLE);
    }

    Some(new_pml4)
}

/// Frees everything `create_process_address_space` allocated: the private
/// PD/PDPT/PML4 structural pages, plus any data pages still mapped in the
/// private image-window PD (the loader's segment pages, if the caller hasn't
/// already unmapped them another way). Shared structures (anything outside
/// the image window) are left untouched.
fn destroy_process_address_space(pml4_phys: u64) {
    if pml4_phys == 0 {
        return;
    }
    let pml4_idx = pml4_index(APP_LOAD_MIN);
    let pdpt_idx = pdpt_index(APP_LOAD_MIN);
    let start_pd_index = pd_index(APP_LOAD_MIN);
    let end_pd_index = pd_index(APP_LOAD_MAX - PAGE_SIZE) + 1;

    if let Some(pdpt_phys) = table_entry_address(pml4_phys as *mut u64, pml4_idx) {
        if let Some(pd_phys) = table_entry_address(pdpt_phys as *mut u64, pdpt_idx) {
            for pd_idx in start_pd_index..end_pd_index {
                if let Some(pt_phys) = table_entry_address(pd_phys as *mut u64, pd_idx) {
                    for pt_idx in 0..PAGE_TABLE_ENTRIES {
                        if let Some(data_phys) = table_entry_address(pt_phys as *mut u64, pt_idx) {
                            free_phys_page(data_phys);
                        }
                    }
                    free_phys_page(pt_phys);
                }
            }
            free_phys_page(pd_phys);
        }
        free_phys_page(pdpt_phys);
    }
    free_phys_page(pml4_phys);
}

/// Maps this pid's kernel stack -- `KERNEL_STACK_PAGES` pages at the *top*
/// of its `KERNEL_STACK_STRIDE` address slice, leaving the rest of the
/// slice unmapped as a guard gap (see the constants' docs) -- into the
/// shared kernel PML4, and returns the initial stack pointer: the top of
/// the mapped range (exclusive), 16-byte-aligned since it's page-aligned.
/// `create_process_address_space` must already have been called for this
/// pid (it pre-touches this window's top-level entry before cloning).
fn create_process_stack(pid: u32) -> Option<u64> {
    let kernel_pml4 = ensure_kernel_pml4()?;
    let stack_base = KERNEL_STACK_BASE + pid as u64 * KERNEL_STACK_STRIDE;
    let stack_top = stack_base + KERNEL_STACK_STRIDE;
    let mapped_base = stack_top - (KERNEL_STACK_PAGES as u64) * PAGE_SIZE;
    if !map_user_pages(kernel_pml4, mapped_base, KERNEL_STACK_PAGES) {
        return None;
    }
    Some(stack_top)
}

/// Frees a pid's kernel stack: the mapped data pages, plus the PT that
/// mapped them -- safe to free the whole PT (not just the data pages)
/// because `KERNEL_STACK_STRIDE` is exactly one PD entry's span, so this
/// pid never shares that PT with any other pid's stack.
fn reclaim_process_stack(pid: u32) {
    let Some(kernel_pml4) = ensure_kernel_pml4() else {
        return;
    };
    let stack_base = KERNEL_STACK_BASE + pid as u64 * KERNEL_STACK_STRIDE;
    let stack_top = stack_base + KERNEL_STACK_STRIDE;
    let mapped_base = stack_top - (KERNEL_STACK_PAGES as u64) * PAGE_SIZE;
    let _ = unmap_user_pages(kernel_pml4, mapped_base, KERNEL_STACK_PAGES);
    if let Some(pdpt_phys) = table_entry_address(kernel_pml4 as *mut u64, pml4_index(stack_base)) {
        let pdpt_idx = pdpt_index(stack_base);
        if let Some(pd_phys) = table_entry_address(pdpt_phys as *mut u64, pdpt_idx) {
            let pd_idx = pd_index(stack_base);
            if let Some(pt_phys) = table_entry_address(pd_phys as *mut u64, pd_idx) {
                free_phys_page(pt_phys);
                unsafe {
                    (pd_phys as *mut u64).add(pd_idx).write_volatile(0);
                    invlpg(mapped_base);
                }
            }
        }
    }
}

fn alloc_zeroed_page() -> Option<u64> {
    unsafe {
        let allocator = &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR);
        let page = allocator.alloc_page()?;
        write_bytes(page as *mut u8, 0, PAGE_SIZE as usize);
        Some(page)
    }
}

fn free_phys_page(phys: u64) -> bool {
    unsafe {
        let allocator = &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR);
        allocator.free_page(phys)
    }
}

fn page_is_zeroed(page: u64) -> bool {
    let ptr = page as *const u64;
    unsafe {
        for index in 0..PAGE_TABLE_ENTRIES {
            if ptr.add(index).read_volatile() != 0 {
                return false;
            }
        }
    }
    true
}

fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

fn read_tsc() -> u64 {
    let high: u32;
    let low: u32;
    unsafe {
        asm!("rdtsc", out("edx") high, out("eax") low, options(nomem, nostack, preserves_flags));
    }
    ((high as u64) << 32) | low as u64
}

/// Measures `rdtsc` ticks per second against the legacy PIT's channel 2,
/// entirely by polling -- no timer interrupt needed (this kernel still has
/// none, deliberately, see the IDT's docs). Classic technique: arm channel 2
/// for a known countdown, read `rdtsc` before and after, and see how many
/// ticks elapsed while a known amount of real time passed.
///
/// Repeats the one-shot countdown (the PIT's 16-bit counter caps a single
/// run at ~54.9 ms) a few times and accumulates both sides of the ratio
/// across all of them, rather than trusting one ~50 ms sample -- a single
/// short sample is more exposed to scheduling/emulation jitter around the
/// two `rdtsc` reads.
fn calibrate_tsc() -> u64 {
    const COUNT: u16 = 59_659; // ~50 ms at the PIT's fixed 1.193182 MHz input
    const ROUNDS: u32 = 4;

    let mut total_ticks: u64 = 0;
    for _ in 0..ROUNDS {
        unsafe {
            // Gate channel 2 on, speaker off.
            let nmi = inb(NMI_STATUS_CONTROL);
            outb(NMI_STATUS_CONTROL, (nmi & 0xFC) | 0x01);
            // Channel 2, lobyte/hibyte access, mode 0 (interrupt on terminal
            // count -- here just used as a one-shot countdown), binary.
            outb(PIT_COMMAND, 0xB0);
            outb(PIT_CHANNEL2_DATA, (COUNT & 0xFF) as u8);
            outb(PIT_CHANNEL2_DATA, (COUNT >> 8) as u8);
        }
        let start = read_tsc();
        // Bit 5 of the NMI status/control port is channel 2's OUT pin;
        // mode 0 holds it low until the countdown reaches zero.
        while unsafe { inb(NMI_STATUS_CONTROL) } & 0x20 == 0 {
            spin_loop();
        }
        let end = read_tsc();
        total_ticks += end.saturating_sub(start);
    }

    let total_seconds_numerator = COUNT as u64 * ROUNDS as u64;
    ((total_ticks as u128 * PIT_INPUT_HZ as u128) / total_seconds_numerator as u128) as u64
}

/// Nanoseconds elapsed since `BOOT_TSC`, using the calibrated `TSC_HZ`. Falls
/// back to treating raw ticks as nanoseconds if calibration hasn't run yet
/// (never actually observed -- `calibrate_tsc` runs before anything else
/// could call this -- but a fallback here is cheap and avoids a divide by
/// zero if that ever changed).
fn ns_since_boot() -> u64 {
    let hz = unsafe { TSC_HZ };
    let elapsed_ticks = read_tsc().saturating_sub(unsafe { BOOT_TSC });
    if hz == 0 {
        return elapsed_ticks;
    }
    ((elapsed_ticks as u128 * 1_000_000_000u128) / hz as u128) as u64
}

/// Real Unix time in nanoseconds: the CMOS RTC reading captured at boot,
/// plus nanoseconds elapsed since then. Returns 0 if the RTC couldn't be
/// read at boot (see `read_rtc_unix_seconds`), same convention as an
/// unset/uncalibrated clock elsewhere in this kernel.
fn unix_nanos_now() -> u64 {
    let boot = unsafe { BOOT_UNIX_NANOS };
    if boot == 0 {
        return 0;
    }
    boot + ns_since_boot()
}

fn read_cmos_register(register: u8) -> u8 {
    unsafe {
        outb(CMOS_INDEX, register);
        inb(CMOS_DATA)
    }
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0x0F) + ((value >> 4) * 10)
}

/// Days since 1970-01-01 for a given (year, month 1-12, day 1-31), in the
/// proleptic Gregorian calendar. Howard Hinnant's `days_from_civil`
/// algorithm (public domain) -- correct for any date, not just a simple
/// month-length table, without needing a full calendar library.
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let month_index = ((month as i64 + 9) % 12) as i64;
    let day_of_year = (153 * month_index + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Reads the current wall-clock time from the CMOS real-time clock, in
/// whole seconds since the Unix epoch. Returns 0 if the read is obviously
/// implausible (e.g. no RTC battery/chip at all), matching the "0 means
/// unset" convention `BOOT_UNIX_NANOS` already uses.
///
/// The CMOS year register is only two digits -- there's no standardized
/// "century" register firmware reliably populates across real hardware and
/// emulators alike, so this assumes the 21st century. Good enough for a
/// build timestamp or log entry; not a substitute for NTP.
fn read_rtc_unix_seconds() -> u64 {
    // The RTC can be mid-update when read; register 0x0A's top bit (UIP)
    // marks that. Wait for it to clear first (bounded, so a genuinely
    // absent/faulty RTC can't hang boot), then read all fields and confirm
    // a second read agrees -- the classic double-read technique, since UIP
    // can also flip on the boundary right after we saw it clear.
    for _ in 0..100_000 {
        if read_cmos_register(0x0A) & 0x80 == 0 {
            break;
        }
        spin_loop();
    }

    let read_fields = || {
        (
            read_cmos_register(0x00), // seconds
            read_cmos_register(0x02), // minutes
            read_cmos_register(0x04), // hours
            read_cmos_register(0x07), // day of month
            read_cmos_register(0x08), // month
            read_cmos_register(0x09), // year (2-digit)
        )
    };
    let mut fields = read_fields();
    for _ in 0..8 {
        let again = read_fields();
        if again == fields {
            break;
        }
        fields = again;
    }
    let (mut second, mut minute, mut hour, mut day, mut month, mut year) = fields;

    let status_b = read_cmos_register(0x0B);
    if status_b & 0x04 == 0 {
        // BCD mode (the common default): every field above is packed BCD.
        second = bcd_to_binary(second);
        minute = bcd_to_binary(minute);
        let pm = hour & 0x80 != 0;
        hour = bcd_to_binary(hour & 0x7F);
        if status_b & 0x02 == 0 && pm {
            hour = (hour % 12) + 12;
        }
        day = bcd_to_binary(day);
        month = bcd_to_binary(month);
        year = bcd_to_binary(year);
    }

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return 0;
    }

    let full_year = 2000i64 + year as i64;
    let days = days_from_civil(full_year, month, day);
    let seconds_of_day = hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    (days * 86_400 + seconds_of_day).max(0) as u64
}

unsafe fn write_cr3(value: u64) {
    unsafe {
        asm!("mov cr3, {}", in(reg) value, options(nostack, preserves_flags));
    }
}

/// The classic minimal fiber-switch: push the outgoing context's
/// callee-saved registers onto its *own* stack, save the resulting `rsp`
/// into `*prev`, load `rsp` from `*next`, then pop what's sitting there
/// (either callee-saved registers a previous switch *into* that context
/// pushed, or -- for a brand-new process that's never run before -- the
/// hand-built initial frame `create_process_stack`'s caller writes; see
/// `spawn_prepared`) and `ret` into whatever return address is on top.
///
/// `rdi`/`rsi` hold `prev`/`next` per the SysV calling convention -- this
/// is a real function call (`switch_to` calls it normally), not an
/// interrupt, so unlike the ISR stubs above this one *must* return
/// (`ret`), just not necessarily to its own caller: it returns to whatever
/// instruction address was sitting at `*next`'s stack top, which is a
/// *different* suspended call to this exact function (or, for a new
/// process, `process_trampoline`).
#[unsafe(naked)]
unsafe extern "sysv64" fn context_switch(prev: *mut CpuContext, next: *const CpuContext) {
    core::arch::naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",
        "mov rsp, [rsi]",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "ret",
    )
}

/// Switches execution to `next_index` (a real `PROCESS_TABLE` slot, or
/// `PROCESS_COUNT` for the reserved idle/kernel-console slot), saving the
/// currently-running context first. Does not return to its caller until
/// something *else* later switches back to whichever index called this --
/// see `context_switch`'s docs. Handles the CR3 switch too (the idle slot's
/// `pml4_phys` is always 0, meaning "use the shared kernel PML4").
fn switch_to(next_index: usize) {
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        if next_index < PROCESS_COUNT {
            (*table)[next_index].state = ProcessState::Running;
        }
        let prev_index = CURRENT_PROCESS_INDEX;
        CURRENT_PROCESS_INDEX = next_index;
        let next_pml4 = (*table)[next_index].pml4_phys;
        let target_pml4 = if next_pml4 != 0 {
            next_pml4
        } else {
            ensure_kernel_pml4().unwrap_or(0)
        };
        if target_pml4 != 0 {
            write_cr3(target_pml4);
        }
        let prev_ctx = core::ptr::addr_of_mut!((*table)[prev_index].context);
        let next_ctx = core::ptr::addr_of!((*table)[next_index].context);
        context_switch(prev_ctx, next_ctx);
    }
    // However this particular call to `switch_to` returned -- resumed
    // later, on a genuinely different stack than whatever most recently
    // exited -- it's always safe right here to reclaim any exited
    // process's stack that couldn't free its own (see `process_trampoline`,
    // `queue_stack_reclaim`).
    reclaim_pending_stacks();
}

/// Pids whose kernel stack `process_trampoline` couldn't free itself
/// (can't free the stack you're currently executing on) -- see
/// `queue_stack_reclaim`/`reclaim_pending_stacks`, drained by `switch_to`.
static mut PENDING_STACK_RECLAIMS: [Option<u32>; PROCESS_COUNT] = [None; PROCESS_COUNT];

/// Queues `pid`'s kernel stack to be freed once we're safely running on a
/// different one -- see `switch_to`'s post-`context_switch` call to
/// `reclaim_pending_stacks`. Sized to `PROCESS_COUNT` since at most that
/// many processes could ever be simultaneously mid-exit-and-not-yet-
/// reclaimed; silently drops (leaking one 128 KiB stack window, never
/// corrupting anything) in the should-never-happen case that's exceeded.
fn queue_stack_reclaim(pid: u32) {
    unsafe {
        let slots = &mut *core::ptr::addr_of_mut!(PENDING_STACK_RECLAIMS);
        for slot in slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(pid);
                return;
            }
        }
    }
}

fn reclaim_pending_stacks() {
    unsafe {
        let slots = &mut *core::ptr::addr_of_mut!(PENDING_STACK_RECLAIMS);
        for slot in slots.iter_mut() {
            if let Some(pid) = slot.take() {
                reclaim_process_stack(pid);
            }
        }
    }
}

fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        asm!("pushfq", "pop {}", out(reg) flags, options(preserves_flags));
    }
    flags & (1 << 9) != 0
}

/// Runs `f` with interrupts disabled, restoring the *actual* previous `IF`
/// state afterward rather than unconditionally re-enabling -- safe to nest,
/// and safe to call from a context that's already interrupt-disabled (e.g.
/// once category 2's stage 3 timer ISR exists and calls into this same
/// scheduler core with interrupts already off on entry). No interrupt
/// exists yet in stage 2 (`sti` is never called anywhere), so this is a
/// no-op in practice today -- but writing every scheduler critical section
/// this way from day one means stage 3 is an addition to this code, not a
/// rewrite of it.
fn without_interrupts<T>(f: impl FnOnce() -> T) -> T {
    let was_enabled = interrupts_enabled();
    if was_enabled {
        unsafe { asm!("cli", options(nomem, nostack, preserves_flags)) };
    }
    let result = f();
    if was_enabled {
        unsafe { asm!("sti", options(nomem, nostack, preserves_flags)) };
    }
    result
}

/// The next `Ready` process to run, scanning `PROCESS_TABLE` in index order
/// (a plain scan is plenty given `PROCESS_COUNT` = 16 and no priority
/// concept yet). Never returns the reserved idle slot -- that's the
/// fallback when nothing else is runnable, not something to "pick".
fn pick_next_ready() -> Option<usize> {
    without_interrupts(|| unsafe {
        let table = &raw const PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            if (*table)[index].state == ProcessState::Ready {
                return Some(index);
            }
        }
        None
    })
}

/// Moves every process `Blocked` on `pid` (or blocked on "any child," via
/// `WaitTarget::AnyChild`) back to `Ready` so `pick_next_ready` can find it
/// again. Called whenever a process exits (`exit_and_yield`) -- see
/// `abi_wait`/`abi_wait_any` for the other half (the actual condition
/// re-check, once resumed).
fn wake_waiters_for(pid: u32) {
    without_interrupts(|| unsafe {
        let table = &raw mut PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            let process = &mut (*table)[index];
            if process.state != ProcessState::Blocked {
                continue;
            }
            let should_wake = match process.waiting_for {
                WaitTarget::Pid(target) => target == pid,
                WaitTarget::AnyChild => true,
                WaitTarget::None => false,
            };
            if should_wake {
                process.waiting_for = WaitTarget::None;
                process.state = ProcessState::Ready;
            }
        }
    });
}

/// Marks the current process's exit, wakes anything waiting on its pid,
/// then hands the CPU to whatever should run next. Called from
/// `process_trampoline` once a process's entry point returns. Nothing will
/// ever `pick_next_ready` an `Exited` process, so this never meaningfully
/// resumes its own now-dead context again -- but it's written as a genuine
/// loop (not relying on that cleverness) to satisfy `-> !` honestly.
fn exit_and_yield(exit_index: usize, code: i32) -> ! {
    let pid = process_pid(exit_index);
    process_set_state(exit_index, ProcessState::Exited, code);
    wake_waiters_for(pid);
    loop {
        let next = pick_next_ready().unwrap_or(PROCESS_COUNT);
        switch_to(next);
    }
}

/// Entry point for a brand-new process's very first time being switched
/// into -- `spawn_prepared` hand-builds the initial `CpuContext` to point
/// here (see its docs). Runs the process's real ELF entry point on the
/// process's own kernel stack; CR3 is already switched to its private PML4
/// by `switch_to` before this ever starts running, so no address-space
/// work is needed here on the way in. Once the entry point returns, tears
/// down everything `spawn_prepared` built for it and hands off to the
/// scheduler -- never returns to a "caller" in the traditional sense (see
/// `exit_and_yield`). Unlike the old synchronous `run_ready_task`, this
/// does *not* drain any children the process spawned but never waited on:
/// under real (even just cooperative) scheduling, an orphaned child simply
/// keeps existing in the process table and gets scheduled normally by
/// whatever later calls `pick_next_ready`, regardless of whether its
/// original parent already exited -- there is no nested call stack relying
/// on it finishing first anymore.
extern "sysv64" fn process_trampoline() -> ! {
    let index = unsafe { CURRENT_PROCESS_INDEX };
    let (entry, pid, pml4_phys) = unsafe {
        let table = &raw const PROCESS_TABLE;
        let process = &(*table)[index];
        (process.entry_point, process.pid, process.pml4_phys)
    };

    let code = unsafe {
        let program: extern "sysv64" fn(*const RymosAbi) -> i32 = core::mem::transmute(entry);
        program(&RYMOS_ABI)
    };

    let reclaimed = process_reclaim_mappings(index);
    reclaim_process_window_tables(pid);
    destroy_process_address_space(pml4_phys);
    // `destroy_process_address_space` just freed the physical page CR3 is
    // still pointing at -- switch back to the shared kernel PML4
    // immediately so nothing runs even briefly with a dangling CR3 (the
    // freed page could be reallocated for something else at any point
    // after this).
    if let Some(kernel_pml4) = ensure_kernel_pml4() {
        unsafe {
            write_cr3(kernel_pml4);
        }
    }
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        (*table)[index].pml4_phys = 0;
    }
    // Can't free *this* process's own stack while still standing on it --
    // queue it, and whichever switch_to next actually returns somewhere
    // (running on a different stack by definition) reclaims it then. See
    // `queue_stack_reclaim`/`reclaim_pending_stacks`.
    queue_stack_reclaim(pid);
    if reclaimed > 0 {
        if let Some(console) = unsafe { APP_CONSOLE.as_mut() } {
            console.write("heap reclaimed ");
            console.write_usize(reclaimed);
            console.write_line(" pages");
        }
    }

    exit_and_yield(index, code)
}

unsafe fn invlpg(virt: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
    }
}

fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}

fn pdpt_index(virt: u64) -> usize {
    ((virt >> 30) & 0x1FF) as usize
}

fn pd_index(virt: u64) -> usize {
    ((virt >> 21) & 0x1FF) as usize
}

fn pt_index(virt: u64) -> usize {
    ((virt >> 12) & 0x1FF) as usize
}

fn pfs_entry_used(header: &[u8; PFS_HEADER_BYTES], index: usize) -> bool {
    header[pfs_entry_offset(index)] != 0
}

fn pfs_entry_is_dir(header: &[u8; PFS_HEADER_BYTES], index: usize) -> bool {
    header[pfs_entry_offset(index)] == PFS_KIND_DIR
}

fn pfs_entry_kind(header: &[u8; PFS_HEADER_BYTES], index: usize) -> u8 {
    header[pfs_entry_offset(index)]
}

fn pfs_entry_name<'a>(header: &'a [u8; PFS_HEADER_BYTES], index: usize) -> &'a [u8] {
    let offset = pfs_entry_offset(index);
    let len = header[offset + 1] as usize;
    &header[offset + PFS_NAME_OFFSET..offset + PFS_NAME_OFFSET + len]
}

fn pfs_entry_size(header: &[u8; PFS_HEADER_BYTES], index: usize) -> usize {
    let offset = pfs_entry_offset(index);
    read_le32_from_slice(&header[offset + 2..offset + 6]) as usize
}

fn pfs_entry_extent_count(header: &[u8; PFS_HEADER_BYTES], index: usize) -> usize {
    let offset = pfs_entry_offset(index);
    (header[offset + PFS_EXTENT_COUNT_OFFSET] as usize).min(PFS_MAX_EXTENTS)
}

fn pfs_entry_extent(header: &[u8; PFS_HEADER_BYTES], index: usize, slot: usize) -> PfsExtent {
    let offset = pfs_entry_offset(index) + PFS_EXTENTS_OFFSET + slot * PFS_EXTENT_ENTRY_SIZE;
    PfsExtent {
        start: read_le32_from_slice(&header[offset..offset + 4]),
        sectors: read_le32_from_slice(&header[offset + 4..offset + 8]),
    }
}

/// Total sectors actually allocated to this entry across every extent. Can
/// be more than `sectors_for_len(pfs_entry_size(...))` when a file has grown
/// and shrunk without ever being fully rewritten -- allocation only ever
/// grows in place (see `pfs_ensure_file_capacity`), it never shrinks a live
/// entry's extents just because the logical size dropped.
fn pfs_entry_total_sectors(header: &[u8; PFS_HEADER_BYTES], index: usize) -> u32 {
    let count = pfs_entry_extent_count(header, index);
    let mut total = 0u32;
    for slot in 0..count {
        total += pfs_entry_extent(header, index, slot).sectors;
    }
    total
}

/// Translates a logical (0-based, whole-file) sector index into the real
/// disk LBA it lives at, by walking the entry's extent list. Replaces the
/// old `start_sector + logical_sector` arithmetic now that a file's sectors
/// aren't necessarily one contiguous run.
fn pfs_entry_lba_for_sector(
    header: &[u8; PFS_HEADER_BYTES],
    index: usize,
    logical_sector: u32,
) -> Option<u32> {
    let extent_count = pfs_entry_extent_count(header, index);
    let mut remaining = logical_sector;
    for slot in 0..extent_count {
        let extent = pfs_entry_extent(header, index, slot);
        if extent.sectors == 0 {
            continue;
        }
        if remaining < extent.sectors {
            return Some(extent.start + remaining);
        }
        remaining -= extent.sectors;
    }
    None
}

fn pfs_entry_created(header: &[u8; PFS_HEADER_BYTES], index: usize) -> u64 {
    let offset = pfs_entry_offset(index) + PFS_CREATED_OFFSET;
    read_le64_from_slice(&header[offset..offset + 8])
}

fn pfs_entry_modified(header: &[u8; PFS_HEADER_BYTES], index: usize) -> u64 {
    let offset = pfs_entry_offset(index) + PFS_MODIFIED_OFFSET;
    read_le64_from_slice(&header[offset..offset + 8])
}

fn pfs_entry_mode(header: &[u8; PFS_HEADER_BYTES], index: usize) -> u8 {
    let offset = pfs_entry_offset(index);
    header[offset + PFS_MODE_OFFSET]
}

fn pfs_touch_modified(header: &mut [u8; PFS_HEADER_BYTES], index: usize) {
    let offset = pfs_entry_offset(index) + PFS_MODIFIED_OFFSET;
    header[offset..offset + 8].copy_from_slice(&ns_since_boot().to_le_bytes());
}

fn pfs_find_entry(header: &[u8; PFS_HEADER_BYTES], name: &[u8]) -> Option<usize> {
    for index in 0..PFS_ENTRY_COUNT {
        if pfs_entry_used(header, index) && pfs_entry_name(header, index) == name {
            return Some(index);
        }
    }
    None
}

fn pfs_parent_exists(header: &[u8; PFS_HEADER_BYTES], name: &[u8]) -> bool {
    let Some(slash) = find_last_byte(name, b'/') else {
        return true;
    };
    if slash == 0 || slash + 1 >= name.len() {
        return false;
    }
    let parent = &name[..slash];
    let Some(index) = pfs_find_entry(header, parent) else {
        return false;
    };
    pfs_entry_is_dir(header, index)
}

fn pfs_dir_empty(header: &[u8; PFS_HEADER_BYTES], dir: &[u8]) -> bool {
    for index in 0..PFS_ENTRY_COUNT {
        if !pfs_entry_used(header, index) {
            continue;
        }
        let name = pfs_entry_name(header, index);
        if name.len() > dir.len() && starts_with(name, dir) && name[dir.len()] == b'/' {
            return false;
        }
    }
    true
}

fn pfs_free_entry(header: &[u8; PFS_HEADER_BYTES]) -> Option<usize> {
    for index in 0..PFS_ENTRY_COUNT {
        if !pfs_entry_used(header, index) {
            return Some(index);
        }
    }
    None
}

fn pfs_set_entry(
    header: &mut [u8; PFS_HEADER_BYTES],
    index: usize,
    name: &[u8],
    size: usize,
    kind: u8,
    extents: &[PfsExtent],
    created_ticks: u64,
    mode: u8,
) {
    let offset = pfs_entry_offset(index);
    // Calibrated ns-since-boot, same unit `time_ticks`'s ABI call reports --
    // not raw `read_tsc()` cycles, which userspace has no way to convert
    // back to real time (it only ever sees `time_ticks`/`time_unix_nanos`,
    // never `TSC_HZ`/`BOOT_TSC`).
    let now = ns_since_boot();
    header[offset..offset + PFS_ENTRY_SIZE].fill(0);
    header[offset] = kind;
    header[offset + 1] = name.len() as u8;
    header[offset + 2..offset + 6].copy_from_slice(&(size as u32).to_le_bytes());
    let extent_count = extents.len().min(PFS_MAX_EXTENTS);
    header[offset + PFS_EXTENT_COUNT_OFFSET] = extent_count as u8;
    for (slot, extent) in extents.iter().take(PFS_MAX_EXTENTS).enumerate() {
        let extent_offset = offset + PFS_EXTENTS_OFFSET + slot * PFS_EXTENT_ENTRY_SIZE;
        header[extent_offset..extent_offset + 4].copy_from_slice(&extent.start.to_le_bytes());
        header[extent_offset + 4..extent_offset + 8].copy_from_slice(&extent.sectors.to_le_bytes());
    }
    let created_offset = offset + PFS_CREATED_OFFSET;
    header[created_offset..created_offset + 8].copy_from_slice(&created_ticks.to_le_bytes());
    let modified_offset = offset + PFS_MODIFIED_OFFSET;
    header[modified_offset..modified_offset + 8].copy_from_slice(&now.to_le_bytes());
    header[offset + PFS_MODE_OFFSET] = mode;
    header[offset + PFS_NAME_OFFSET..offset + PFS_NAME_OFFSET + name.len()].copy_from_slice(name);
}

fn pfs_clear_entry(header: &mut [u8; PFS_HEADER_BYTES], index: usize) {
    let offset = pfs_entry_offset(index);
    header[offset..offset + PFS_ENTRY_SIZE].fill(0);
}

fn pfs_unlink_header(header: &mut [u8; PFS_HEADER_BYTES], name: &[u8]) -> Result<(), i32> {
    let Some(index) = pfs_find_entry(header, name) else {
        return Err(-1);
    };
    if pfs_entry_is_dir(header, index) && !pfs_dir_empty(header, name) {
        return Err(-2);
    }
    pfs_clear_entry(header, index);
    Ok(())
}

fn pfs_rename_header(
    header: &mut [u8; PFS_HEADER_BYTES],
    old_name: &[u8],
    new_name: &[u8],
) -> Result<(), i32> {
    if !valid_pfs_path(old_name) || !valid_pfs_path(new_name) {
        return Err(-1);
    }
    let Some(index) = pfs_find_entry(header, old_name) else {
        return Err(-2);
    };
    if pfs_find_entry(header, new_name).is_some() {
        return Err(-3);
    }
    if !pfs_parent_exists(header, new_name) {
        return Err(-4);
    }
    if pfs_entry_is_dir(header, index) && !pfs_dir_empty(header, old_name) {
        return Err(-5);
    }
    let kind = pfs_entry_kind(header, index);
    let size = pfs_entry_size(header, index);
    let extent_count = pfs_entry_extent_count(header, index);
    let mut extents = [PfsExtent::empty(); PFS_MAX_EXTENTS];
    for slot in 0..extent_count {
        extents[slot] = pfs_entry_extent(header, index, slot);
    }
    let created = pfs_entry_created(header, index);
    let mode = pfs_entry_mode(header, index);
    pfs_set_entry(
        header,
        index,
        new_name,
        size,
        kind,
        &extents[..extent_count],
        created,
        mode,
    );
    Ok(())
}

fn valid_pfs_path(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > PFS_NAME_MAX {
        return false;
    }
    let mut last_was_slash = false;
    for byte in name {
        if *byte == b'/' {
            if last_was_slash {
                return false;
            }
            last_was_slash = true;
            continue;
        }
        last_was_slash = false;
        if !(byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'_' || *byte == b'-') {
            return false;
        }
    }
    !last_was_slash
}

fn pfs_list_match<'a>(namespace: &[u8], name: &'a [u8]) -> Option<&'a [u8]> {
    if namespace.is_empty() || eq(namespace, b"pfs:") {
        if contains(name, b'/') {
            return None;
        }
        return Some(name);
    }

    let prefix = if starts_with(namespace, b"pfs:") {
        &namespace[4..]
    } else {
        namespace
    };
    if prefix.is_empty() {
        return None;
    }
    if !starts_with(name, prefix) || name.len() <= prefix.len() || name[prefix.len()] != b'/' {
        return None;
    }
    let child = &name[prefix.len() + 1..];
    if child.is_empty() || contains(child, b'/') {
        return None;
    }
    Some(child)
}

fn set_app_error(error: i32) -> i32 {
    unsafe {
        PROCESS_TABLE[CURRENT_PROCESS_INDEX].last_error = error;
    }
    -1
}

fn app_resolve_fd(fd: i32) -> i32 {
    if (STDIN_FD..=STDERR_FD).contains(&fd) {
        unsafe {
            let std_fds = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].std_fds);
            return (*std_fds)[fd as usize];
        }
    }
    fd
}

fn app_fd_open(fd: i32) -> bool {
    if (STDIN_FD..=STDERR_FD).contains(&fd) {
        return true;
    }
    let Some(index) = app_fd_index(fd) else {
        return false;
    };
    unsafe {
        let table = core::ptr::addr_of!(APP_FDS);
        (*table)[index].open
    }
}

fn clear_app_error() {
    unsafe {
        PROCESS_TABLE[CURRENT_PROCESS_INDEX].last_error = ERR_OK;
    }
}

fn app_cwd_is_pfs() -> bool {
    unsafe {
        let cwd = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd);
        PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len >= 4 && &(&(*cwd))[..4] == b"pfs:"
    }
}

fn app_cwd_pfs_name<'a>(buffer: &'a mut [u8; PFS_NAME_MAX]) -> &'a [u8] {
    unsafe {
        let cwd = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd);
        if PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len <= 4 {
            return &buffer[..0];
        }
        let len = min(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len - 4, PFS_NAME_MAX);
        buffer[..len].copy_from_slice(&(&(*cwd))[4..4 + len]);
        &buffer[..len]
    }
}

fn pfs_resolve_path(path: &[u8], output: &mut [u8; PFS_NAME_MAX]) -> Option<usize> {
    output.fill(0);
    if starts_with(path, b"pfs:") {
        return normalize_pfs_path(b"", &path[4..], output);
    }
    if app_cwd_is_pfs() {
        let mut cwd_name = [0u8; PFS_NAME_MAX];
        let base = app_cwd_pfs_name(&mut cwd_name);
        return normalize_pfs_path(base, path, output);
    }
    None
}

fn normalize_pfs_path(base: &[u8], path: &[u8], output: &mut [u8; PFS_NAME_MAX]) -> Option<usize> {
    let mut len = 0usize;
    if !path.starts_with(b"/") {
        for component in PfsComponents::new(base) {
            len = pfs_push_component(output, len, component)?;
        }
    }
    let relative = if path.starts_with(b"/") {
        &path[1..]
    } else {
        path
    };
    for component in PfsComponents::new(relative) {
        if eq(component, b".") {
            continue;
        }
        if eq(component, b"..") {
            len = pfs_pop_component(output, len);
            continue;
        }
        len = pfs_push_component(output, len, component)?;
    }
    if len == 0 || !valid_pfs_path(&output[..len]) {
        return None;
    }
    Some(len)
}

fn pfs_push_component(
    output: &mut [u8; PFS_NAME_MAX],
    mut len: usize,
    component: &[u8],
) -> Option<usize> {
    if component.is_empty() || !valid_pfs_component(component) {
        return None;
    }
    if len != 0 {
        if len >= PFS_NAME_MAX {
            return None;
        }
        output[len] = b'/';
        len += 1;
    }
    if len + component.len() > PFS_NAME_MAX {
        return None;
    }
    output[len..len + component.len()].copy_from_slice(component);
    Some(len + component.len())
}

fn pfs_pop_component(output: &mut [u8; PFS_NAME_MAX], len: usize) -> usize {
    for index in (0..len).rev() {
        if output[index] == b'/' {
            output[index..len].fill(0);
            return index;
        }
    }
    output[..len].fill(0);
    0
}

fn valid_pfs_component(component: &[u8]) -> bool {
    if component.is_empty() {
        return false;
    }
    for byte in component {
        if !(byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'_' || *byte == b'-') {
            return false;
        }
    }
    true
}

struct PfsComponents<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> PfsComponents<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, index: 0 }
    }
}

impl<'a> Iterator for PfsComponents<'a> {
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

fn find_last_byte(bytes: &[u8], needle: u8) -> Option<usize> {
    let mut index = bytes.len();
    while index > 0 {
        index -= 1;
        if bytes[index] == needle {
            return Some(index);
        }
    }
    None
}

fn read_le32_from_slice(bytes: &[u8]) -> u32 {
    bytes[0] as u32
        | ((bytes[1] as u32) << 8)
        | ((bytes[2] as u32) << 16)
        | ((bytes[3] as u32) << 24)
}

fn read_le64_from_slice(bytes: &[u8]) -> u64 {
    let mut value: u64 = 0;
    for index in 0..8 {
        value |= (bytes[index] as u64) << (index * 8);
    }
    value
}

#[derive(Clone, Copy)]
struct BootFs {
    base: *const u8,
    size: usize,
}

#[derive(Clone, Copy)]
struct BootFsEntry {
    kind: u8,
    name: *const u8,
    name_len: usize,
    data: *const u8,
    data_len: usize,
}

impl BootFs {
    unsafe fn new(boot_info: *const BootInfo) -> Self {
        let Some(info) = (unsafe { boot_info.as_ref() }) else {
            return Self::empty();
        };
        if info.initrd_base == 0 || info.initrd_size == 0 {
            return Self::empty();
        }
        Self {
            base: info.initrd_base as *const u8,
            size: info.initrd_size,
        }
    }

    const fn empty() -> Self {
        Self {
            base: core::ptr::null(),
            size: 0,
        }
    }

    fn valid(&self) -> bool {
        if self.base.is_null() || self.size < 12 {
            return false;
        }
        unsafe { core::slice::from_raw_parts(self.base, 8) == b"RYFS1\0\0\0" }
    }

    fn entry_count(&self) -> usize {
        if !self.valid() {
            return 0;
        }
        unsafe {
            let lo = *self.base.add(8) as usize;
            let hi = *self.base.add(9) as usize;
            lo | (hi << 8)
        }
    }

    fn entry_size(&self) -> usize {
        if !self.valid() {
            return 0;
        }
        unsafe {
            let lo = *self.base.add(10) as usize;
            let hi = *self.base.add(11) as usize;
            lo | (hi << 8)
        }
    }

    fn ls(&self, console: &mut Console) {
        if !self.valid() {
            console.write_line("bootfs: not loaded");
            return;
        }
        for index in 0..self.entry_count() {
            if let Some(entry) = self.entry(index) {
                if entry.kind == 2 {
                    console.write("[dir]  ");
                } else {
                    console.write("[file] ");
                }
                unsafe {
                    console.write_bytes(core::slice::from_raw_parts(entry.name, entry.name_len));
                }
                console.write("  ");
                console.write_usize(entry.data_len);
                console.write_line(" B");
            }
        }
    }

    fn cat(&self, console: &mut Console, name: &[u8]) {
        if !self.valid() {
            console.write_line("bootfs: not loaded");
            return;
        }
        if let Some(data) = self.find_data(name) {
            console.write_bytes(data);
            console.new_line();
            return;
        }
        console.write_line("bootcat: not found");
    }

    fn find_data(&self, name: &[u8]) -> Option<&[u8]> {
        if !self.valid() {
            return None;
        }
        for index in 0..self.entry_count() {
            if let Some(entry) = self.entry(index) {
                let entry_name = unsafe { core::slice::from_raw_parts(entry.name, entry.name_len) };
                if entry_name == name {
                    return Some(unsafe {
                        core::slice::from_raw_parts(entry.data, entry.data_len)
                    });
                }
            }
        }
        None
    }

    fn find_entry(&self, name: &[u8]) -> Option<BootFsEntry> {
        if !self.valid() {
            return None;
        }
        for index in 0..self.entry_count() {
            if let Some(entry) = self.entry(index) {
                let entry_name = unsafe { core::slice::from_raw_parts(entry.name, entry.name_len) };
                if entry_name == name {
                    return Some(entry);
                }
            }
        }
        None
    }

    fn used_bytes(&self) -> usize {
        if self.valid() { self.size } else { 0 }
    }

    fn entry(&self, index: usize) -> Option<BootFsEntry> {
        let entry_size = self.entry_size();
        if entry_size < 42 {
            return None;
        }
        let offset = 12 + index * entry_size;
        if offset + 42 > self.size {
            return None;
        }
        unsafe {
            let ptr = self.base.add(offset);
            let kind = *ptr;
            let name_len = *ptr.add(1) as usize;
            let data_offset = read_le32(ptr.add(2)) as usize;
            let data_len = read_le32(ptr.add(6)) as usize;
            if name_len > 32 || data_offset + data_len > self.size {
                return None;
            }
            Some(BootFsEntry {
                kind,
                name: ptr.add(10),
                name_len,
                data: self.base.add(data_offset),
                data_len,
            })
        }
    }
}

impl RamFs {
    const fn new() -> Self {
        Self {
            entries: [FileEntry::empty(); FILE_COUNT],
        }
    }

    fn seed(&mut self) {
        self.write_raw(b"readme", b"Welcome to RYMOS RAMFS.");
        self.write_raw(
            b"notes",
            b"Disk FAT32 loading works in the bootloader. Kernel FAT32 comes later.",
        );
        self.touch_raw(b"tmp", true);
    }

    fn ls(&self, console: &mut Console) {
        for entry in &self.entries {
            if entry.used {
                if entry.is_dir {
                    console.write("[dir]  ");
                } else {
                    console.write("[file] ");
                }
                console.write_bytes(&entry.name[..entry.name_len]);
                if !entry.is_dir {
                    console.write("  ");
                    console.write_usize(entry.data_len);
                    console.write(" B");
                }
                console.new_line();
            }
        }
    }

    fn df(&self, console: &mut Console, bootfs: BootFs) {
        let mut used_entries = 0usize;
        let mut used_bytes = 0usize;
        for entry in &self.entries {
            if entry.used {
                used_entries += 1;
                used_bytes += entry.data_len;
            }
        }

        console.write_line("filesystem   entries        bytes");
        console.write("ramfs        ");
        console.write_usize(used_entries);
        console.write("/");
        console.write_usize(FILE_COUNT);
        console.write("          ");
        console.write_usize(used_bytes);
        console.write("/");
        console.write_usize(FILE_COUNT * FILE_DATA_MAX);
        console.new_line();
        console.write("bootfs       ro initrd      ");
        console.write_usize(bootfs.used_bytes());
        console.write_line(" bytes");
    }

    fn cat(&self, console: &mut Console, name: &[u8]) {
        match self.find(name) {
            Some(index) if self.entries[index].is_dir => console.write_line("cat: is a directory"),
            Some(index) => {
                let entry = &self.entries[index];
                console.write_bytes(&entry.data[..entry.data_len]);
                console.new_line();
            }
            None => console.write_line("cat: not found"),
        }
    }

    fn touch(&mut self, console: &mut Console, name: &[u8], is_dir: bool) {
        if !valid_name(name) {
            console.write_line("invalid name");
            return;
        }
        if self.touch_raw(name, is_dir) {
            console.write_line("ok");
        } else {
            console.write_line("ramfs full");
        }
    }

    fn write(&mut self, console: &mut Console, name: &[u8], text: &[u8]) {
        if !valid_name(name) {
            console.write_line("invalid name");
            return;
        }
        if text.len() > FILE_DATA_MAX {
            console.write_line("write: text too large");
            return;
        }
        self.write_raw(name, text);
        console.write_line("ok");
    }

    fn remove(&mut self, console: &mut Console, name: &[u8], dir: bool) {
        match self.find(name) {
            Some(index) if self.entries[index].is_dir == dir => {
                self.entries[index] = FileEntry::empty();
                console.write_line("ok");
            }
            Some(_) if dir => console.write_line("rmdir: not a directory"),
            Some(_) => console.write_line("rm: is a directory"),
            None => console.write_line("not found"),
        }
    }

    fn append(&mut self, console: &mut Console, name: &[u8], text: &[u8]) {
        if !valid_name(name) {
            console.write_line("invalid name");
            return;
        }
        let Some(index) = self.find(name) else {
            self.write(console, name, text);
            return;
        };
        if self.entries[index].is_dir {
            console.write_line("append: is a directory");
            return;
        }
        if self.entries[index].data_len + text.len() > FILE_DATA_MAX {
            console.write_line("append: text too large");
            return;
        }
        let start = self.entries[index].data_len;
        self.entries[index].data[start..start + text.len()].copy_from_slice(text);
        self.entries[index].data_len += text.len();
        console.write_line("ok");
    }

    fn copy(&mut self, console: &mut Console, src: &[u8], dst: &[u8]) {
        if !valid_name(dst) {
            console.write_line("invalid destination");
            return;
        }
        let Some(index) = self.find(src) else {
            console.write_line("copy: source not found");
            return;
        };
        if self.entries[index].is_dir {
            console.write_line("copy: directories not supported yet");
            return;
        }
        let data_len = self.entries[index].data_len;
        let data = self.entries[index].data;
        self.write_raw(dst, &data[..data_len]);
        console.write_line("ok");
    }

    fn move_entry(&mut self, console: &mut Console, src: &[u8], dst: &[u8]) {
        if !valid_name(dst) {
            console.write_line("invalid destination");
            return;
        }
        let Some(index) = self.find(src) else {
            console.write_line("move: source not found");
            return;
        };
        if dst.len() > FILE_NAME_MAX {
            console.write_line("move: destination too long");
            return;
        }
        self.entries[index].name = [0; FILE_NAME_MAX];
        self.entries[index].name_len = dst.len();
        self.entries[index].name[..dst.len()].copy_from_slice(dst);
        console.write_line("ok");
    }

    fn touch_raw(&mut self, name: &[u8], is_dir: bool) -> bool {
        if let Some(index) = self.find(name) {
            self.entries[index].is_dir = is_dir;
            return true;
        }
        let Some(index) = self.free_slot() else {
            return false;
        };
        self.entries[index].used = true;
        self.entries[index].is_dir = is_dir;
        self.entries[index].name_len = name.len();
        self.entries[index].name[..name.len()].copy_from_slice(name);
        true
    }

    fn write_raw(&mut self, name: &[u8], text: &[u8]) {
        if !self.touch_raw(name, false) {
            return;
        }
        let index = self.find(name).unwrap_or(0);
        self.entries[index].is_dir = false;
        self.entries[index].data = [0; FILE_DATA_MAX];
        self.entries[index].data_len = text.len();
        self.entries[index].data[..text.len()].copy_from_slice(text);
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        for index in 0..FILE_COUNT {
            let entry = &self.entries[index];
            if entry.used && entry.name_len == name.len() && &entry.name[..entry.name_len] == name {
                return Some(index);
            }
        }
        None
    }

    fn free_slot(&self) -> Option<usize> {
        for index in 0..FILE_COUNT {
            if !self.entries[index].used {
                return Some(index);
            }
        }
        None
    }
}

impl Console {
    fn write_usize(&mut self, mut value: usize) {
        let mut digits = [0u8; 20];
        let mut len = 0usize;

        if value == 0 {
            self.write_byte(b'0');
            return;
        }

        while value > 0 {
            digits[len] = b'0' + (value % 10) as u8;
            value /= 10;
            len += 1;
        }

        while len > 0 {
            len -= 1;
            self.write_byte(digits[len]);
        }
    }

    fn write_i32(&mut self, value: i32) {
        if value < 0 {
            self.write_byte(b'-');
            self.write_usize(value.wrapping_neg() as usize);
        } else {
            self.write_usize(value as usize);
        }
    }

    fn write_hex_u8(&mut self, value: u8) {
        self.write_hex_digit(value >> 4);
        self.write_hex_digit(value & 0x0F);
    }

    fn write_hex_u16(&mut self, value: u16) {
        self.write_hex_u8((value >> 8) as u8);
        self.write_hex_u8(value as u8);
    }

    fn write_hex_u64(&mut self, value: u64) {
        for shift in (0..64).step_by(4).rev() {
            self.write_hex_digit(((value >> shift) & 0x0F) as u8);
        }
    }

    fn write_hex_digit(&mut self, value: u8) {
        let digit = match value & 0x0F {
            0..=9 => b'0' + (value & 0x0F),
            _ => b'A' + ((value & 0x0F) - 10),
        };
        self.write_byte(digit);
    }
}

extern "sysv64" fn abi_write(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }

    unsafe {
        if let Some(console) = APP_CONSOLE.as_mut() {
            console.write_bytes(core::slice::from_raw_parts(ptr, len));
        }
    }
}

extern "sysv64" fn abi_pid() -> u32 {
    current_pid()
}

extern "sysv64" fn abi_args(ptr: *mut u8, len: usize) -> usize {
    let (args_ptr, args_len) = unsafe {
        let table = &raw const PROCESS_TABLE;
        let process = &(*table)[CURRENT_PROCESS_INDEX];
        (process.args.as_ptr(), process.args_len)
    };
    if ptr.is_null() || len == 0 {
        return args_len;
    }

    unsafe {
        let copy_len = min(len, args_len);
        copy_nonoverlapping(args_ptr, ptr, copy_len);
        copy_len
    }
}

extern "sysv64" fn abi_argv_count() -> usize {
    unsafe {
        let process_index = CURRENT_PROCESS_INDEX;
        if process_index >= PROCESS_COUNT {
            0
        } else {
            let table = core::ptr::addr_of!(PROCESS_TABLE);
            1 + (*table)[process_index].argv_count
        }
    }
}

extern "sysv64" fn abi_argv_get(index: usize, ptr: *mut u8, len: usize) -> isize {
    let mut name = [0u8; PROCESS_NAME_MAX];
    let Some(value_len) = copy_argv_value(index, &mut name, ptr, len) else {
        return set_app_error(ERR_NOENT) as isize;
    };
    clear_app_error();
    value_len as isize
}

fn copy_argv_value(
    index: usize,
    process_name: &mut [u8; PROCESS_NAME_MAX],
    ptr: *mut u8,
    len: usize,
) -> Option<usize> {
    let value = if index == 0 {
        let process_index = unsafe { CURRENT_PROCESS_INDEX };
        if process_index >= PROCESS_COUNT {
            return None;
        }
        unsafe {
            let table = core::ptr::addr_of!(PROCESS_TABLE);
            let process = &(*table)[process_index];
            process_name[..process.name_len].copy_from_slice(&process.name[..process.name_len]);
            &process_name[..process.name_len]
        }
    } else {
        let process_index = unsafe { CURRENT_PROCESS_INDEX };
        if process_index >= PROCESS_COUNT {
            return None;
        }
        unsafe {
            let table = core::ptr::addr_of!(PROCESS_TABLE);
            let process = &(*table)[process_index];
            let arg_index = index - 1;
            if arg_index >= process.argv_count {
                return None;
            }
            &process.argv[arg_index][..process.argv_lens[arg_index]]
        }
    };

    if !ptr.is_null() && len > 0 {
        let copy_len = min(len, value.len());
        unsafe {
            copy_nonoverlapping(value.as_ptr(), ptr, copy_len);
        }
    }
    Some(value.len())
}

fn next_arg_token(args: &[u8], mut index: usize) -> Option<(&[u8], usize)> {
    while index < args.len() && is_arg_space(args[index]) {
        index += 1;
    }
    if index >= args.len() {
        return None;
    }
    let start = index;
    while index < args.len() && !is_arg_space(args[index]) {
        index += 1;
    }
    Some((&args[start..index], index))
}

fn is_arg_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r'
}

extern "sysv64" fn abi_read_line(ptr: *mut u8, len: usize) -> usize {
    if ptr.is_null() || len == 0 {
        return 0;
    }

    unsafe {
        let Some(console) = APP_CONSOLE.as_mut() else {
            return 0;
        };

        let mut input_len = 0usize;
        loop {
            if let Some(byte) = read_input_byte() {
                match byte {
                    b'\r' | b'\n' => {
                        console.new_line();
                        return input_len;
                    }
                    8 | 127 => {
                        if input_len > 0 {
                            input_len -= 1;
                            console.backspace();
                        }
                    }
                    byte if byte.is_ascii_graphic() || byte == b' ' => {
                        if input_len + 1 < len {
                            ptr.add(input_len).write(byte);
                            input_len += 1;
                            console.write_byte(byte);
                        }
                    }
                    _ => {}
                }
            } else {
                spin_loop();
            }
        }
    }
}

extern "sysv64" fn abi_file_size(path_ptr: *const u8, path_len: usize) -> isize {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return -1;
    };

    unsafe {
        let bootfs = APP_BOOTFS;
        let Some(data) = bootfs.find_data(path) else {
            return -1;
        };
        data.len() as isize
    }
}

extern "sysv64" fn abi_file_read(
    path_ptr: *const u8,
    path_len: usize,
    buffer_ptr: *mut u8,
    buffer_len: usize,
) -> isize {
    if buffer_ptr.is_null() {
        return -1;
    }
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return -1;
    };

    unsafe {
        let bootfs = APP_BOOTFS;
        let Some(data) = bootfs.find_data(path) else {
            return -1;
        };
        let copy_len = min(buffer_len, data.len());
        copy_nonoverlapping(data.as_ptr(), buffer_ptr, copy_len);
        copy_len as isize
    }
}

extern "sysv64" fn abi_open(path_ptr: *const u8, path_len: usize, flags: u32) -> i32 {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return set_app_error(ERR_INVAL);
    };

    let mut pfs_name = [0u8; PFS_NAME_MAX];
    if let Some(name_len) = pfs_resolve_path(path, &mut pfs_name) {
        return abi_open_pfs(&pfs_name[..name_len], flags);
    }
    if starts_with(path, b"pfs:") {
        return set_app_error(ERR_INVAL);
    }

    if flags & FD_WRITE != 0 {
        return set_app_error(ERR_INVAL);
    }

    unsafe {
        let bootfs = APP_BOOTFS;
        let Some(data) = bootfs.find_data(path) else {
            return set_app_error(ERR_NOENT);
        };
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        for index in 0..APP_FD_COUNT {
            if !table[index].open {
                table[index] = AppFd {
                    open: true,
                    kind: AppFdKind::BootFs,
                    flags: FD_READ,
                    data: data.as_ptr(),
                    len: data.len(),
                    offset: 0,
                    pfs_index: 0,
                };
                clear_app_error();
                return index as i32 + APP_FD_BASE;
            }
        }
    }

    set_app_error(ERR_NOSPC)
}

fn abi_open_pfs(name: &[u8], flags: u32) -> i32 {
    if !valid_pfs_path(name) {
        return set_app_error(ERR_INVAL);
    }
    if flags & FD_CREATE_NEW != 0 && flags & FD_CREATE == 0 {
        return set_app_error(ERR_INVAL);
    }
    if flags & FD_TRUNCATE != 0 && flags & FD_WRITE == 0 {
        return set_app_error(ERR_INVAL);
    }
    if flags & FD_APPEND != 0 && flags & FD_WRITE == 0 {
        return set_app_error(ERR_INVAL);
    }

    let Some(mut header) = PersistentFs::read_header_silent() else {
        return set_app_error(ERR_IO);
    };
    if !pfs_parent_exists(&header, name) {
        return set_app_error(ERR_NOENT);
    }
    let index = match pfs_find_entry(&header, name) {
        Some(index) if pfs_entry_is_dir(&header, index) => return set_app_error(ERR_ISDIR),
        Some(_) if flags & FD_CREATE_NEW != 0 => return set_app_error(ERR_EXIST),
        Some(index) => index,
        None if flags & FD_CREATE != 0 => {
            let Some(index) = pfs_free_entry(&header) else {
                return set_app_error(ERR_NOSPC);
            };
            pfs_set_entry(
                &mut header,
                index,
                name,
                0,
                PFS_KIND_FILE,
                &[],
                ns_since_boot(),
                PFS_MODE_DEFAULT_FILE,
            );
            if !PersistentFs::write_header(&header) {
                return set_app_error(ERR_IO);
            }
            index
        }
        None => return set_app_error(ERR_NOENT),
    };

    if flags & FD_TRUNCATE != 0 {
        let created = pfs_entry_created(&header, index);
        let mode = pfs_entry_mode(&header, index);
        pfs_set_entry(
            &mut header,
            index,
            name,
            0,
            PFS_KIND_FILE,
            &[],
            created,
            mode,
        );
        if !PersistentFs::write_header(&header) {
            return set_app_error(ERR_IO);
        }
    }

    let len = pfs_entry_size(&header, index);
    unsafe {
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        for fd in 0..APP_FD_COUNT {
            if !table[fd].open {
                table[fd] = AppFd {
                    open: true,
                    kind: AppFdKind::Pfs,
                    flags,
                    data: core::ptr::null(),
                    len,
                    offset: if flags & FD_APPEND != 0 { len } else { 0 },
                    pfs_index: index,
                };
                clear_app_error();
                return fd as i32 + APP_FD_BASE;
            }
        }
    }
    set_app_error(ERR_NOSPC)
}

extern "sysv64" fn abi_read(fd: i32, buffer_ptr: *mut u8, buffer_len: usize) -> isize {
    let fd = app_resolve_fd(fd);
    if fd < 0 || buffer_ptr.is_null() {
        return -1;
    }
    if fd == STDIN_FD {
        return abi_read_line(buffer_ptr, buffer_len) as isize;
    }
    let Some(index) = app_fd_index(fd) else {
        return -1;
    };

    unsafe {
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        let handle = &mut table[index];
        if !handle.open {
            return -1;
        }
        if handle.flags & FD_READ == 0 {
            return -1;
        }
        if handle.kind == AppFdKind::PipeRead {
            let pipes = core::ptr::addr_of_mut!(APP_PIPES);
            let pipe = &mut (*pipes)[handle.pfs_index];
            if !pipe.used || !pipe.read_open {
                return -1;
            }
            let available = pipe.len.saturating_sub(pipe.read_offset);
            let pipe_copy = min(buffer_len, available);
            if pipe_copy == 0 {
                return 0;
            }
            copy_nonoverlapping(
                pipe.buffer[pipe.read_offset..].as_ptr(),
                buffer_ptr,
                pipe_copy,
            );
            pipe.read_offset += pipe_copy;
            if pipe.read_offset == pipe.len {
                pipe.read_offset = 0;
                pipe.len = 0;
            }
            return pipe_copy as isize;
        }
        let remaining = handle.len.saturating_sub(handle.offset);
        let copy_len = min(buffer_len, remaining);
        if copy_len == 0 {
            return 0;
        }

        match handle.kind {
            AppFdKind::BootFs => {
                copy_nonoverlapping(handle.data.add(handle.offset), buffer_ptr, copy_len);
            }
            AppFdKind::Pfs => {
                if !pfs_read_at(handle.pfs_index, handle.offset, buffer_ptr, copy_len) {
                    return -1;
                }
            }
            AppFdKind::PipeRead => return -1,
            AppFdKind::PipeWrite => return -1,
            AppFdKind::Empty => return -1,
        }
        handle.offset += copy_len;
        copy_len as isize
    }
}

extern "sysv64" fn abi_write_fd(fd: i32, buffer_ptr: *const u8, buffer_len: usize) -> isize {
    let fd = app_resolve_fd(fd);
    if fd < 0 || buffer_ptr.is_null() {
        return -1;
    }
    if fd == STDOUT_FD || fd == STDERR_FD {
        abi_write(buffer_ptr, buffer_len);
        return buffer_len as isize;
    }
    let Some(index) = app_fd_index(fd) else {
        return -1;
    };

    unsafe {
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        let handle = &mut table[index];
        if !handle.open || handle.flags & FD_WRITE == 0 {
            return -1;
        }
        if handle.kind == AppFdKind::PipeWrite {
            let pipes = core::ptr::addr_of_mut!(APP_PIPES);
            let pipe = &mut (*pipes)[handle.pfs_index];
            if !pipe.used || !pipe.write_open || !pipe.read_open {
                return -1;
            }
            let available = APP_PIPE_BUFFER_SIZE.saturating_sub(pipe.len);
            let write_len = min(buffer_len, available);
            if write_len == 0 && buffer_len != 0 {
                return -1;
            }
            let data = core::slice::from_raw_parts(buffer_ptr, write_len);
            pipe.buffer[pipe.len..pipe.len + write_len].copy_from_slice(data);
            pipe.len += write_len;
            return write_len as isize;
        }
        if handle.kind != AppFdKind::Pfs {
            return -1;
        }
        if handle.flags & FD_APPEND != 0 {
            handle.offset = handle.len;
        }
        if handle.offset + buffer_len > PFS_FILE_MAX {
            return -1;
        }
        let data = core::slice::from_raw_parts(buffer_ptr, buffer_len);
        if !pfs_write_at(handle.pfs_index, handle.offset, data) {
            return -1;
        }
        handle.offset += buffer_len;
        if handle.offset > handle.len {
            handle.len = handle.offset;
            if !pfs_update_size(handle.pfs_index, handle.len) {
                return -1;
            }
        }
        buffer_len as isize
    }
}

extern "sysv64" fn abi_seek(fd: i32, offset: usize) -> isize {
    if fd < 0 {
        return -1;
    }
    let Some(index) = app_fd_index(fd) else {
        return -1;
    };

    unsafe {
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        let handle = &mut table[index];
        if !handle.open {
            return -1;
        }
        // Seeking past the current end is legal (POSIX lseek semantics): it
        // only actually extends the file once something is written there,
        // and pfs_write_at zero-fills the gap in between (see
        // pfs_zero_range) so the hole reads back as zero, not stale data.
        // BootFS handles are read-only ROM data with a fixed length, so
        // still clamp those to their real size.
        if handle.kind == AppFdKind::BootFs && offset > handle.len {
            return -1;
        }
        handle.offset = offset;
        handle.offset as isize
    }
}

extern "sysv64" fn abi_close(fd: i32) -> i32 {
    if fd < 0 {
        return -1;
    }
    if fd == STDIN_FD || fd == STDOUT_FD || fd == STDERR_FD {
        unsafe {
            let std_fds = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].std_fds);
            (*std_fds)[fd as usize] = fd;
        }
        return 0;
    }
    let Some(index) = app_fd_index(fd) else {
        return -1;
    };

    unsafe {
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        if !table[index].open {
            return -1;
        }
        match table[index].kind {
            AppFdKind::PipeRead => {
                let pipes = core::ptr::addr_of_mut!(APP_PIPES);
                let pipe = &mut (*pipes)[table[index].pfs_index];
                pipe.read_open = false;
                if !pipe.write_open {
                    *pipe = AppPipe::empty();
                }
            }
            AppFdKind::PipeWrite => {
                let pipes = core::ptr::addr_of_mut!(APP_PIPES);
                let pipe = &mut (*pipes)[table[index].pfs_index];
                pipe.write_open = false;
                if !pipe.read_open {
                    *pipe = AppPipe::empty();
                }
            }
            _ => {}
        }
        table[index] = AppFd::empty();
    }
    0
}

fn app_fd_index(fd: i32) -> Option<usize> {
    if fd < APP_FD_BASE {
        return None;
    }
    let index = (fd - APP_FD_BASE) as usize;
    if index < APP_FD_COUNT {
        Some(index)
    } else {
        None
    }
}

extern "sysv64" fn abi_stat(path_ptr: *const u8, path_len: usize, stat_ptr: *mut RymosStat) -> i32 {
    if stat_ptr.is_null() {
        return set_app_error(ERR_INVAL);
    }
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return set_app_error(ERR_INVAL);
    };

    let Some(stat) = stat_path(path) else {
        return set_app_error(ERR_NOENT);
    };
    unsafe {
        stat_ptr.write(stat);
    }
    clear_app_error();
    0
}

extern "sysv64" fn abi_list(
    namespace_ptr: *const u8,
    namespace_len: usize,
    index: usize,
    name_ptr: *mut u8,
    name_len: usize,
    stat_ptr: *mut RymosStat,
) -> isize {
    if name_ptr.is_null() || name_len == 0 || stat_ptr.is_null() {
        return set_app_error(ERR_INVAL) as isize;
    }
    let namespace = if namespace_ptr.is_null() || namespace_len == 0 {
        b"".as_slice()
    } else {
        let Some(namespace) = checked_app_slice(namespace_ptr, namespace_len) else {
            return set_app_error(ERR_INVAL) as isize;
        };
        namespace
    };

    let mut resolved = [0u8; PFS_NAME_MAX];
    let pfs_namespace: Option<&[u8]> = if namespace.is_empty() && app_cwd_is_pfs() {
        let mut cwd_name = [0u8; PFS_NAME_MAX];
        let cwd = app_cwd_pfs_name(&mut cwd_name);
        let len = cwd.len();
        resolved[..len].copy_from_slice(cwd);
        Some(&resolved[..len])
    } else if eq(namespace, b"pfs:") || eq(namespace, b"pfs:/") {
        Some(b"")
    } else if let Some(len) = pfs_resolve_path(namespace, &mut resolved) {
        Some(&resolved[..len])
    } else {
        None
    };

    if let Some(namespace) = pfs_namespace {
        let Some(header) = PersistentFs::read_header_silent() else {
            return set_app_error(ERR_IO) as isize;
        };
        let mut seen = 0usize;
        for entry_index in 0..PFS_ENTRY_COUNT {
            if pfs_entry_used(&header, entry_index) {
                let full_name = pfs_entry_name(&header, entry_index);
                let Some(name) = pfs_list_match(namespace, full_name) else {
                    continue;
                };
                if seen == index {
                    let copy_len = min(name_len, name.len());
                    unsafe {
                        copy_nonoverlapping(name.as_ptr(), name_ptr, copy_len);
                        stat_ptr.write(RymosStat {
                            kind: if pfs_entry_is_dir(&header, entry_index) {
                                STAT_KIND_DIR
                            } else {
                                STAT_KIND_FILE
                            },
                            fs: STAT_FS_PFS,
                            size: pfs_entry_size(&header, entry_index),
                            created_ticks: pfs_entry_created(&header, entry_index),
                            modified_ticks: pfs_entry_modified(&header, entry_index),
                            mode: pfs_entry_mode(&header, entry_index) as u32,
                        });
                    }
                    clear_app_error();
                    return copy_len as isize;
                }
                seen += 1;
            }
        }
        return set_app_error(ERR_NOENT) as isize;
    }

    let Some((name, stat)) = list_bootfs(index) else {
        return set_app_error(ERR_NOENT) as isize;
    };
    let copy_len = min(name_len, name.len());
    unsafe {
        copy_nonoverlapping(name.as_ptr(), name_ptr, copy_len);
        stat_ptr.write(stat);
    }
    clear_app_error();
    copy_len as isize
}

extern "sysv64" fn abi_mkdir(path_ptr: *const u8, path_len: usize) -> i32 {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return set_app_error(ERR_INVAL);
    };
    let mut resolved = [0u8; PFS_NAME_MAX];
    let Some(name_len) = pfs_resolve_path(path, &mut resolved) else {
        return set_app_error(ERR_INVAL);
    };
    let name = &resolved[..name_len];
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return set_app_error(ERR_IO);
    };
    if !pfs_parent_exists(&header, name) {
        return set_app_error(ERR_NOENT);
    }
    if pfs_find_entry(&header, name).is_some() {
        return set_app_error(ERR_EXIST);
    }
    let Some(index) = pfs_free_entry(&header) else {
        return set_app_error(ERR_NOSPC);
    };
    pfs_set_entry(
        &mut header,
        index,
        name,
        0,
        PFS_KIND_DIR,
        &[],
        ns_since_boot(),
        PFS_MODE_DEFAULT_DIR,
    );
    if PersistentFs::write_header(&header) {
        clear_app_error();
        0
    } else {
        set_app_error(ERR_IO)
    }
}

extern "sysv64" fn abi_env_get(
    key_ptr: *const u8,
    key_len: usize,
    value_ptr: *mut u8,
    value_len: usize,
) -> isize {
    let Some(key) = checked_app_slice(key_ptr, key_len) else {
        return set_app_error(ERR_INVAL) as isize;
    };
    let Some(value) = env_lookup(key) else {
        return set_app_error(ERR_NOENT) as isize;
    };
    if value_ptr.is_null() || value_len == 0 {
        clear_app_error();
        return value.len() as isize;
    }
    let copy_len = min(value_len, value.len());
    unsafe {
        copy_nonoverlapping(value.as_ptr(), value_ptr, copy_len);
    }
    clear_app_error();
    copy_len as isize
}

extern "sysv64" fn abi_env_list(
    index: usize,
    key_ptr: *mut u8,
    key_len: usize,
    value_ptr: *mut u8,
    value_len: usize,
) -> isize {
    if key_ptr.is_null() || value_ptr.is_null() {
        return set_app_error(ERR_INVAL) as isize;
    }
    let Some((key, value)) = env_list_entry(index) else {
        return set_app_error(ERR_NOENT) as isize;
    };
    let key_copy = min(key_len, key.len());
    let value_copy = min(value_len, value.len());
    unsafe {
        copy_nonoverlapping(key.as_ptr(), key_ptr, key_copy);
        copy_nonoverlapping(value.as_ptr(), value_ptr, value_copy);
    }
    clear_app_error();
    ((key_copy as isize) << 32) | value_copy as isize
}

extern "sysv64" fn abi_env_set(
    key_ptr: *const u8,
    key_len: usize,
    value_ptr: *const u8,
    value_len: usize,
) -> i32 {
    let Some(key) = checked_app_slice(key_ptr, key_len) else {
        return set_app_error(ERR_INVAL);
    };
    let Some(value) = checked_app_slice(value_ptr, value_len) else {
        return set_app_error(ERR_INVAL);
    };
    env_set_overlay(key, value, false)
}

extern "sysv64" fn abi_env_remove(key_ptr: *const u8, key_len: usize) -> i32 {
    let Some(key) = checked_app_slice(key_ptr, key_len) else {
        return set_app_error(ERR_INVAL);
    };
    env_set_overlay(key, b"", true)
}

fn env_lookup<'a>(key: &[u8]) -> Option<&'a [u8]> {
    unsafe {
        let app_env = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].env);
        for entry in &*app_env {
            if entry.used && &entry.key[..entry.key_len] == key {
                if entry.deleted {
                    return None;
                }
                return Some(&entry.value[..entry.value_len]);
            }
        }
    }
    for (env_key, env_value) in ENV {
        if env_key == key {
            return Some(env_value);
        }
    }
    None
}

fn env_list_entry<'a>(wanted: usize) -> Option<(&'a [u8], &'a [u8])> {
    let mut seen = 0usize;
    unsafe {
        let app_env = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].env);
        for entry in &*app_env {
            if entry.used && !entry.deleted {
                if seen == wanted {
                    return Some((&entry.key[..entry.key_len], &entry.value[..entry.value_len]));
                }
                seen += 1;
            }
        }
    }
    for (key, value) in ENV {
        if env_overlay_has_key(key) {
            continue;
        }
        if seen == wanted {
            return Some((key, value));
        }
        seen += 1;
    }
    None
}

fn env_overlay_has_key(key: &[u8]) -> bool {
    unsafe {
        let app_env = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].env);
        for entry in &*app_env {
            if entry.used && &entry.key[..entry.key_len] == key {
                return true;
            }
        }
    }
    false
}

fn env_set_overlay(key: &[u8], value: &[u8], deleted: bool) -> i32 {
    if key.is_empty()
        || key.len() > APP_ENV_KEY_MAX
        || value.len() > APP_ENV_VALUE_MAX
        || key.iter().any(|byte| *byte == b'=' || *byte == 0)
    {
        return set_app_error(ERR_INVAL);
    }

    unsafe {
        let app_env = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].env);
        let mut slot = APP_ENV_COUNT;
        for index in 0..APP_ENV_COUNT {
            if (*app_env)[index].used
                && &(&(*app_env)[index].key)[..(*app_env)[index].key_len] == key
            {
                slot = index;
                break;
            }
            if slot == APP_ENV_COUNT && !(*app_env)[index].used {
                slot = index;
            }
        }
        if slot == APP_ENV_COUNT {
            return set_app_error(ERR_NOSPC);
        }

        let entry = &mut (*app_env)[slot];
        *entry = AppEnvVar::empty();
        entry.used = true;
        entry.deleted = deleted;
        entry.key_len = key.len();
        entry.key[..key.len()].copy_from_slice(key);
        entry.value_len = value.len();
        entry.value[..value.len()].copy_from_slice(value);
    }
    clear_app_error();
    0
}

extern "sysv64" fn abi_spawn(
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const u8,
    args_len: usize,
) -> i32 {
    let Some(name) = checked_app_slice(name_ptr, name_len) else {
        return set_app_error(ERR_INVAL);
    };
    if name.is_empty() || name.len() > PROCESS_NAME_MAX {
        return set_app_error(ERR_INVAL);
    }
    let args = if args_len == 0 {
        b"".as_slice()
    } else {
        let Some(args) = checked_app_slice(args_ptr, args_len) else {
            return set_app_error(ERR_INVAL);
        };
        args
    };
    if args.len() > PROCESS_ARGS_MAX {
        return set_app_error(ERR_INVAL);
    }

    spawn_prepared(name, args, None)
}

extern "sysv64" fn abi_spawn_argv(
    name_ptr: *const u8,
    name_len: usize,
    argv_ptr: *const ArgSlice,
    argv_len: usize,
) -> i32 {
    let Some(name) = checked_app_slice(name_ptr, name_len) else {
        return set_app_error(ERR_INVAL);
    };
    if name.is_empty() || name.len() > PROCESS_NAME_MAX || argv_len > PROCESS_ARGV_COUNT_MAX {
        return set_app_error(ERR_INVAL);
    }
    if argv_len > 0 && argv_ptr.is_null() {
        return set_app_error(ERR_INVAL);
    }

    let mut arg_storage = [[0u8; PROCESS_ARGV_VALUE_MAX]; PROCESS_ARGV_COUNT_MAX];
    let mut argv = [ArgSlice::empty(); PROCESS_ARGV_COUNT_MAX];
    let mut raw_args = [0u8; PROCESS_ARGS_MAX];
    let mut raw_len = 0usize;

    for index in 0..argv_len {
        let arg = unsafe { *argv_ptr.add(index) };
        if arg.ptr.is_null() || arg.len > PROCESS_ARGV_VALUE_MAX {
            return set_app_error(ERR_INVAL);
        }
        let Some(arg_bytes) = checked_app_slice(arg.ptr, arg.len) else {
            return set_app_error(ERR_INVAL);
        };
        arg_storage[index][..arg.len].copy_from_slice(arg_bytes);
        argv[index] = ArgSlice {
            ptr: arg_storage[index].as_ptr(),
            len: arg.len,
        };
        let separator = usize::from(raw_len > 0);
        if raw_len + separator + arg.len > PROCESS_ARGS_MAX {
            return set_app_error(ERR_NOSPC);
        }
        if separator == 1 {
            raw_args[raw_len] = b' ';
            raw_len += 1;
        }
        raw_args[raw_len..raw_len + arg.len].copy_from_slice(arg_bytes);
        raw_len += arg.len;
    }

    spawn_prepared(name, &raw_args[..raw_len], Some(&argv[..argv_len]))
}

/// Hand-builds a brand-new process's initial stack frame so that
/// `context_switch`'s `pop`+`ret` sequence lands at `entry_point` -- see
/// `context_switch`'s docs for the exact register layout expected. The `-
/// 64` (not `-56`, the literal size of 6 zeroed registers plus one return
/// address) keeps SysV's 16-byte alignment intact from a page-aligned
/// (hence already 16-aligned) `stack_top`: 64 is itself a multiple of 16,
/// so the arithmetic can't drift it, unlike 56 -- verified against the
/// context-switch smoke test earlier in this file. Returns the initial
/// `rsp` to store in the new process's `CpuContext`.
fn build_initial_stack_frame(stack_top: u64, entry_point: u64) -> u64 {
    let initial_rsp = stack_top - 64;
    unsafe {
        let slot = initial_rsp as *mut u64;
        for i in 0..6u64 {
            slot.add(i as usize).write(0);
        }
        slot.add(6).write(entry_point);
    }
    initial_rsp
}

/// Builds everything a freshly `process_spawn`-registered process needs to
/// actually run later -- an isolated address space, its ELF loaded into it,
/// its own kernel stack, and an initial `CpuContext` pointing at
/// `process_trampoline` -- and marks it `Ready`. Nothing runs yet: the
/// scheduler (`pick_next_ready`, driven from `abi_wait`/`abi_wait_any`/
/// `run_program`) decides when this process actually gets switched into,
/// which is the real behavior change from the old synchronous model (see
/// `spawn_prepared`'s docs for why that model couldn't just be extended
/// bit by bit). On failure, marks the process `Failed` and returns `false`.
fn prepare_process(index: usize, console: &mut Console, image: &[u8]) -> bool {
    let pid = process_pid(index);
    let Some(parent_pml4) = current_pml4_or_kernel() else {
        process_set_state(index, ProcessState::Failed, -1);
        return false;
    };
    let Some(child_pml4) = create_process_address_space(pid) else {
        process_set_state(index, ProcessState::Failed, -1);
        return false;
    };

    // Loading the ELF writes segment data through the child's own virtual
    // image-window addresses, so CR3 has to point at its private PML4 for
    // the duration of the load -- restored immediately after, since this
    // process isn't actually running yet (see `switch_to`, which is what
    // sets CR3 for real once the scheduler picks this process).
    unsafe {
        write_cr3(child_pml4);
    }
    let entry = load_program_elf_isolated(console, image, child_pml4);
    unsafe {
        write_cr3(parent_pml4);
    }
    let Some(entry) = entry else {
        destroy_process_address_space(child_pml4);
        process_set_state(index, ProcessState::Failed, -1);
        return false;
    };

    let Some(stack_top) = create_process_stack(pid) else {
        destroy_process_address_space(child_pml4);
        process_set_state(index, ProcessState::Failed, -1);
        return false;
    };
    let initial_rsp =
        build_initial_stack_frame(stack_top, process_trampoline as *const () as u64);

    app_set_heap_window(index, pid);
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[index];
        process.pml4_phys = child_pml4;
        process.entry_point = entry;
        process.context.rsp = initial_rsp;
    }
    process_set_state(index, ProcessState::Ready, 0);
    true
}

/// Registers a child in the process table, prepares it to run (address
/// space, ELF, stack, initial context), and returns its pid immediately --
/// spawn no longer runs the child itself; see `prepare_process`'s docs.
///
/// A genuinely deferred version of this was tried once before, much
/// earlier, and reverted: it silently broke every existing
/// `spawn`-then-read-a-pipe caller that had no intervening `wait()` at all
/// -- several of rysh's own shell built-ins (`spawnredir`, `spawnstdin`,
/// `spawnio`, `spawnioe`) read a redirected pipe's output immediately after
/// `spawn()` purely because `spawn()` used to block until the child
/// finished; deferring it left the pipe empty (or, worse, closed before
/// the child ever ran). That attempt was reverted rather than fixing every
/// affected caller as its own large audit. This time the fix *is* done:
/// those rysh built-ins and `rymos-user`'s `Command` helpers now actually
/// `wait()` before reverting stdio redirection (see their own docs), and
/// `abi_wait`/`abi_wait_any` are real blocking calls now (see their docs)
/// that drive the scheduler until the right child has actually run.
fn spawn_prepared(name: &[u8], args: &[u8], argv: Option<&[ArgSlice]>) -> i32 {
    let bootfs = unsafe { APP_BOOTFS };

    let parent_process_index = unsafe { CURRENT_PROCESS_INDEX };
    if parent_process_index >= PROCESS_COUNT {
        return set_app_error(ERR_INVAL);
    }

    let mut child_path = [0u8; 32];
    let child_path_len = program_path(name, &mut child_path);
    let Some(child_image) = bootfs.find_data(&child_path[..child_path_len]) else {
        return set_app_error(ERR_NOENT);
    };

    let child_process_index = if let Some(argv) = argv {
        process_spawn_with_argv(name, args, argv, current_pid())
    } else {
        process_spawn(name, args)
    };
    let Some(child_process_index) = child_process_index else {
        return set_app_error(ERR_NOSPC);
    };

    // Copy the caller's *current* std-fd/cwd/env redirection state into the
    // child's own fields right now, not when the child actually runs -- see
    // `Process`'s docs for why this has to happen here rather than reading
    // the parent's live state later (a `Command`-style helper redirects,
    // spawns, then reverts the redirection before ever waiting).
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        let parent_std_fds = (*table)[parent_process_index].std_fds;
        let parent_cwd = (*table)[parent_process_index].cwd;
        let parent_cwd_len = (*table)[parent_process_index].cwd_len;
        let parent_env = (*table)[parent_process_index].env;
        let process = &mut (*table)[child_process_index];
        process.std_fds = parent_std_fds;
        process.cwd = parent_cwd;
        process.cwd_len = parent_cwd_len;
        process.env = parent_env;
    }

    let Some(console) = (unsafe { APP_CONSOLE.as_mut() }) else {
        process_set_state(child_process_index, ProcessState::Failed, -1);
        return set_app_error(ERR_INVAL);
    };
    if !prepare_process(child_process_index, console, child_image) {
        return set_app_error(ERR_IO);
    }

    clear_app_error();
    process_pid(child_process_index) as i32
}

/// Marks the currently-running process (a no-op if we're actually the
/// reserved idle/console context, which has no `state` that matters)
/// `Blocked` on `target`, so `pick_next_ready` skips it until
/// `wake_waiters_for` moves it back to `Ready`.
fn block_current_on(target: WaitTarget) {
    unsafe {
        let index = CURRENT_PROCESS_INDEX;
        if index < PROCESS_COUNT {
            let table = &raw mut PROCESS_TABLE;
            let process = &mut (*table)[index];
            process.waiting_for = target;
            process.state = ProcessState::Blocked;
        }
    }
}

/// Gives the CPU to whatever should run next -- a `Ready` process if one
/// exists, else falls back to the reserved idle/console slot. The one
/// building block `wait`/`wait_any`'s blocking loops share between
/// "recheck my condition" attempts.
fn run_scheduler_step() {
    let next = pick_next_ready().unwrap_or(PROCESS_COUNT);
    switch_to(next);
}

/// Is there anything still alive (not yet `Exited`/`Failed`) that's a
/// child of `parent_pid`? Used to decide whether continuing to block in
/// `wait_any` could ever pay off, or whether there's nothing left that
/// will ever satisfy it.
fn has_live_child(parent_pid: u32) -> bool {
    unsafe {
        let table = &raw const PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            let process = &(*table)[index];
            if process.parent_pid == parent_pid
                && matches!(
                    process.state,
                    ProcessState::Ready | ProcessState::Running | ProcessState::Blocked
                )
            {
                return true;
            }
        }
    }
    false
}

/// Blocks (via the scheduler -- see `block_current_on`/`run_scheduler_step`)
/// until `pid` has exited, then returns its status. This is the real
/// blocking wait category 2's scheduler work was for: if `pid` is still
/// `Ready` (spawned but never actually run yet, under the new deferred
/// `spawn_prepared`), the very first iteration's `run_scheduler_step` is
/// what actually runs it for the first time. Shared core for both
/// `abi_wait` and `run_program` (the top-level console `run` command,
/// which isn't going through the ABI's pointer-writing convention but
/// still needs to block the same way).
fn wait_for_pid_blocking(pid: u32) -> Option<RymosProcessStatus> {
    loop {
        if let Some(status) = process_wait_by_pid(pid) {
            return Some(status);
        }
        if find_process_index_by_pid(pid).is_none() {
            // No such pid at all (never existed, or already reaped) --
            // nothing will ever satisfy this wait.
            return None;
        }
        block_current_on(WaitTarget::Pid(pid));
        run_scheduler_step();
    }
}

/// Same as `wait_for_pid_blocking`, but for any unwaited child of
/// `parent_pid` rather than one specific pid -- the shared core for
/// `abi_wait_any`.
fn wait_for_any_child_blocking(parent_pid: u32) -> Option<(u32, RymosProcessStatus)> {
    loop {
        if let Some(result) = process_wait_any_child(parent_pid) {
            return Some(result);
        }
        if !has_live_child(parent_pid) {
            return None;
        }
        block_current_on(WaitTarget::AnyChild);
        run_scheduler_step();
    }
}

extern "sysv64" fn abi_wait(pid: u32, status_ptr: *mut RymosProcessStatus) -> i32 {
    if status_ptr.is_null() {
        return -1;
    }
    match wait_for_pid_blocking(pid) {
        Some(status) => {
            unsafe {
                status_ptr.write(status);
            }
            0
        }
        None => -1,
    }
}

extern "sysv64" fn abi_wait_any(status_ptr: *mut RymosProcessStatus) -> i32 {
    if status_ptr.is_null() {
        return -1;
    }
    let parent_pid = current_pid();
    match wait_for_any_child_blocking(parent_pid) {
        Some((pid, status)) => {
            unsafe {
                status_ptr.write(status);
            }
            pid as i32
        }
        None => -1,
    }
}

extern "sysv64" fn abi_mem_alloc_pages(page_count: usize) -> u64 {
    if page_count == 0 || page_count > USER_HEAP_MAX_PAGES_PER_CALL {
        return 0;
    }
    let Some(pml4_phys) = ensure_kernel_pml4() else {
        return 0;
    };
    let process_index = unsafe { CURRENT_PROCESS_INDEX };

    let base = unsafe {
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[process_index];
        if process.heap_base == 0 || process.heap_next == 0 || process.heap_limit == 0 {
            return 0;
        }
        let base = process.heap_next;
        let Some(bytes) = (page_count as u64).checked_mul(PAGE_SIZE) else {
            return 0;
        };
        let Some(next) = process.heap_next.checked_add(bytes) else {
            return 0;
        };
        if next > process.heap_limit {
            return 0;
        }
        process.heap_next = next;
        base
    };

    if !process_track_mapping(process_index, base, page_count) {
        return 0;
    }
    if !map_user_pages(pml4_phys, base, page_count) {
        let _ = process_untrack_mapping(process_index, base, page_count);
        return 0;
    }
    base
}

extern "sysv64" fn abi_mem_map_pages(page_count: usize, flags: u32) -> u64 {
    if page_count == 0 || page_count > USER_MMAP_MAX_PAGES_PER_CALL {
        return 0;
    }
    if flags & !MEM_MAP_GUARD != 0 {
        return 0;
    }
    let Some(pml4_phys) = ensure_kernel_pml4() else {
        return 0;
    };
    let process_index = unsafe { CURRENT_PROCESS_INDEX };
    let guard_pages = if flags & MEM_MAP_GUARD != 0 { 2 } else { 0 };
    let Some(total_pages) = page_count.checked_add(guard_pages) else {
        return 0;
    };

    let mapped_base = unsafe {
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[process_index];
        if process.mmap_next == 0 || process.mmap_limit == 0 {
            return 0;
        }
        let Some(bytes) = (total_pages as u64).checked_mul(PAGE_SIZE) else {
            return 0;
        };
        let reservation_base = process.mmap_next;
        let Some(next) = process.mmap_next.checked_add(bytes) else {
            return 0;
        };
        if next > process.mmap_limit {
            return 0;
        }
        process.mmap_next = next;
        if flags & MEM_MAP_GUARD != 0 {
            reservation_base + PAGE_SIZE
        } else {
            reservation_base
        }
    };

    if !process_track_mapping(process_index, mapped_base, page_count) {
        return 0;
    }
    if !map_user_pages(pml4_phys, mapped_base, page_count) {
        let _ = process_untrack_mapping(process_index, mapped_base, page_count);
        return 0;
    }
    mapped_base
}

extern "sysv64" fn abi_mem_unmap_pages(address: u64, page_count: usize) -> i32 {
    if address == 0
        || page_count == 0
        || page_count > USER_MMAP_MAX_PAGES_PER_CALL
        || address & (PAGE_SIZE - 1) != 0
    {
        return -1;
    }
    let process_index = unsafe { CURRENT_PROCESS_INDEX };
    if !process_untrack_mapping(process_index, address, page_count) {
        return -1;
    }
    let _ = unmap_user_pages(unsafe { KERNEL_PML4_PHYS }, address, page_count);
    0
}

extern "sysv64" fn abi_time_ticks() -> u64 {
    ns_since_boot()
}

/// Real wall-clock time as nanoseconds since the Unix epoch, or 0 if the
/// CMOS RTC couldn't be read at boot (see `read_rtc_unix_seconds`).
extern "sysv64" fn abi_time_unix_nanos() -> u64 {
    unix_nanos_now()
}

/// Busy-waits for at least `nanos` nanoseconds. RYMOS has no timer
/// interrupt or scheduler (see the process-model docs), so there is nothing
/// else that could usefully run during a sleep anyway -- spinning against
/// the calibrated TSC is the correct implementation here, not a placeholder
/// for a real one.
extern "sysv64" fn abi_sleep_nanos(nanos: u64) {
    let start = ns_since_boot();
    while ns_since_boot().saturating_sub(start) < nanos {
        spin_loop();
    }
}

/// Reports the console's current text grid size. Real hardware/OVMF's GOP
/// framebuffer mode is fixed for the whole boot (`1024x768`, see
/// `Console::new`), so this never changes after start, but it's still real
/// -- not a hardcoded guess -- for programs that want to lay out output
/// without assuming 80x25.
extern "sysv64" fn abi_term_size(rows_ptr: *mut usize, cols_ptr: *mut usize) -> i32 {
    if rows_ptr.is_null() || cols_ptr.is_null() {
        return set_app_error(ERR_INVAL);
    }
    let Some(console) = (unsafe { APP_CONSOLE.as_ref() }) else {
        return set_app_error(ERR_INVAL);
    };
    unsafe {
        rows_ptr.write(console.rows);
        cols_ptr.write(console.cols);
    }
    clear_app_error();
    0
}

extern "sysv64" fn abi_unlink(path_ptr: *const u8, path_len: usize) -> i32 {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return set_app_error(ERR_INVAL);
    };
    let mut resolved = [0u8; PFS_NAME_MAX];
    let Some(name_len) = pfs_resolve_path(path, &mut resolved) else {
        return set_app_error(ERR_INVAL);
    };
    let name = &resolved[..name_len];
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return set_app_error(ERR_IO);
    };
    if let Err(code) = pfs_unlink_header(&mut header, name) {
        return set_app_error(if code == -2 { ERR_INVAL } else { ERR_NOENT });
    }
    if PersistentFs::write_header(&header) {
        clear_app_error();
        0
    } else {
        set_app_error(ERR_IO)
    }
}

extern "sysv64" fn abi_rename(
    old_ptr: *const u8,
    old_len: usize,
    new_ptr: *const u8,
    new_len: usize,
) -> i32 {
    let Some(old_path) = checked_app_slice(old_ptr, old_len) else {
        return set_app_error(ERR_INVAL);
    };
    let Some(new_path) = checked_app_slice(new_ptr, new_len) else {
        return set_app_error(ERR_INVAL);
    };
    let mut old_resolved = [0u8; PFS_NAME_MAX];
    let mut new_resolved = [0u8; PFS_NAME_MAX];
    let Some(old_len) = pfs_resolve_path(old_path, &mut old_resolved) else {
        return set_app_error(ERR_INVAL);
    };
    let Some(new_len) = pfs_resolve_path(new_path, &mut new_resolved) else {
        return set_app_error(ERR_INVAL);
    };
    let old_name = &old_resolved[..old_len];
    let new_name = &new_resolved[..new_len];
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return set_app_error(ERR_IO);
    };
    if let Err(code) = pfs_rename_header(&mut header, old_name, new_name) {
        let error = match code {
            -2 => ERR_NOENT,
            -3 => ERR_EXIST,
            -4 => ERR_NOENT,
            -5 => ERR_INVAL,
            _ => ERR_INVAL,
        };
        return set_app_error(error);
    }
    if PersistentFs::write_header(&header) {
        clear_app_error();
        0
    } else {
        set_app_error(ERR_IO)
    }
}

extern "sysv64" fn abi_cwd(buffer_ptr: *mut u8, buffer_len: usize) -> isize {
    let len = unsafe { PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len };
    if buffer_ptr.is_null() || buffer_len == 0 {
        return len as isize;
    }
    let copy_len = min(buffer_len, len);
    unsafe {
        let cwd = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd);
        copy_nonoverlapping((*cwd).as_ptr(), buffer_ptr, copy_len);
    }
    clear_app_error();
    copy_len as isize
}

extern "sysv64" fn abi_chdir(path_ptr: *const u8, path_len: usize) -> i32 {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return set_app_error(ERR_INVAL);
    };
    if eq(path, b"/") {
        unsafe {
            let cwd = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd);
            (*cwd).fill(0);
            (*cwd)[0] = b'/';
            PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len = 1;
        }
        clear_app_error();
        return 0;
    }
    if eq(path, b"pfs:") || eq(path, b"pfs:/") {
        unsafe {
            let cwd = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd);
            (*cwd).fill(0);
            (&mut (*cwd))[..4].copy_from_slice(b"pfs:");
            PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len = 4;
        }
        clear_app_error();
        return 0;
    }

    let mut resolved = [0u8; PFS_NAME_MAX];
    let Some(name_len) = pfs_resolve_path(path, &mut resolved) else {
        return set_app_error(ERR_INVAL);
    };
    let name = &resolved[..name_len];
    let Some(header) = PersistentFs::read_header_silent() else {
        return set_app_error(ERR_IO);
    };
    let Some(index) = pfs_find_entry(&header, name) else {
        return set_app_error(ERR_NOENT);
    };
    if !pfs_entry_is_dir(&header, index) {
        return set_app_error(ERR_NOTDIR);
    }
    unsafe {
        let cwd = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd);
        (*cwd).fill(0);
        (&mut (*cwd))[..4].copy_from_slice(b"pfs:");
        (&mut (*cwd))[4..4 + name_len].copy_from_slice(name);
        PROCESS_TABLE[CURRENT_PROCESS_INDEX].cwd_len = 4 + name_len;
    }
    clear_app_error();
    0
}

extern "sysv64" fn abi_last_error() -> i32 {
    unsafe { PROCESS_TABLE[CURRENT_PROCESS_INDEX].last_error }
}

extern "sysv64" fn abi_pipe(read_fd_ptr: *mut i32, write_fd_ptr: *mut i32) -> i32 {
    if read_fd_ptr.is_null() || write_fd_ptr.is_null() {
        return set_app_error(ERR_INVAL);
    }

    unsafe {
        let pipes = core::ptr::addr_of_mut!(APP_PIPES);
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let pipe_index = match (0..APP_PIPE_COUNT).find(|index| !(*pipes)[*index].used) {
            Some(index) => index,
            None => return set_app_error(ERR_NOSPC),
        };

        let mut read_slot = APP_FD_COUNT;
        let mut write_slot = APP_FD_COUNT;
        for index in 0..APP_FD_COUNT {
            if !(*table)[index].open {
                if read_slot == APP_FD_COUNT {
                    read_slot = index;
                } else {
                    write_slot = index;
                    break;
                }
            }
        }
        if write_slot == APP_FD_COUNT {
            return set_app_error(ERR_NOSPC);
        }

        (*pipes)[pipe_index] = AppPipe {
            used: true,
            read_open: true,
            write_open: true,
            buffer: [0; APP_PIPE_BUFFER_SIZE],
            read_offset: 0,
            len: 0,
        };
        (*table)[read_slot] = AppFd {
            open: true,
            kind: AppFdKind::PipeRead,
            flags: FD_READ,
            data: core::ptr::null(),
            len: 0,
            offset: 0,
            pfs_index: pipe_index,
        };
        (*table)[write_slot] = AppFd {
            open: true,
            kind: AppFdKind::PipeWrite,
            flags: FD_WRITE,
            data: core::ptr::null(),
            len: 0,
            offset: 0,
            pfs_index: pipe_index,
        };
        read_fd_ptr.write(read_slot as i32 + APP_FD_BASE);
        write_fd_ptr.write(write_slot as i32 + APP_FD_BASE);
    }
    clear_app_error();
    0
}

extern "sysv64" fn abi_dup2(old_fd: i32, new_fd: i32) -> i32 {
    if !(STDIN_FD..=STDERR_FD).contains(&new_fd) {
        return set_app_error(ERR_INVAL);
    }
    if old_fd == new_fd {
        unsafe {
            let std_fds = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].std_fds);
            (*std_fds)[new_fd as usize] = new_fd;
        }
        clear_app_error();
        return new_fd;
    }
    if !app_fd_open(old_fd) {
        return set_app_error(ERR_INVAL);
    }
    unsafe {
        let std_fds = core::ptr::addr_of_mut!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].std_fds);
        (*std_fds)[new_fd as usize] = old_fd;
    }
    clear_app_error();
    new_fd
}

/// Returns what `which` (`STDIN_FD`/`STDOUT_FD`/`STDERR_FD`) currently
/// resolves to -- the real console (the fd number itself, e.g. `1` for
/// `STDOUT_FD`) or a redirected fd (e.g. a pipe's write end), whichever
/// `dup2` last pointed it at. Lets a caller save the *current* redirection
/// before doing its own temporary one (e.g. `Command::output()` redirecting
/// stdout onto a capturing pipe) and restore exactly that afterward via
/// `dup2(saved, which)`, instead of unconditionally resetting to the real
/// console -- which was a real, confirmed bug for any caller whose own
/// stdout wasn't the console to begin with (a nested `Command` call, e.g.
/// `relay` spawning `relay` spawning `hello`, undoing its *own* redirect by
/// blindly pointing stdout back at the console instead of its caller's
/// capturing pipe -- see `docs/self-hosting.md`'s Recently Closed).
extern "sysv64" fn abi_std_fd(which: i32) -> i32 {
    if !(STDIN_FD..=STDERR_FD).contains(&which) {
        return -1;
    }
    unsafe {
        let std_fds = core::ptr::addr_of!(PROCESS_TABLE[CURRENT_PROCESS_INDEX].std_fds);
        (*std_fds)[which as usize]
    }
}

fn stat_path(path: &[u8]) -> Option<RymosStat> {
    let mut resolved = [0u8; PFS_NAME_MAX];
    if let Some(name_len) = pfs_resolve_path(path, &mut resolved) {
        let name = &resolved[..name_len];
        let header = PersistentFs::read_header_silent()?;
        let index = pfs_find_entry(&header, name)?;
        return Some(RymosStat {
            kind: if pfs_entry_is_dir(&header, index) {
                STAT_KIND_DIR
            } else {
                STAT_KIND_FILE
            },
            fs: STAT_FS_PFS,
            size: pfs_entry_size(&header, index),
            created_ticks: pfs_entry_created(&header, index),
            modified_ticks: pfs_entry_modified(&header, index),
            mode: pfs_entry_mode(&header, index) as u32,
        });
    }

    unsafe {
        let bootfs = APP_BOOTFS;
        let entry = bootfs.find_entry(path)?;
        Some(RymosStat {
            kind: if entry.kind == 2 {
                STAT_KIND_DIR
            } else {
                STAT_KIND_FILE
            },
            fs: STAT_FS_BOOTFS,
            size: entry.data_len,
            created_ticks: 0,
            modified_ticks: 0,
            mode: BOOTFS_MODE,
        })
    }
}

fn list_bootfs(wanted: usize) -> Option<(&'static [u8], RymosStat)> {
    unsafe {
        let bootfs = APP_BOOTFS;
        let entry = bootfs.entry(wanted)?;
        let name = core::slice::from_raw_parts(entry.name, entry.name_len);
        Some((
            name,
            RymosStat {
                kind: if entry.kind == 2 {
                    STAT_KIND_DIR
                } else {
                    STAT_KIND_FILE
                },
                fs: STAT_FS_BOOTFS,
                size: entry.data_len,
                created_ticks: 0,
                modified_ticks: 0,
                mode: BOOTFS_MODE,
            },
        ))
    }
}

fn app_close_all_fds() {
    unsafe {
        let table = core::ptr::addr_of_mut!(APP_FDS);
        let table = &mut *table;
        for handle in table.iter_mut() {
            *handle = AppFd::empty();
        }
        let pipes = core::ptr::addr_of_mut!(APP_PIPES);
        let pipes = &mut *pipes;
        for pipe in pipes.iter_mut() {
            *pipe = AppPipe::empty();
        }
    }
}

/// Sets up `PROCESS_TABLE[index]`'s heap/mmap bump-allocator window. Takes
/// an explicit index (not `CURRENT_PROCESS_INDEX`) since category 2's
/// scheduler work prepares a new process's window *before* ever switching
/// to it -- see `prepare_process`. `heap_base`/`heap_limit`/`mmap_limit`
/// are pure functions of `pid` (a fixed per-pid address slice) and are
/// simply (re)computed here; `heap_next`/`mmap_next` are only reset to the
/// base the *first* time a given process's window is set up, so a process
/// resumed after being switched away (once real concurrency exists) never
/// has its bump-pointer progress clobbered back to the base.
fn app_set_heap_window(index: usize, pid: u32) {
    unsafe {
        let base = USER_HEAP_BASE + pid as u64 * USER_HEAP_STRIDE;
        let mmap_base = USER_MMAP_BASE + pid as u64 * USER_MMAP_STRIDE;
        if index <= PROCESS_COUNT {
            let table = &raw mut PROCESS_TABLE;
            let process = &mut (*table)[index];
            let first_setup = process.heap_base == 0;
            process.heap_base = base;
            process.heap_limit = base + USER_HEAP_STRIDE;
            process.mmap_limit = mmap_base + USER_MMAP_SIZE;
            if first_setup {
                process.heap_next = base;
                process.mmap_next = mmap_base;
            }
        }
    }
}

fn checked_app_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() || len == 0 || len > INPUT_MAX {
        return None;
    }

    Some(unsafe { core::slice::from_raw_parts(ptr, len) })
}

fn process_config_sys(console: &mut Console, bootfs: BootFs) {
    if bootfs.find_data(b"config.sys").is_some() {
        console.write_line("config.sys loaded");
    } else {
        console.write_line("config.sys missing");
    }
}

fn run_script(console: &mut Console, fs: &mut RamFs, bootfs: BootFs, name: &[u8]) {
    let Some(data) = bootfs.find_data(name) else {
        return;
    };

    console.write("running ");
    console.write_bytes(name);
    console.new_line();

    let mut start = 0usize;
    for index in 0..=data.len() {
        if index == data.len() || data[index] == b'\n' {
            let mut line = &data[start..index];
            if matches!(line.last(), Some(b'\r')) {
                line = &line[..line.len() - 1];
            }
            let line = trim(line);
            if !line.is_empty() && !starts_with(line, b"rem") {
                run_command(console, fs, bootfs, line);
            }
            start = index + 1;
        }
    }
}

/// Runs a top-level console `run <program>` command. Prepares the process
/// exactly like a nested `spawn` does now (`prepare_process` -- isolated
/// address space, own kernel stack, initial context) and then blocks on it
/// the same way `abi_wait` does (`wait_for_pid_blocking`), so the console
/// stays "synchronous from the shell's perspective" (a `run` command
/// doesn't return to the prompt until the program exits, matching every
/// user-visible behavior before this refactor) while the process itself
/// now runs through the real scheduler rather than a nested function call.
/// A top-level process has no ABI parent to inherit stdio/cwd/env from, so
/// it starts fresh (matching pre-refactor behavior) instead of copying the
/// console's own ambient state the way `spawn_prepared` copies a real
/// parent's.
fn run_program(console: &mut Console, bootfs: BootFs, name: &[u8], args: &[u8]) {
    let Some(process_index) = process_spawn(name, args) else {
        console.write_line("run: process table full");
        return;
    };

    let mut path = [0u8; 32];
    let path_len = program_path(name, &mut path);
    let Some(image) = bootfs.find_data(&path[..path_len]) else {
        console.write_line("run: program not found");
        process_fail_unwaited(process_index);
        return;
    };

    console.write("run: ");
    console.write_bytes(&path[..path_len]);
    console.write(" pid ");
    console.write_usize(process_pid(process_index) as usize);
    console.new_line();

    unsafe {
        APP_CONSOLE = console as *mut Console;
        APP_BOOTFS = bootfs;
        // Shared (not per-process, see `Process`'s docs) fd/pipe table
        // hygiene between sequential top-level shell commands -- matches
        // pre-refactor behavior. Safe today since the shell only ever has
        // one top-level foreground command pending at a time (it blocks
        // below until this exact process exits before processing anything
        // else); revisit once Stage 4 adds real backgrounded daemons that
        // could still be alive when a *different* top-level command starts.
        app_close_all_fds();
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[process_index];
        process.std_fds = [STDIN_FD, STDOUT_FD, STDERR_FD];
        process.cwd.fill(0);
        process.cwd[0] = b'/';
        process.cwd_len = 1;
        process.last_error = ERR_OK;
    }

    if !prepare_process(process_index, console, image) {
        console.write_line("run: failed to prepare process");
        return;
    }

    let pid = process_pid(process_index);
    let code = match wait_for_pid_blocking(pid) {
        Some(status) => status.exit_code,
        None => -1,
    };

    // Same reasoning as `process_fail_unwaited`: nobody will ever call
    // `wait`/`wait_any` on a top-level `run`'d process, so it must not
    // become an unreapable zombie.
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        (*table)[process_index].waited = true;
    }

    console.write("exit ");
    console.write_i32(code);
    console.new_line();
}

fn program_path<'a>(name: &[u8], output: &'a mut [u8; 32]) -> usize {
    if contains(name, b'/') {
        output[..name.len()].copy_from_slice(name);
        return name.len();
    }

    let prefix = b"programs/";
    let suffix = b".elf";
    let mut len = 0usize;
    output[..prefix.len()].copy_from_slice(prefix);
    len += prefix.len();
    output[len..len + name.len()].copy_from_slice(name);
    len += name.len();
    if !ends_with(name, suffix) {
        output[len..len + suffix.len()].copy_from_slice(suffix);
        len += suffix.len();
    }
    len
}

/// Loads an ELF image's `PT_LOAD` segments for a process with its own
/// private address space (see `create_process_address_space`): maps fresh
/// physical pages for every segment into the given PML4 rather than
/// assuming the fixed program-image window is already accessible. Every
/// process is isolated this way now (category 2's scheduler work unified
/// top-level `run` commands and nested `spawn`s onto the same
/// `prepare_process` path, since real concurrent scheduling needs every
/// runnable process to have its own address space regardless of how it
/// was started).
///
/// Deliberately does *not* use `process_track_mapping`/`process_reclaim_mappings`:
/// those always unmap against the shared `KERNEL_PML4_PHYS` (that's correct
/// for heap/mmap, which really do live there), but these segment pages live
/// in the process's *private* PML4 instead. Using the wrong PML4 to unmap
/// them wouldn't just fail quietly -- physical pages below `APP_LOAD_MAX`
/// aren't part of any `PHYS_ALLOCATOR` range, and `free_page` doesn't
/// validate that, so it would push a firmware-owned address onto the
/// allocator's free list. `destroy_process_address_space` reclaims these
/// pages correctly instead, by walking the private PD it already owns.
///
/// The caller must already have `pml4_phys` active in CR3 -- segment bytes
/// are written through the mapped virtual addresses directly.
fn load_program_elf_isolated(console: &mut Console, elf: &[u8], pml4_phys: u64) -> Option<u64> {
    if elf.len() < core::mem::size_of::<Elf64Header>() {
        console.write_line("run: bad elf header");
        return None;
    }

    let header = unsafe { &*(elf.as_ptr().cast::<Elf64Header>()) };
    if &header.ident[0..4] != ELF_MAGIC || header.ident[4] != 2 || header.machine != 0x3E {
        console.write_line("run: unsupported elf");
        return None;
    }

    let phoff = header.phoff as usize;
    let phentsize = header.phentsize as usize;
    let phnum = header.phnum as usize;
    if phentsize < core::mem::size_of::<Elf64ProgramHeader>()
        || phoff + phentsize * phnum > elf.len()
    {
        console.write_line("run: bad program headers");
        return None;
    }

    for index in 0..phnum {
        let ph = unsafe {
            &*(elf
                .as_ptr()
                .add(phoff + index * phentsize)
                .cast::<Elf64ProgramHeader>())
        };
        if ph.typ != PT_LOAD || ph.memsz == 0 {
            continue;
        }
        if ph.offset as usize + ph.filesz as usize > elf.len() || ph.filesz > ph.memsz {
            console.write_line("run: bad load segment");
            return None;
        }
        if ph.paddr < APP_LOAD_MIN || ph.paddr + ph.memsz > APP_LOAD_MAX {
            console.write_line("run: segment outside app area");
            return None;
        }

        let page_start = ph.paddr & !(PAGE_SIZE - 1);
        let page_end = (ph.paddr + ph.memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let page_count = ((page_end - page_start) / PAGE_SIZE) as usize;

        if !map_image_pages(pml4_phys, page_start, page_count) {
            console.write_line("run: segment map failed");
            return None;
        }

        let destination = ph.paddr as *mut u8;
        unsafe {
            zero_bytes(destination, ph.memsz as usize);
            copy_nonoverlapping(
                elf.as_ptr().add(ph.offset as usize),
                destination,
                ph.filesz as usize,
            );
        }
    }

    Some(header.entry)
}

unsafe fn zero_bytes(destination: *mut u8, length: usize) {
    for index in 0..length {
        unsafe {
            destination.add(index).write_volatile(0);
        }
    }
}

fn show_drivers(console: &mut Console) {
    console.write_line("driver                 state");
    console.write_line("serial 16550           active");
    console.write_line("ps2 keyboard           polling");
    console.write_line("vga text               active");
    console.write_line("uefi gop framebuffer   active");
    console.write_line("ramfs                  active");
    console.write_line("pci config             polling");
    console.write_line("ata/net/sound          planned from MOROS study");
}

fn show_devices(console: &mut Console) {
    console.write_line("/dev/console      serial + keyboard console");
    console.write_line("/dev/null         planned sink device");
    console.write_line("/dev/random       planned rng device");
    console.write_line("/dev/vga          text + framebuffer display");
    console.write_line("/dev/kbd          ps2 keyboard poller");
    console.write_line("/dev/pci          config-space scanner");
    console.write_line("/dev/ramfs        in-kernel volatile filesystem");
}

fn pci_scan(console: &mut Console) {
    let mut found = 0usize;
    console.write_line("bus dev fn vendor device class subclass");
    for bus in 0u8..=0 {
        for device in 0u8..32 {
            let vendor = pci_read_u16(bus, device, 0, 0x00);
            if vendor == 0xFFFF {
                continue;
            }
            let header_type = pci_read_u8(bus, device, 0, 0x0E);
            let functions = if header_type & 0x80 != 0 { 8 } else { 1 };
            for function in 0u8..functions {
                let vendor = pci_read_u16(bus, device, function, 0x00);
                if vendor == 0xFFFF {
                    continue;
                }
                let device_id = pci_read_u16(bus, device, function, 0x02);
                let class = pci_read_u8(bus, device, function, 0x0B);
                let subclass = pci_read_u8(bus, device, function, 0x0A);

                console.write_hex_u8(bus);
                console.write("  ");
                console.write_hex_u8(device);
                console.write("  ");
                console.write_hex_u8(function);
                console.write("  ");
                console.write_hex_u16(vendor);
                console.write("   ");
                console.write_hex_u16(device_id);
                console.write("   ");
                console.write_hex_u8(class);
                console.write("    ");
                console.write_hex_u8(subclass);
                console.write("       ");
                console.write_line(pci_class_name(class, subclass));
                found += 1;
            }
        }
    }
    if found == 0 {
        console.write_line("no pci devices found");
    }
}

fn pci_class_name(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x01, 0x01) => "ide controller",
        (0x01, 0x06) => "sata controller",
        (0x02, 0x00) => "ethernet controller",
        (0x03, 0x00) => "vga/display controller",
        (0x04, 0x01) => "audio device",
        (0x06, 0x00) => "host bridge",
        (0x06, 0x01) => "isa bridge",
        (0x06, 0x04) => "pci bridge",
        (0x0C, 0x03) => "usb controller",
        _ => "device",
    }
}

fn pci_read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = pci_read_u32(bus, device, function, offset & 0xFC);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

fn pci_read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = pci_read_u32(bus, device, function, offset & 0xFC);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

fn pci_read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);
    unsafe {
        outl(0xCF8, address);
        inl(0xCFC)
    }
}

fn glyph(byte: u8) -> [u8; 7] {
    match byte {
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        b'3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        b'6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        b'a' | b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'b' | b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'c' | b'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'd' | b'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        b'e' | b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'f' | b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'g' | b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'h' | b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'i' | b'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'j' | b'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        b'k' | b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'l' | b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'm' | b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'n' | b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'o' | b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'p' | b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'q' | b'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'r' | b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b's' | b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b't' | b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'u' | b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'v' | b'V' => [0x11, 0x11, 0x11, 0x11, 0x0A, 0x0A, 0x04],
        b'w' | b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        b'x' | b'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'y' | b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'z' | b'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b' ' => [0, 0, 0, 0, 0, 0, 0],
        b'.' => [0, 0, 0, 0, 0, 0x0C, 0x0C],
        b',' => [0, 0, 0, 0, 0, 0x0C, 0x08],
        b':' => [0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0],
        b';' => [0, 0x0C, 0x0C, 0, 0x0C, 0x08, 0x10],
        b'/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        b'\\' => [0x10, 0x10, 0x08, 0x04, 0x02, 0x01, 0x01],
        b'-' => [0, 0, 0, 0x1F, 0, 0, 0],
        b'_' => [0, 0, 0, 0, 0, 0, 0x1F],
        b'=' => [0, 0x1F, 0, 0x1F, 0, 0, 0],
        b'+' => [0, 0x04, 0x04, 0x1F, 0x04, 0x04, 0],
        b'$' => [0x04, 0x0F, 0x14, 0x0E, 0x05, 0x1E, 0x04],
        b'>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        b'<' => [0x01, 0x02, 0x04, 0x08, 0x04, 0x02, 0x01],
        b'[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        b']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        b'(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b'!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04],
        b'?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
        b'\'' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        b'"' => [0x0A, 0x0A, 0, 0, 0, 0, 0],
        _ => [0x1F, 0x11, 0x02, 0x04, 0x04, 0, 0x04],
    }
}

fn read_input_byte() -> Option<u8> {
    unsafe {
        if serial_received() {
            return Some(inb(COM1));
        }

        if keyboard_has_data() {
            return scancode_to_ascii(inb(KEYBOARD_DATA));
        }
    }

    None
}

fn scancode_to_ascii(scancode: u8) -> Option<u8> {
    if scancode & 0x80 != 0 {
        return None;
    }

    match scancode {
        0x02 => Some(b'1'),
        0x03 => Some(b'2'),
        0x04 => Some(b'3'),
        0x05 => Some(b'4'),
        0x06 => Some(b'5'),
        0x07 => Some(b'6'),
        0x08 => Some(b'7'),
        0x09 => Some(b'8'),
        0x0A => Some(b'9'),
        0x0B => Some(b'0'),
        0x0C => Some(b'-'),
        0x0D => Some(b'='),
        0x0E => Some(8),
        0x0F => Some(b' '),
        0x10 => Some(b'q'),
        0x11 => Some(b'w'),
        0x12 => Some(b'e'),
        0x13 => Some(b'r'),
        0x14 => Some(b't'),
        0x15 => Some(b'y'),
        0x16 => Some(b'u'),
        0x17 => Some(b'i'),
        0x18 => Some(b'o'),
        0x19 => Some(b'p'),
        0x1A => Some(b'['),
        0x1B => Some(b']'),
        0x1C => Some(b'\n'),
        0x1E => Some(b'a'),
        0x1F => Some(b's'),
        0x20 => Some(b'd'),
        0x21 => Some(b'f'),
        0x22 => Some(b'g'),
        0x23 => Some(b'h'),
        0x24 => Some(b'j'),
        0x25 => Some(b'k'),
        0x26 => Some(b'l'),
        0x27 => Some(b';'),
        0x28 => Some(b'\''),
        0x29 => Some(b'`'),
        0x2B => Some(b'\\'),
        0x2C => Some(b'z'),
        0x2D => Some(b'x'),
        0x2E => Some(b'c'),
        0x2F => Some(b'v'),
        0x30 => Some(b'b'),
        0x31 => Some(b'n'),
        0x32 => Some(b'm'),
        0x33 => Some(b','),
        0x34 => Some(b'.'),
        0x35 => Some(b'/'),
        0x39 => Some(b' '),
        _ => None,
    }
}

fn trim(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\t')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\t')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn split_word(bytes: &[u8]) -> (&[u8], &[u8]) {
    let bytes = trim(bytes);
    for index in 0..bytes.len() {
        if bytes[index] == b' ' || bytes[index] == b'\t' {
            return (&bytes[..index], trim(&bytes[index + 1..]));
        }
    }
    (bytes, &[])
}

fn eq(left: &[u8], right: &[u8]) -> bool {
    left == right
}

fn starts_with(value: &[u8], prefix: &[u8]) -> bool {
    value.len() >= prefix.len() && &value[..prefix.len()] == prefix
}

fn ends_with(value: &[u8], suffix: &[u8]) -> bool {
    value.len() >= suffix.len() && &value[value.len() - suffix.len()..] == suffix
}

fn contains(value: &[u8], needle: u8) -> bool {
    for byte in value {
        if *byte == needle {
            return true;
        }
    }
    false
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    if bytes.is_empty() {
        return None;
    }
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?;
        value = value.checked_add((*byte - b'0') as u32)?;
    }
    Some(value)
}

fn align_up_u64(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

fn align_down_u64(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn valid_name(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > FILE_NAME_MAX {
        return false;
    }
    for byte in name {
        if !(byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'_' || *byte == b'-') {
            return false;
        }
    }
    true
}

unsafe fn read_le32(ptr: *const u8) -> u32 {
    unsafe {
        (*ptr as u32)
            | ((*ptr.add(1) as u32) << 8)
            | ((*ptr.add(2) as u32) << 16)
            | ((*ptr.add(3) as u32) << 24)
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    unsafe {
        core::arch::asm!(
            "in ax, dx",
            out("ax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn outw(port: u16, value: u16) {
    unsafe {
        core::arch::asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    unsafe {
        core::arch::asm!(
            "in eax, dx",
            out("eax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

unsafe fn outl(port: u16, value: u32) {
    unsafe {
        core::arch::asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

fn ata_present() -> bool {
    unsafe {
        ata_select_drive(ATA_DATA_DRIVE, 0);
        let status = inb(ATA_PRIMARY_IO + 7);
        status != 0x00 && status != 0xFF
    }
}

fn ata_read_sector(drive: u8, lba: u32, buffer: &mut [u8; 512]) -> bool {
    unsafe {
        ata_select_drive(drive, lba);
        outb(ATA_PRIMARY_IO + 2, 1);
        outb(ATA_PRIMARY_IO + 3, lba as u8);
        outb(ATA_PRIMARY_IO + 4, (lba >> 8) as u8);
        outb(ATA_PRIMARY_IO + 5, (lba >> 16) as u8);
        outb(ATA_PRIMARY_IO + 7, 0x20);
        if !ata_wait_drq() {
            return false;
        }
        for index in 0..256 {
            let word = inw(ATA_PRIMARY_IO);
            buffer[index * 2] = word as u8;
            buffer[index * 2 + 1] = (word >> 8) as u8;
        }
        true
    }
}

fn ata_write_sector(drive: u8, lba: u32, buffer: &[u8; 512]) -> bool {
    unsafe {
        ata_select_drive(drive, lba);
        outb(ATA_PRIMARY_IO + 2, 1);
        outb(ATA_PRIMARY_IO + 3, lba as u8);
        outb(ATA_PRIMARY_IO + 4, (lba >> 8) as u8);
        outb(ATA_PRIMARY_IO + 5, (lba >> 16) as u8);
        outb(ATA_PRIMARY_IO + 7, 0x30);
        if !ata_wait_drq() {
            return false;
        }
        for index in 0..256 {
            let word = buffer[index * 2] as u16 | ((buffer[index * 2 + 1] as u16) << 8);
            outw(ATA_PRIMARY_IO, word);
        }
        outb(ATA_PRIMARY_IO + 7, 0xE7);
        ata_wait_not_busy()
    }
}

unsafe fn ata_select_drive(drive: u8, lba: u32) {
    unsafe {
        outb(
            ATA_PRIMARY_IO + 6,
            0xE0 | ((drive & 1) << 4) | (((lba >> 24) as u8) & 0x0F),
        );
        for _ in 0..4 {
            inb(ATA_PRIMARY_CTRL);
        }
    }
}

fn ata_wait_not_busy() -> bool {
    for _ in 0..1_000_000 {
        let status = unsafe { inb(ATA_PRIMARY_IO + 7) };
        if status & 0x80 == 0 {
            return status & 0x01 == 0;
        }
        spin_loop();
    }
    false
}

fn ata_wait_drq() -> bool {
    for _ in 0..1_000_000 {
        let status = unsafe { inb(ATA_PRIMARY_IO + 7) };
        if status & 0x01 != 0 {
            return false;
        }
        if status & 0x80 == 0 && status & 0x08 != 0 {
            return true;
        }
        spin_loop();
    }
    false
}

unsafe fn serial_init() {
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1, 0x03);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0xC7);
        outb(COM1 + 4, 0x0B);
    }
}

unsafe fn serial_received() -> bool {
    unsafe { inb(COM1 + 5) & 1 != 0 }
}

unsafe fn serial_can_send() -> bool {
    unsafe { inb(COM1 + 5) & 0x20 != 0 }
}

unsafe fn serial_write_byte(byte: u8) {
    unsafe {
        while !serial_can_send() {
            spin_loop();
        }
        outb(COM1, byte);
    }
}

unsafe fn keyboard_has_data() -> bool {
    unsafe { inb(KEYBOARD_STATUS) & 1 != 0 }
}

// --- CPU exception handling -------------------------------------------------
//
// Before this, this kernel had no IDT at all: any fault (a null-pointer read,
// a bad guard-page access, an out-of-bounds write) triple-faulted the whole
// machine with zero diagnostics -- QEMU just silently reset. This installs
// handlers for the 32 CPU exception vectors that print a clear diagnostic
// over serial (vector, mnemonic, error code, faulting RIP, CR2 for page
// faults, and the current process if one is running) and then halt, instead
// of an unexplained reboot.
//
// Deliberately *not* attempted: recovering and continuing (killing just the
// offending process while the rest of the OS keeps running). That needs a
// prepared non-local-exit point to unwind out of the arbitrary nested Rust
// call stack a fault can land in -- a real, separate piece of work, not a
// bounded extension of this. Also not attempted: a dedicated IST/TSS stack
// for the double-fault handler, so a double fault caused by genuine kernel
// stack exhaustion could still (rarely) overrun into a real triple fault
// when the handler itself has no stack room left -- every other fault still
// gets a clean diagnostic.
//
// No GDT is built here either: the kernel has run entirely on whichever GDT
// UEFI's firmware set up (never replaced, only paging got its own kernel-
// owned tables via `vmclone`), and since we're already executing through it
// in 64-bit ring 0, its active `cs` selector is exactly the one IDT gate
// descriptors need -- read at init time instead of assuming a fixed value.

const IDT_LEN: usize = 48;
const IDT_GATE_PRESENT_INTERRUPT: u8 = 0x8E;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; IDT_LEN] = [IdtEntry::missing(); IDT_LEN];

/// The CPU-pushed frame below the vector number and error code every stub
/// normalizes onto the stack (see the `isr_stub_*` macros). Same-privilege
/// fault (we only ever run ring 0), so there's no stack-segment switch
/// involved beyond what's listed here.
#[repr(C)]
struct InterruptStackFrame {
    instruction_pointer: u64,
    code_segment: u64,
    cpu_flags: u64,
    stack_pointer: u64,
    stack_segment: u64,
}

/// Exact layout each `isr_stub_*` leaves on the stack before jumping to
/// `isr_common`: the vector number and error code it normalized (a real
/// hardware-pushed one for vectors that have one, a fake `0` pushed by the
/// stub itself otherwise -- see `vector_info` for which is which), directly
/// followed by the CPU's own `InterruptStackFrame`.
#[repr(C)]
struct RawExceptionFrame {
    vector: u64,
    error_code: u64,
    frame: InterruptStackFrame,
}

/// Mnemonic plus whether this vector's error code (see `RawExceptionFrame`)
/// is a real one from the CPU rather than the stub's own filler `0`.
fn vector_info(vector: u8) -> (&'static str, bool) {
    match vector {
        0 => ("divide error", false),
        1 => ("debug", false),
        2 => ("non-maskable interrupt", false),
        3 => ("breakpoint", false),
        4 => ("overflow", false),
        5 => ("bound range exceeded", false),
        6 => ("invalid opcode", false),
        7 => ("device not available", false),
        8 => ("double fault", true),
        10 => ("invalid TSS", true),
        11 => ("segment not present", true),
        12 => ("stack fault", true),
        13 => ("general protection fault", true),
        14 => ("page fault", true),
        16 => ("x87 floating point", false),
        17 => ("alignment check", true),
        18 => ("machine check", false),
        19 => ("SIMD floating point", false),
        20 => ("virtualization", false),
        21 => ("control protection", true),
        _ => ("unknown", false),
    }
}

fn read_cr2() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

fn fault_print(text: &str) {
    for byte in text.bytes() {
        unsafe {
            serial_write_byte(byte);
        }
    }
}

fn fault_print_hex(value: u64) {
    fault_print("0x");
    let mut started = false;
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        if nibble != 0 || started || shift == 0 {
            started = true;
            let digit = if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + (nibble - 10)
            };
            unsafe {
                serial_write_byte(digit);
            }
        }
    }
}

/// Common diagnostic path for every installed exception vector: print what
/// faulted and where, then halt. Written to depend on as little of the rest
/// of the kernel as possible (raw serial writes, not the `Console`/
/// `APP_CONSOLE` machinery) since a fault can land here from literally
/// anywhere -- including before any process, or even the shell, has set up
/// console state.
fn report_fault(vector: u8, mnemonic: &str, frame: &InterruptStackFrame, error_code: Option<u64>) -> ! {
    fault_print("\r\n!! CPU EXCEPTION vector=");
    fault_print_hex(vector as u64);
    fault_print(" (");
    fault_print(mnemonic);
    fault_print(")\r\n");
    if let Some(code) = error_code {
        fault_print("   error_code=");
        fault_print_hex(code);
        fault_print("\r\n");
    }
    if vector == 14 {
        fault_print("   cr2 (fault address)=");
        fault_print_hex(read_cr2());
        fault_print("\r\n");
    }
    fault_print("   rip=");
    fault_print_hex(frame.instruction_pointer);
    fault_print(" cs=");
    fault_print_hex(frame.code_segment);
    fault_print(" rflags=");
    fault_print_hex(frame.cpu_flags);
    fault_print("\r\n   rsp=");
    fault_print_hex(frame.stack_pointer);
    fault_print(" ss=");
    fault_print_hex(frame.stack_segment);
    fault_print("\r\n");
    unsafe {
        let index = CURRENT_PROCESS_INDEX;
        if index < PROCESS_COUNT {
            let table = &raw const PROCESS_TABLE;
            let process = &(*table)[index];
            fault_print("   pid=");
            fault_print_hex(process.pid as u64);
            fault_print(" name=");
            fault_print(core::str::from_utf8(&process.name[..process.name_len]).unwrap_or("?"));
            fault_print("\r\n");
        } else {
            fault_print("   (fault outside any process context)\r\n");
        }
    }
    fault_print("!! halting\r\n");
    loop {
        unsafe {
            asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}

/// Common landing point every `isr_stub_*` jumps to once it has normalized
/// the stack to a `RawExceptionFrame` layout. `rdi` already holds a pointer
/// to that frame (the SysV ABI's first integer argument register) by the
/// time this runs, exactly as if it had been `call`ed with that one pointer
/// argument -- diverging, so the stubs never need to restore anything or
/// `iretq` back.
extern "sysv64" fn isr_common_entry(raw: *const RawExceptionFrame) -> ! {
    let raw = unsafe { &*raw };
    let vector = raw.vector as u8;
    let (mnemonic, has_error_code) = vector_info(vector);
    let error_code = if has_error_code { Some(raw.error_code) } else { None };
    report_fault(vector, mnemonic, &raw.frame, error_code);
}

/// The x86-interrupt calling convention is unstable (nightly-only), and this
/// kernel builds on stable, so each vector gets a small hand-written naked
/// stub instead: normalize the stack to a `RawExceptionFrame` (pushing a
/// filler `0` error code first for vectors the CPU doesn't supply one for,
/// so every vector ends up with the exact same layout regardless), point
/// `rdi` at it, 16-byte-align `rsp` for the SysV ABI, and jump into the one
/// shared `isr_common_entry`. Never returns, so there's no epilogue/`iretq`
/// to get right -- every installed handler just diagnoses and halts.
macro_rules! isr_stub_noerr {
    ($name:ident, $vector:literal) => {
        #[unsafe(naked)]
        extern "sysv64" fn $name() -> ! {
            core::arch::naked_asm!(
                "push 0",
                concat!("push ", $vector),
                "mov rdi, rsp",
                "and rsp, -16",
                "call {entry}",
                entry = sym isr_common_entry,
            )
        }
    };
}

macro_rules! isr_stub_err {
    ($name:ident, $vector:literal) => {
        #[unsafe(naked)]
        extern "sysv64" fn $name() -> ! {
            core::arch::naked_asm!(
                concat!("push ", $vector),
                "mov rdi, rsp",
                "and rsp, -16",
                "call {entry}",
                entry = sym isr_common_entry,
            )
        }
    };
}

isr_stub_noerr!(isr_stub_0, 0);
isr_stub_noerr!(isr_stub_1, 1);
isr_stub_noerr!(isr_stub_2, 2);
isr_stub_noerr!(isr_stub_3, 3);
isr_stub_noerr!(isr_stub_4, 4);
isr_stub_noerr!(isr_stub_5, 5);
isr_stub_noerr!(isr_stub_6, 6);
isr_stub_noerr!(isr_stub_7, 7);
isr_stub_err!(isr_stub_8, 8);
isr_stub_err!(isr_stub_10, 10);
isr_stub_err!(isr_stub_11, 11);
isr_stub_err!(isr_stub_12, 12);
isr_stub_err!(isr_stub_13, 13);
isr_stub_err!(isr_stub_14, 14);
isr_stub_noerr!(isr_stub_16, 16);
isr_stub_err!(isr_stub_17, 17);
isr_stub_noerr!(isr_stub_18, 18);
isr_stub_noerr!(isr_stub_19, 19);
isr_stub_noerr!(isr_stub_20, 20);
isr_stub_err!(isr_stub_21, 21);

// --- Timer interrupt (IRQ0) -------------------------------------------------
//
// Stage 3a: plumbing only, no rescheduling yet -- deliberately its own
// checkpoint before the scheduler core gets wired to a tick, since this is
// the highest-risk code in the whole scheduler/preemption plan: the first
// interrupt in this kernel that must *resume* the interrupted task, not
// diagnose-and-halt like every isr_stub_* above.
//
// The 8259 PIC's power-on vector base (0x08) collides with CPU exception
// vectors 8-15 (8 = double fault), so it must be remapped before ever
// unmasking an IRQ -- `remap_pic` moves IRQ0-7 to vectors 32-39 and IRQ8-15
// to 40-47. Only IRQ0 (the PIT) is left unmasked; every other line stays
// masked until something actually needs it. The benign spurious-IRQ stubs
// below exist purely as defense in depth -- a well-known hardware quirk can
// still occasionally raise IRQ7/IRQ15 even while masked -- distinct from the
// fatal exception path so a spurious hit EOIs and resumes instead of halting
// the whole kernel over nothing.
//
// `irq0_stub` saves every general-purpose register (an interrupt can land
// mid-instruction with any of them live, unlike a normal call site where the
// compiler has already spilled what it needs), calls into Rust to bump the
// tick counter and send the End-Of-Interrupt, restores every register, and
// `iretq`s. The incoming stack alignment at an arbitrary interrupted
// instruction can't be assumed 16-byte aligned, so it snapshots `rsp` into
// `rbp` (free to reuse -- `rbp`'s real value is already saved on the stack
// by that point), aligns down for the SysV `call`, then restores the exact
// original `rsp` before popping everything back and `iretq`ing -- correct
// regardless of what the interrupted code's stack looked like.
//
// Only `sti` once all of this -- the IDT gates, the PIC remap, and the PIT
// programming -- is in place (`enable_timer_interrupts`, called once from
// `_start`). This is the first and only `sti` anywhere in this kernel.

static mut TIMER_TICKS: u64 = 0;

unsafe fn io_wait() {
    unsafe {
        outb(0x80, 0);
    }
}

fn remap_pic() {
    unsafe {
        outb(PIC1_COMMAND, 0x11); // ICW1: begin init, ICW4 present
        io_wait();
        outb(PIC2_COMMAND, 0x11);
        io_wait();
        outb(PIC1_DATA, 0x20); // ICW2: master offset -> vector 32
        io_wait();
        outb(PIC2_DATA, 0x28); // ICW2: slave offset -> vector 40
        io_wait();
        outb(PIC1_DATA, 0x04); // ICW3: slave attached at master's IRQ2
        io_wait();
        outb(PIC2_DATA, 0x02); // ICW3: slave's own cascade identity
        io_wait();
        outb(PIC1_DATA, 0x01); // ICW4: 8086 mode
        io_wait();
        outb(PIC2_DATA, 0x01);
        io_wait();
        outb(PIC1_DATA, 0xFE); // mask every master line except IRQ0
        outb(PIC2_DATA, 0xFF); // mask every slave line
    }
}

fn init_pit_timer(hz: u32) {
    let divisor = (PIT_INPUT_HZ / hz as u64) as u16;
    unsafe {
        outb(PIT_COMMAND, 0x36); // channel 0, lobyte/hibyte, mode 3, binary
        outb(PIT_CHANNEL0_DATA, (divisor & 0xFF) as u8);
        outb(PIT_CHANNEL0_DATA, (divisor >> 8) as u8);
    }
}

fn enable_timer_interrupts() {
    remap_pic();
    init_pit_timer(100);
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

extern "sysv64" fn timer_tick_isr() {
    unsafe {
        TIMER_TICKS += 1;
        outb(PIC1_COMMAND, PIC_EOI);
    }
}

#[unsafe(naked)]
extern "sysv64" fn irq0_stub() {
    core::arch::naked_asm!(
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rbp, rsp",
        "and rsp, -16",
        "call {handler}",
        "mov rsp, rbp",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "iretq",
        handler = sym timer_tick_isr,
    )
}

/// Benign default for the now-technically-reachable master-PIC vectors
/// (33-39, IRQ1-7) -- masked and not expected to fire, but a spurious IRQ7 is
/// a well-known hardware quirk that can occur even while masked. Pure inline
/// asm (no Rust call) since it never needs to inspect anything, just EOI and
/// resume.
#[unsafe(naked)]
extern "sysv64" fn irq_spurious_master_stub() {
    core::arch::naked_asm!(
        "push rax",
        "push rdx",
        "mov al, 0x20",
        "mov dx, 0x20",
        "out dx, al",
        "pop rdx",
        "pop rax",
        "iretq",
    )
}

/// Same as above for the slave-PIC vectors (40-47, IRQ8-15) -- a spurious
/// slave IRQ (classically IRQ15) needs EOI sent to both PICs, not just the
/// slave, or the master's in-service bit for the cascade line never clears.
#[unsafe(naked)]
extern "sysv64" fn irq_spurious_slave_stub() {
    core::arch::naked_asm!(
        "push rax",
        "push rdx",
        "mov al, 0x20",
        "mov dx, 0xA0",
        "out dx, al",
        "mov dx, 0x20",
        "out dx, al",
        "pop rdx",
        "pop rax",
        "iretq",
    )
}

fn set_idt_gate(vector: usize, handler: u64, selector: u16) {
    unsafe {
        let idt = &raw mut IDT;
        (*idt)[vector] = IdtEntry {
            offset_low: handler as u16,
            selector,
            ist: 0,
            type_attr: IDT_GATE_PRESENT_INTERRUPT,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        };
    }
}

/// Installs handlers for every CPU exception vector RYMOS currently expects
/// to ever see, then loads the IDT. Vectors Intel reserves and never
/// generates (9, 15, 22-31) are left `present = 0` on purpose: if one ever
/// somehow fired, using a not-present gate raises #GP instead, which we do
/// handle, so it still gets a diagnostic rather than a triple fault.
fn init_idt() {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }
    let cs: u16;
    unsafe {
        asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags));
    }

    set_idt_gate(0, isr_stub_0 as *const () as u64, cs);
    set_idt_gate(1, isr_stub_1 as *const () as u64, cs);
    set_idt_gate(2, isr_stub_2 as *const () as u64, cs);
    set_idt_gate(3, isr_stub_3 as *const () as u64, cs);
    set_idt_gate(4, isr_stub_4 as *const () as u64, cs);
    set_idt_gate(5, isr_stub_5 as *const () as u64, cs);
    set_idt_gate(6, isr_stub_6 as *const () as u64, cs);
    set_idt_gate(7, isr_stub_7 as *const () as u64, cs);
    set_idt_gate(8, isr_stub_8 as *const () as u64, cs);
    set_idt_gate(10, isr_stub_10 as *const () as u64, cs);
    set_idt_gate(11, isr_stub_11 as *const () as u64, cs);
    set_idt_gate(12, isr_stub_12 as *const () as u64, cs);
    set_idt_gate(13, isr_stub_13 as *const () as u64, cs);
    set_idt_gate(14, isr_stub_14 as *const () as u64, cs);
    set_idt_gate(16, isr_stub_16 as *const () as u64, cs);
    set_idt_gate(17, isr_stub_17 as *const () as u64, cs);
    set_idt_gate(18, isr_stub_18 as *const () as u64, cs);
    set_idt_gate(19, isr_stub_19 as *const () as u64, cs);
    set_idt_gate(20, isr_stub_20 as *const () as u64, cs);
    set_idt_gate(21, isr_stub_21 as *const () as u64, cs);

    set_idt_gate(32, irq0_stub as *const () as u64, cs);
    for vector in 33..=39usize {
        set_idt_gate(vector, irq_spurious_master_stub as *const () as u64, cs);
    }
    for vector in 40..=47usize {
        set_idt_gate(vector, irq_spurious_slave_stub as *const () as u64, cs);
    }

    unsafe {
        let idt = &raw const IDT;
        let pointer = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; IDT_LEN]>() - 1) as u16,
            base: idt as u64,
        };
        asm!("lidt [{}]", in(reg) &pointer, options(nostack, preserves_flags));
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        spin_loop();
    }
}
