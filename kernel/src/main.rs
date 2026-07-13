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
const INPUT_MAX: usize = 128;
const FILE_COUNT: usize = 12;
const FILE_NAME_MAX: usize = 16;
const FILE_DATA_MAX: usize = 192;
const PROCESS_COUNT: usize = 16;
const PROCESS_NAME_MAX: usize = 24;
const PROCESS_ARGS_MAX: usize = 64;
const PROCESS_HEAP_PAGE_MAX: usize = 1024;
const APP_CWD_MAX: usize = 64;
const ENV_COUNT: usize = 6;
const APP_FD_COUNT: usize = 4;
const APP_FD_BASE: i32 = 3;
const STDIN_FD: i32 = 0;
const STDOUT_FD: i32 = 1;
const STDERR_FD: i32 = 2;
const FD_READ: u32 = 1;
const FD_WRITE: u32 = 2;
const FD_CREATE: u32 = 4;
const FD_TRUNCATE: u32 = 8;
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
const PFS_HEADER_SECTORS: u32 = 8;
const PFS_HEADER_BYTES: usize = PFS_HEADER_SECTORS as usize * 512;
const PFS_ENTRY_COUNT: usize = 96;
const PFS_ENTRY_SIZE: usize = 40;
const PFS_NAME_MAX: usize = 30;
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
    spawn: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    wait: extern "sysv64" fn(u32, *mut RymosProcessStatus) -> i32,
    mem_alloc_pages: extern "sysv64" fn(usize) -> u64,
    time_ticks: extern "sysv64" fn() -> u64,
    unlink: extern "sysv64" fn(*const u8, usize) -> i32,
    rename: extern "sysv64" fn(*const u8, usize, *const u8, usize) -> i32,
    cwd: extern "sysv64" fn(*mut u8, usize) -> isize,
    chdir: extern "sysv64" fn(*const u8, usize) -> i32,
    last_error: extern "sysv64" fn() -> i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RymosStat {
    kind: u32,
    fs: u32,
    size: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RymosProcessStatus {
    state: u32,
    exit_code: i32,
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
}

#[derive(Clone, Copy)]
struct Process {
    pid: u32,
    state: ProcessState,
    exit_code: i32,
    name: [u8; PROCESS_NAME_MAX],
    name_len: usize,
    args: [u8; PROCESS_ARGS_MAX],
    args_len: usize,
    heap_base: u64,
    heap_pages: [u64; PROCESS_HEAP_PAGE_MAX],
    heap_page_count: usize,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppFdKind {
    Empty,
    BootFs,
    Pfs,
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

impl Process {
    const fn empty() -> Self {
        Self {
            pid: 0,
            state: ProcessState::Empty,
            exit_code: 0,
            name: [0; PROCESS_NAME_MAX],
            name_len: 0,
            args: [0; PROCESS_ARGS_MAX],
            args_len: 0,
            heap_base: 0,
            heap_pages: [0; PROCESS_HEAP_PAGE_MAX],
            heap_page_count: 0,
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
static mut APP_ARGS_PTR: *const u8 = core::ptr::null();
static mut APP_ARGS_LEN: usize = 0;
static mut APP_PID: u32 = 0;
static mut APP_PROCESS_INDEX: usize = PROCESS_COUNT;
static mut APP_FDS: [AppFd; APP_FD_COUNT] = [AppFd::empty(); APP_FD_COUNT];
static mut APP_CWD: [u8; APP_CWD_MAX] = [0; APP_CWD_MAX];
static mut APP_CWD_LEN: usize = 0;
static mut APP_LAST_ERROR: i32 = ERR_OK;
static mut PROCESS_TABLE: [Process; PROCESS_COUNT] = [Process::empty(); PROCESS_COUNT];
static mut NEXT_PID: u32 = 1;
static mut KERNEL_BOOT_INFO: *const BootInfo = core::ptr::null();
static mut PHYS_ALLOCATOR: PhysPageAllocator = PhysPageAllocator::empty();
static mut KERNEL_PML4_PHYS: u64 = 0;
static mut NEXT_SCRATCH_VIRT: u64 = KERNEL_SCRATCH_BASE;
static mut APP_HEAP_BASE: u64 = 0;
static mut APP_HEAP_NEXT: u64 = 0;
static mut APP_HEAP_LIMIT: u64 = 0;
static ENV: [(&[u8], &[u8]); ENV_COUNT] = [
    (b"PATH", b"programs"),
    (b"HOME", b"/"),
    (b"SHELL", b"rysh"),
    (b"USER", b"root"),
    (b"RYMOS_TARGET", b"x86_64-rymos"),
    (b"TMPDIR", b"pfs:tmp"),
];
static RYMOS_ABI: RymosAbi = RymosAbi {
    version: 11,
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
    spawn: abi_spawn,
    wait: abi_wait,
    mem_alloc_pages: abi_mem_alloc_pages,
    time_ticks: abi_time_ticks,
    unlink: abi_unlink,
    rename: abi_rename,
    cwd: abi_cwd,
    chdir: abi_chdir,
    last_error: abi_last_error,
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

struct PersistentFs;

impl PersistentFs {
    fn format(console: &mut Console) {
        if !ata_present() {
            console.write_line("pfs: ATA data disk not found");
            return;
        }

        let mut header = [0u8; PFS_HEADER_BYTES];
        header[0..8].copy_from_slice(b"RYMFS3\0\0");
        header[8] = 3;
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
        let start_sector = pfs_entry_start(&header, index);
        for sector_index in 0..sectors_for_len(size) {
            if remaining == 0 {
                break;
            }
            let mut sector = [0u8; 512];
            if !ata_read_sector(ATA_DATA_DRIVE, start_sector + sector_index, &mut sector) {
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

        let sectors = sectors_for_len(data.len());
        let start_sector = if sectors == 0 {
            0
        } else {
            let Some(start_sector) = pfs_alloc_extent(&header, sectors, Some(index)) else {
                console.write_line("pwrite: disk full");
                return;
            };
            start_sector
        };
        for sector_index in 0..sectors {
            let mut sector = [0u8; 512];
            let start = sector_index as usize * 512;
            if start < data.len() {
                let end = min(start + 512, data.len());
                sector[..end - start].copy_from_slice(&data[start..end]);
            }
            if !ata_write_sector(ATA_DATA_DRIVE, start_sector + sector_index, &sector) {
                console.write_line("pwrite: disk write failed");
                return;
            }
        }

        pfs_set_entry(
            &mut header,
            index,
            name,
            data.len(),
            PFS_KIND_FILE,
            start_sector,
        );
        if Self::write_header(&header) {
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
        pfs_set_entry(&mut header, index, name, 0, PFS_KIND_DIR, 0);
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
        if &header[0..8] != b"RYMFS3\0\0" {
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
    let start_sector = pfs_entry_start(&header, index);
    if start_sector == 0 && len != 0 {
        return false;
    }

    let mut copied = 0usize;
    while copied < len {
        let absolute = offset + copied;
        let sector_index = absolute / 512;
        let sector_offset = absolute % 512;
        let count = min(len - copied, 512 - sector_offset);
        let mut sector = [0u8; 512];
        if !ata_read_sector(
            ATA_DATA_DRIVE,
            start_sector + sector_index as u32,
            &mut sector,
        ) {
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
    if !pfs_ensure_file_capacity(index, new_size) {
        return false;
    }
    let Some(header) = PersistentFs::read_header_silent() else {
        return false;
    };
    let start_sector = pfs_entry_start(&header, index);

    let mut copied = 0usize;
    while copied < data.len() {
        let absolute = offset + copied;
        let sector_index = absolute / 512;
        let sector_offset = absolute % 512;
        let count = min(data.len() - copied, 512 - sector_offset);
        let lba = start_sector + sector_index as u32;
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

fn pfs_update_size(index: usize, size: usize) -> bool {
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return false;
    };
    if !pfs_entry_used(&header, index) {
        return false;
    }
    let offset = pfs_entry_offset(index);
    header[offset + 2..offset + 6].copy_from_slice(&(size as u32).to_le_bytes());
    PersistentFs::write_header(&header)
}

fn pfs_ensure_file_capacity(index: usize, size: usize) -> bool {
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return false;
    };
    if !pfs_entry_used(&header, index) || pfs_entry_is_dir(&header, index) {
        return false;
    }

    let old_size = pfs_entry_size(&header, index);
    let old_start = pfs_entry_start(&header, index);
    let old_sectors = sectors_for_len(old_size);
    let new_sectors = sectors_for_len(size);
    if new_sectors == 0 || (old_start != 0 && new_sectors <= old_sectors) {
        return true;
    }

    let Some(new_start) = pfs_alloc_extent(&header, new_sectors, Some(index)) else {
        return false;
    };
    if old_start != 0 && old_sectors != 0 && !pfs_copy_extent(old_start, new_start, old_sectors) {
        return false;
    }
    let name = {
        let old_name = pfs_entry_name(&header, index);
        let mut name = [0u8; PFS_NAME_MAX];
        name[..old_name.len()].copy_from_slice(old_name);
        (name, old_name.len())
    };
    let kind = pfs_entry_kind(&header, index);
    pfs_set_entry(
        &mut header,
        index,
        &name.0[..name.1],
        old_size,
        kind,
        new_start,
    );
    PersistentFs::write_header(&header)
}

fn pfs_copy_extent(old_start: u32, new_start: u32, sectors: u32) -> bool {
    for sector_index in 0..sectors {
        let mut sector = [0u8; 512];
        if !ata_read_sector(ATA_DATA_DRIVE, old_start + sector_index, &mut sector) {
            return false;
        }
        if !ata_write_sector(ATA_DATA_DRIVE, new_start + sector_index, &sector) {
            return false;
        }
    }
    true
}

fn pfs_alloc_extent(
    header: &[u8; PFS_HEADER_BYTES],
    sectors: u32,
    skip_index: Option<usize>,
) -> Option<u32> {
    if sectors == 0 || sectors > PFS_SECTORS_PER_FILE {
        return None;
    }
    let mut candidate = PFS_DATA_START;
    while candidate.checked_add(sectors)? <= PFS_DISK_SECTORS {
        let mut overlaps = false;
        for index in 0..PFS_ENTRY_COUNT {
            if Some(index) == skip_index
                || !pfs_entry_used(header, index)
                || pfs_entry_is_dir(header, index)
            {
                continue;
            }
            let used_start = pfs_entry_start(header, index);
            let used_sectors = sectors_for_len(pfs_entry_size(header, index));
            if used_start == 0 || used_sectors == 0 {
                continue;
            }
            let used_end = used_start + used_sectors;
            let candidate_end = candidate + sectors;
            if candidate < used_end && used_start < candidate_end {
                candidate = used_end;
                overlaps = true;
                break;
            }
        }
        if !overlaps {
            return Some(candidate);
        }
    }
    None
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
    unsafe {
        let table = &raw mut PROCESS_TABLE;
        let pid = NEXT_PID;
        NEXT_PID = NEXT_PID.wrapping_add(1);
        if NEXT_PID == 0 {
            NEXT_PID = 1;
        }

        if let Some(index) = process_find_slot(table, ProcessState::Empty)
            .or_else(|| process_find_slot(table, ProcessState::Exited))
            .or_else(|| process_find_slot(table, ProcessState::Failed))
        {
            let process = &mut (*table)[index];
            *process = Process::empty();
            process.pid = pid;
            process.state = ProcessState::Ready;
            process.name_len = min(name.len(), PROCESS_NAME_MAX);
            process.name[..process.name_len].copy_from_slice(&name[..process.name_len]);
            process.args_len = min(args.len(), PROCESS_ARGS_MAX);
            process.args[..process.args_len].copy_from_slice(&args[..process.args_len]);
            return Some(index);
        }
    }
    None
}

unsafe fn process_find_slot(
    table: *mut [Process; PROCESS_COUNT],
    state: ProcessState,
) -> Option<usize> {
    for index in 0..PROCESS_COUNT {
        if unsafe { (*table)[index].state } == state {
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

fn process_can_track_heap_pages(index: usize, pages: usize) -> bool {
    unsafe {
        let table = &raw const PROCESS_TABLE;
        index < PROCESS_COUNT && (*table)[index].heap_page_count + pages <= PROCESS_HEAP_PAGE_MAX
    }
}

fn process_track_heap_page(index: usize, page: u64) -> bool {
    unsafe {
        if index >= PROCESS_COUNT {
            return false;
        }
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[index];
        if process.heap_page_count >= PROCESS_HEAP_PAGE_MAX {
            return false;
        }
        process.heap_pages[process.heap_page_count] = page;
        process.heap_page_count += 1;
        true
    }
}

fn process_reclaim_heap_pages(index: usize) -> usize {
    unsafe {
        if index >= PROCESS_COUNT {
            return 0;
        }
        let table = &raw mut PROCESS_TABLE;
        let process = &mut (*table)[index];
        let allocator = &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR);
        let mut reclaimed = 0usize;
        for page_index in 0..process.heap_page_count {
            let page = process.heap_pages[page_index];
            let virt = process.heap_base + page_index as u64 * PAGE_SIZE;
            let unmapped = unmap_page(virt).unwrap_or(page);
            if unmapped != 0 && allocator.free_page(unmapped) {
                reclaimed += 1;
            }
            process.heap_pages[page_index] = 0;
        }
        process.heap_base = 0;
        process.heap_page_count = 0;
        reclaimed
    }
}

fn process_pid(index: usize) -> u32 {
    unsafe {
        let table = &raw const PROCESS_TABLE;
        (*table)[index].pid
    }
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
    let Some(pid) = parse_u32(pid_bytes) else {
        console.write_line("wait: invalid pid");
        return;
    };

    if let Some(status) = process_status_by_pid(pid) {
        console.write("pid ");
        console.write_usize(pid as usize);
        console.write(" ");
        console.write(process_state_name(process_state_from_u32(status.state)));
        console.write(" exit ");
        console.write_i32(status.exit_code);
        console.new_line();
        return;
    }

    console.write_line("wait: pid not found");
}

fn process_status_by_pid(pid: u32) -> Option<RymosProcessStatus> {
    unsafe {
        let table = &raw const PROCESS_TABLE;
        for index in 0..PROCESS_COUNT {
            let process = &(*table)[index];
            if process.pid == pid && !matches!(process.state, ProcessState::Empty) {
                return Some(RymosProcessStatus {
                    state: process.state as u32,
                    exit_code: process.exit_code,
                });
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
    }
}

fn process_state_from_u32(state: u32) -> ProcessState {
    match state {
        1 => ProcessState::Ready,
        2 => ProcessState::Running,
        3 => ProcessState::Exited,
        4 => ProcessState::Failed,
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
    let current_pml4 = read_cr3() & PAGE_ADDR_MASK;
    let kernel_pml4 = unsafe { KERNEL_PML4_PHYS };
    if kernel_pml4 != 0 {
        if current_pml4 != kernel_pml4 {
            unsafe {
                write_cr3(kernel_pml4);
            }
        }
        return Some(kernel_pml4);
    }

    let new_pml4 = clone_active_pml4()?;
    unsafe {
        KERNEL_PML4_PHYS = new_pml4;
        write_cr3(new_pml4);
    }
    Some(new_pml4)
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
    if !map_page(virt, phys, PAGE_PRESENT | PAGE_WRITABLE) {
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

fn map_page(virt: u64, phys: u64, flags: u64) -> bool {
    let pml4_phys = unsafe { KERNEL_PML4_PHYS };
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

fn unmap_page(virt: u64) -> Option<u64> {
    let pml4_phys = unsafe { KERNEL_PML4_PHYS };
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

fn alloc_zeroed_page() -> Option<u64> {
    unsafe {
        let allocator = &mut *core::ptr::addr_of_mut!(PHYS_ALLOCATOR);
        let page = allocator.alloc_page()?;
        write_bytes(page as *mut u8, 0, PAGE_SIZE as usize);
        Some(page)
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

unsafe fn write_cr3(value: u64) {
    unsafe {
        asm!("mov cr3, {}", in(reg) value, options(nostack, preserves_flags));
    }
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
    &header[offset + 10..offset + 10 + len]
}

fn pfs_entry_size(header: &[u8; PFS_HEADER_BYTES], index: usize) -> usize {
    let offset = pfs_entry_offset(index);
    read_le32_from_slice(&header[offset + 2..offset + 6]) as usize
}

fn pfs_entry_start(header: &[u8; PFS_HEADER_BYTES], index: usize) -> u32 {
    let offset = pfs_entry_offset(index);
    read_le32_from_slice(&header[offset + 6..offset + 10])
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
    start_sector: u32,
) {
    let offset = pfs_entry_offset(index);
    header[offset..offset + PFS_ENTRY_SIZE].fill(0);
    header[offset] = kind;
    header[offset + 1] = name.len() as u8;
    header[offset + 2..offset + 6].copy_from_slice(&(size as u32).to_le_bytes());
    header[offset + 6..offset + 10].copy_from_slice(&start_sector.to_le_bytes());
    header[offset + 10..offset + 10 + name.len()].copy_from_slice(name);
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
    let start_sector = pfs_entry_start(header, index);
    pfs_set_entry(header, index, new_name, size, kind, start_sector);
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

fn app_reset_path_state() {
    unsafe {
        let cwd = core::ptr::addr_of_mut!(APP_CWD);
        (*cwd).fill(0);
        (*cwd)[0] = b'/';
        APP_CWD_LEN = 1;
        APP_LAST_ERROR = ERR_OK;
    }
}

fn set_app_error(error: i32) -> i32 {
    unsafe {
        APP_LAST_ERROR = error;
    }
    -1
}

fn clear_app_error() {
    unsafe {
        APP_LAST_ERROR = ERR_OK;
    }
}

fn app_cwd_is_pfs() -> bool {
    unsafe {
        let cwd = core::ptr::addr_of!(APP_CWD);
        APP_CWD_LEN >= 4 && &(*cwd)[..4] == b"pfs:"
    }
}

fn app_cwd_pfs_name<'a>(buffer: &'a mut [u8; PFS_NAME_MAX]) -> &'a [u8] {
    unsafe {
        let cwd = core::ptr::addr_of!(APP_CWD);
        if APP_CWD_LEN <= 4 {
            return &buffer[..0];
        }
        let len = min(APP_CWD_LEN - 4, PFS_NAME_MAX);
        buffer[..len].copy_from_slice(&(*cwd)[4..4 + len]);
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
    unsafe { APP_PID }
}

extern "sysv64" fn abi_args(ptr: *mut u8, len: usize) -> usize {
    if ptr.is_null() || len == 0 {
        return unsafe { APP_ARGS_LEN };
    }

    unsafe {
        let copy_len = min(len, APP_ARGS_LEN);
        copy_nonoverlapping(APP_ARGS_PTR, ptr, copy_len);
        copy_len
    }
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
        return -1;
    };

    if starts_with(path, b"pfs:") {
        return abi_open_pfs(&path[4..], flags);
    }

    if flags & FD_WRITE != 0 {
        return -1;
    }

    unsafe {
        let bootfs = APP_BOOTFS;
        let Some(data) = bootfs.find_data(path) else {
            return -1;
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
                return index as i32 + APP_FD_BASE;
            }
        }
    }

    -1
}

fn abi_open_pfs(name: &[u8], flags: u32) -> i32 {
    if !valid_pfs_path(name) {
        return -1;
    }

    let Some(mut header) = PersistentFs::read_header_silent() else {
        return -1;
    };
    if !pfs_parent_exists(&header, name) {
        return -1;
    }
    let index = match pfs_find_entry(&header, name) {
        Some(index) if pfs_entry_is_dir(&header, index) => return -1,
        Some(index) => index,
        None if flags & FD_CREATE != 0 => {
            let Some(index) = pfs_free_entry(&header) else {
                return -1;
            };
            pfs_set_entry(&mut header, index, name, 0, PFS_KIND_FILE, 0);
            if !PersistentFs::write_header(&header) {
                return -1;
            }
            index
        }
        None => return -1,
    };

    if flags & FD_TRUNCATE != 0 {
        if flags & FD_WRITE == 0 {
            return -1;
        }
        pfs_set_entry(&mut header, index, name, 0, PFS_KIND_FILE, 0);
        if !PersistentFs::write_header(&header) {
            return -1;
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
                    offset: 0,
                    pfs_index: index,
                };
                return fd as i32 + APP_FD_BASE;
            }
        }
    }
    -1
}

extern "sysv64" fn abi_read(fd: i32, buffer_ptr: *mut u8, buffer_len: usize) -> isize {
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
            AppFdKind::Empty => return -1,
        }
        handle.offset += copy_len;
        copy_len as isize
    }
}

extern "sysv64" fn abi_write_fd(fd: i32, buffer_ptr: *const u8, buffer_len: usize) -> isize {
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
        if !handle.open || handle.flags & FD_WRITE == 0 || handle.kind != AppFdKind::Pfs {
            return -1;
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
        if !handle.open || offset > handle.len {
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
        return -1;
    }
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return -1;
    };

    let Some(stat) = stat_path(path) else {
        return -1;
    };
    unsafe {
        stat_ptr.write(stat);
    }
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
        return -1;
    }
    let namespace = if namespace_ptr.is_null() || namespace_len == 0 {
        b"".as_slice()
    } else {
        let Some(namespace) = checked_app_slice(namespace_ptr, namespace_len) else {
            return -1;
        };
        namespace
    };

    if starts_with(namespace, b"pfs:") {
        let Some(header) = PersistentFs::read_header_silent() else {
            return -1;
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
                        });
                    }
                    return copy_len as isize;
                }
                seen += 1;
            }
        }
        return -1;
    }

    let Some((name, stat)) = list_bootfs(index) else {
        return -1;
    };
    let copy_len = min(name_len, name.len());
    unsafe {
        copy_nonoverlapping(name.as_ptr(), name_ptr, copy_len);
        stat_ptr.write(stat);
    }
    copy_len as isize
}

extern "sysv64" fn abi_mkdir(path_ptr: *const u8, path_len: usize) -> i32 {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return -1;
    };
    if !starts_with(path, b"pfs:") {
        return -1;
    }
    let name = &path[4..];
    if !valid_pfs_path(name) {
        return -1;
    }
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return -1;
    };
    if !pfs_parent_exists(&header, name) {
        return -1;
    }
    if pfs_find_entry(&header, name).is_some() {
        return -1;
    }
    let Some(index) = pfs_free_entry(&header) else {
        return -1;
    };
    pfs_set_entry(&mut header, index, name, 0, PFS_KIND_DIR, 0);
    if PersistentFs::write_header(&header) {
        0
    } else {
        -1
    }
}

extern "sysv64" fn abi_env_get(
    key_ptr: *const u8,
    key_len: usize,
    value_ptr: *mut u8,
    value_len: usize,
) -> isize {
    let Some(key) = checked_app_slice(key_ptr, key_len) else {
        return -1;
    };
    for (env_key, env_value) in ENV {
        if env_key == key {
            if value_ptr.is_null() || value_len == 0 {
                return env_value.len() as isize;
            }
            let copy_len = min(value_len, env_value.len());
            unsafe {
                copy_nonoverlapping(env_value.as_ptr(), value_ptr, copy_len);
            }
            return copy_len as isize;
        }
    }
    -1
}

extern "sysv64" fn abi_env_list(
    index: usize,
    key_ptr: *mut u8,
    key_len: usize,
    value_ptr: *mut u8,
    value_len: usize,
) -> isize {
    if index >= ENV_COUNT || key_ptr.is_null() || value_ptr.is_null() {
        return -1;
    }
    let (key, value) = ENV[index];
    let key_copy = min(key_len, key.len());
    let value_copy = min(value_len, value.len());
    unsafe {
        copy_nonoverlapping(key.as_ptr(), key_ptr, key_copy);
        copy_nonoverlapping(value.as_ptr(), value_ptr, value_copy);
    }
    ((key_copy as isize) << 32) | value_copy as isize
}

extern "sysv64" fn abi_spawn(
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const u8,
    args_len: usize,
) -> i32 {
    let Some(name) = checked_app_slice(name_ptr, name_len) else {
        return -1;
    };
    if name.is_empty() || name.len() > PROCESS_NAME_MAX {
        return -1;
    }
    if args_len != 0 {
        let Some(args) = checked_app_slice(args_ptr, args_len) else {
            return -1;
        };
        if args.len() > PROCESS_ARGS_MAX {
            return -1;
        }
    }

    // Programs are still loaded at fixed physical addresses, so a child load
    // would overwrite the caller. Keep the ABI slot stable while the loader
    // grows relocatable or isolated address spaces.
    -2
}

extern "sysv64" fn abi_wait(pid: u32, status_ptr: *mut RymosProcessStatus) -> i32 {
    if status_ptr.is_null() {
        return -1;
    }
    let Some(status) = process_status_by_pid(pid) else {
        return -1;
    };
    unsafe {
        status_ptr.write(status);
    }
    0
}

extern "sysv64" fn abi_mem_alloc_pages(page_count: usize) -> u64 {
    if page_count == 0 || page_count > USER_HEAP_MAX_PAGES_PER_CALL {
        return 0;
    }
    if ensure_kernel_pml4().is_none() {
        return 0;
    }
    let process_index = unsafe { APP_PROCESS_INDEX };
    if !process_can_track_heap_pages(process_index, page_count) {
        return 0;
    }

    let base = unsafe {
        if APP_HEAP_BASE == 0 || APP_HEAP_NEXT == 0 || APP_HEAP_LIMIT == 0 {
            return 0;
        }
        let base = APP_HEAP_NEXT;
        let Some(bytes) = (page_count as u64).checked_mul(PAGE_SIZE) else {
            return 0;
        };
        let Some(next) = APP_HEAP_NEXT.checked_add(bytes) else {
            return 0;
        };
        if next > APP_HEAP_LIMIT {
            return 0;
        }
        APP_HEAP_NEXT = next;
        base
    };

    for index in 0..page_count {
        let virt = base + index as u64 * PAGE_SIZE;
        let Some(phys) = alloc_zeroed_page() else {
            return 0;
        };
        if !map_page(virt, phys, PAGE_PRESENT | PAGE_WRITABLE) {
            return 0;
        }
        if !process_track_heap_page(process_index, phys) {
            return 0;
        }
    }
    base
}

extern "sysv64" fn abi_time_ticks() -> u64 {
    read_tsc()
}

extern "sysv64" fn abi_unlink(path_ptr: *const u8, path_len: usize) -> i32 {
    let Some(path) = checked_app_slice(path_ptr, path_len) else {
        return -1;
    };
    if !starts_with(path, b"pfs:") {
        return -1;
    }
    let name = &path[4..];
    if !valid_pfs_path(name) {
        return -1;
    }
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return -1;
    };
    if pfs_unlink_header(&mut header, name).is_err() {
        return -1;
    }
    if PersistentFs::write_header(&header) {
        0
    } else {
        -1
    }
}

extern "sysv64" fn abi_rename(
    old_ptr: *const u8,
    old_len: usize,
    new_ptr: *const u8,
    new_len: usize,
) -> i32 {
    let Some(old_path) = checked_app_slice(old_ptr, old_len) else {
        return -1;
    };
    let Some(new_path) = checked_app_slice(new_ptr, new_len) else {
        return -1;
    };
    if !starts_with(old_path, b"pfs:") || !starts_with(new_path, b"pfs:") {
        return -1;
    }
    let old_name = &old_path[4..];
    let new_name = &new_path[4..];
    let Some(mut header) = PersistentFs::read_header_silent() else {
        return -1;
    };
    if pfs_rename_header(&mut header, old_name, new_name).is_err() {
        return -1;
    }
    if PersistentFs::write_header(&header) {
        0
    } else {
        -1
    }
}

fn stat_path(path: &[u8]) -> Option<RymosStat> {
    if starts_with(path, b"pfs:") {
        let name = &path[4..];
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
    }
}

fn app_set_heap_window(pid: u32) {
    unsafe {
        let base = USER_HEAP_BASE + pid as u64 * USER_HEAP_STRIDE;
        APP_HEAP_BASE = base;
        APP_HEAP_NEXT = base;
        APP_HEAP_LIMIT = base + USER_HEAP_STRIDE;
        if APP_PROCESS_INDEX < PROCESS_COUNT {
            let table = &raw mut PROCESS_TABLE;
            (*table)[APP_PROCESS_INDEX].heap_base = base;
        }
    }
}

fn app_clear_heap_window() {
    unsafe {
        APP_HEAP_BASE = 0;
        APP_HEAP_NEXT = 0;
        APP_HEAP_LIMIT = 0;
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

fn run_program(console: &mut Console, bootfs: BootFs, name: &[u8], args: &[u8]) {
    let Some(process_index) = process_spawn(name, args) else {
        console.write_line("run: process table full");
        return;
    };

    let mut path = [0u8; 32];
    let path_len = program_path(name, &mut path);
    let Some(image) = bootfs.find_data(&path[..path_len]) else {
        console.write_line("run: program not found");
        process_set_state(process_index, ProcessState::Failed, -1);
        return;
    };

    let Some(entry) = load_program_elf(console, image) else {
        process_set_state(process_index, ProcessState::Failed, -1);
        return;
    };

    console.write("run: ");
    console.write_bytes(&path[..path_len]);
    console.write(" pid ");
    console.write_usize(process_pid(process_index) as usize);
    console.new_line();

    unsafe {
        process_set_state(process_index, ProcessState::Running, 0);
        APP_CONSOLE = console as *mut Console;
        APP_BOOTFS = bootfs;
        APP_ARGS_PTR = args.as_ptr();
        APP_ARGS_LEN = args.len();
        APP_PID = process_pid(process_index);
        APP_PROCESS_INDEX = process_index;
        app_set_heap_window(APP_PID);
        app_close_all_fds();
        app_reset_path_state();
        let program: extern "sysv64" fn(*const RymosAbi) -> i32 = core::mem::transmute(entry);
        let code = program(&RYMOS_ABI);
        app_close_all_fds();
        let reclaimed = process_reclaim_heap_pages(process_index);
        app_clear_heap_window();
        APP_CONSOLE = core::ptr::null_mut();
        APP_BOOTFS = BootFs::empty();
        APP_ARGS_PTR = core::ptr::null();
        APP_ARGS_LEN = 0;
        APP_PID = 0;
        APP_PROCESS_INDEX = PROCESS_COUNT;
        app_reset_path_state();
        process_set_state(process_index, ProcessState::Exited, code);
        if reclaimed > 0 {
            console.write("heap reclaimed ");
            console.write_usize(reclaimed);
            console.write_line(" pages");
        }
        console.write("exit ");
        console.write_i32(code);
        console.new_line();
    }
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

fn load_program_elf(console: &mut Console, elf: &[u8]) -> Option<u64> {
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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        spin_loop();
    }
}
