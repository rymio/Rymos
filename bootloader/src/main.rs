#![no_main]
#![no_std]

use core::ffi::c_void;
use core::hint::spin_loop;
use core::mem::size_of;
use core::panic::PanicInfo;
use core::ptr::copy_nonoverlapping;

type EfiHandle = *mut c_void;
type EfiStatus = usize;
type Char16 = u16;

const EFI_SUCCESS: EfiStatus = 0;
const EFI_LOAD_ERROR: EfiStatus = 1;
const EFI_NOT_FOUND: EfiStatus = 14;
const EFI_FILE_MODE_READ: u64 = 0x1;
const EFI_ALLOCATE_ADDRESS: usize = 0;
const EFI_LOADER_DATA: usize = 2;
const EFI_RUNTIME_SERVICES_DATA: usize = 6;
const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
const PT_LOAD: u32 = 1;
const KERNEL_READ_LIMIT: usize = 1024 * 1024;
// UEFI's File.Read() silently reads at most this many bytes with no error if
// the file is larger -- there is no size check before the read, so this must
// stay ahead of INITRD.RFS's real size or bootfs entries get silently
// truncated away. Real (unstripped, debug) `std` binaries are much larger
// than `no_std` ones (single digit MiB each), so this has real headroom.
const INITRD_READ_LIMIT: usize = 32 * 1024 * 1024;
const KERNEL_PATH: [Char16; 12] = [
    '\\' as Char16,
    'K' as Char16,
    'E' as Char16,
    'R' as Char16,
    'N' as Char16,
    'E' as Char16,
    'L' as Char16,
    '.' as Char16,
    'E' as Char16,
    'L' as Char16,
    'F' as Char16,
    0,
];
const INITRD_PATH: [Char16; 12] = [
    '\\' as Char16,
    'I' as Char16,
    'N' as Char16,
    'I' as Char16,
    'T' as Char16,
    'R' as Char16,
    'D' as Char16,
    '.' as Char16,
    'R' as Char16,
    'F' as Char16,
    'S' as Char16,
    0,
];

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const LOADED_IMAGE_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x5B1B31A1,
    data2: 0x9562,
    data3: 0x11D2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

const SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x0964E5B22,
    data2: 0x6459,
    data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

const GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x9042A9DE,
    data2: 0x23DC,
    data3: 0x4A38,
    data4: [0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A],
};

#[repr(C)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
struct EfiSimpleTextOutputProtocol {
    reset: usize,
    output_string: extern "efiapi" fn(
        this: *mut EfiSimpleTextOutputProtocol,
        string: *const Char16,
    ) -> EfiStatus,
    test_string: usize,
    query_mode: usize,
    set_mode: usize,
    set_attribute: usize,
    clear_screen: extern "efiapi" fn(this: *mut EfiSimpleTextOutputProtocol) -> EfiStatus,
    set_cursor_position: usize,
    enable_cursor: usize,
    mode: usize,
}

#[repr(C)]
struct EfiSystemTable {
    header: EfiTableHeader,
    firmware_vendor: *mut Char16,
    firmware_revision: u32,
    console_in_handle: EfiHandle,
    con_in: *mut c_void,
    console_out_handle: EfiHandle,
    con_out: *mut EfiSimpleTextOutputProtocol,
    standard_error_handle: EfiHandle,
    std_err: *mut EfiSimpleTextOutputProtocol,
    runtime_services: *mut c_void,
    boot_services: *mut EfiBootServices,
    number_of_table_entries: usize,
    configuration_table: *mut c_void,
}

#[repr(C)]
struct EfiBootServices {
    header: EfiTableHeader,
    raise_tpl: usize,
    restore_tpl: usize,
    allocate_pages: extern "efiapi" fn(usize, usize, usize, *mut u64) -> EfiStatus,
    free_pages: usize,
    get_memory_map: extern "efiapi" fn(
        *mut usize,
        *mut EfiMemoryDescriptor,
        *mut usize,
        *mut usize,
        *mut u32,
    ) -> EfiStatus,
    allocate_pool: extern "efiapi" fn(usize, usize, *mut *mut c_void) -> EfiStatus,
    free_pool: extern "efiapi" fn(*mut c_void) -> EfiStatus,
    create_event: usize,
    set_timer: usize,
    wait_for_event: usize,
    signal_event: usize,
    close_event: usize,
    check_event: usize,
    install_protocol_interface: usize,
    reinstall_protocol_interface: usize,
    uninstall_protocol_interface: usize,
    handle_protocol: extern "efiapi" fn(EfiHandle, *const EfiGuid, *mut *mut c_void) -> EfiStatus,
    reserved: usize,
    register_protocol_notify: usize,
    locate_handle: usize,
    locate_device_path: usize,
    install_configuration_table: usize,
    load_image: usize,
    start_image: usize,
    exit: usize,
    unload_image: usize,
    exit_boot_services: extern "efiapi" fn(EfiHandle, usize) -> EfiStatus,
    get_next_monotonic_count: usize,
    stall: usize,
    set_watchdog_timer: usize,
    connect_controller: usize,
    disconnect_controller: usize,
    open_protocol: usize,
    close_protocol: usize,
    open_protocol_information: usize,
    protocols_per_handle: usize,
    locate_handle_buffer: usize,
    locate_protocol: extern "efiapi" fn(*const EfiGuid, *mut c_void, *mut *mut c_void) -> EfiStatus,
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
struct EfiLoadedImageProtocol {
    revision: u32,
    parent_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
    device_handle: EfiHandle,
    file_path: *mut c_void,
    reserved: *mut c_void,
    load_options_size: u32,
    load_options: *mut c_void,
    image_base: *mut c_void,
    image_size: u64,
    image_code_type: u32,
    image_data_type: u32,
    unload: usize,
}

#[repr(C)]
struct EfiSimpleFileSystemProtocol {
    revision: u64,
    open_volume: extern "efiapi" fn(
        *mut EfiSimpleFileSystemProtocol,
        *mut *mut EfiFileProtocol,
    ) -> EfiStatus,
}

#[repr(C)]
struct EfiFileProtocol {
    revision: u64,
    open: extern "efiapi" fn(
        *mut EfiFileProtocol,
        *mut *mut EfiFileProtocol,
        *const Char16,
        u64,
        u64,
    ) -> EfiStatus,
    close: extern "efiapi" fn(*mut EfiFileProtocol) -> EfiStatus,
    delete: usize,
    read: extern "efiapi" fn(*mut EfiFileProtocol, *mut usize, *mut c_void) -> EfiStatus,
    write: usize,
    get_position: usize,
    set_position: usize,
    get_info: usize,
    set_info: usize,
    flush: usize,
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

#[repr(C)]
#[derive(Clone, Copy)]
struct BootInfo {
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
struct EfiGraphicsOutputProtocol {
    query_mode: extern "efiapi" fn(
        *mut EfiGraphicsOutputProtocol,
        u32,
        *mut usize,
        *mut *mut EfiGraphicsOutputModeInformation,
    ) -> EfiStatus,
    set_mode: extern "efiapi" fn(*mut EfiGraphicsOutputProtocol, u32) -> EfiStatus,
    blt: usize,
    mode: *mut EfiGraphicsOutputProtocolMode,
}

#[repr(C)]
struct EfiGraphicsOutputProtocolMode {
    max_mode: u32,
    mode: u32,
    info: *mut EfiGraphicsOutputModeInformation,
    size_of_info: usize,
    framebuffer_base: u64,
    framebuffer_size: usize,
}

#[repr(C)]
struct EfiGraphicsOutputModeInformation {
    version: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    pixel_format: u32,
    pixel_information: [u8; 16],
    pixels_per_scan_line: u32,
}

static mut BOOT_INFO: BootInfo = BootInfo {
    framebuffer_base: 0,
    framebuffer_size: 0,
    horizontal_resolution: 0,
    vertical_resolution: 0,
    pixels_per_scan_line: 0,
    pixel_format: 0,
    initrd_base: 0,
    initrd_size: 0,
    memory_map_base: 0,
    memory_map_size: 0,
    memory_descriptor_size: 0,
};

static BOOT_MESSAGE: [Char16; 59] =
    ascii_to_uefi("RYMOS bootloader: loading KERNEL.ELF from FAT32...\r\n");
static JUMP_MESSAGE: [Char16; 39] = ascii_to_uefi("RYMOS bootloader: jumping to kernel\r\n");
static ERROR_MESSAGE: [Char16; 34] = ascii_to_uefi("RYMOS bootloader: load failed\r\n");

const SERIAL_BOOT_MESSAGE: &[u8] = b"RYMOS bootloader: loading KERNEL.ELF from FAT32...\r\n";
const SERIAL_JUMP_MESSAGE: &[u8] = b"RYMOS bootloader: jumping to kernel\r\n";
const SERIAL_ERROR_MESSAGE: &[u8] = b"RYMOS bootloader: load failed\r\n";
const COM1: u16 = 0x3F8;

#[unsafe(no_mangle)]
extern "efiapi" fn efi_main(image: EfiHandle, system_table: *mut EfiSystemTable) -> EfiStatus {
    unsafe {
        serial_init();
        serial_write(SERIAL_BOOT_MESSAGE);
    }

    let result = unsafe { boot_kernel(image, system_table) };
    if result != EFI_SUCCESS {
        unsafe {
            serial_write(SERIAL_ERROR_MESSAGE);
            write_uefi(system_table, ERROR_MESSAGE.as_ptr());
        }
    }

    loop {
        spin_loop();
    }
}

unsafe fn boot_kernel(image: EfiHandle, system_table: *mut EfiSystemTable) -> EfiStatus {
    if system_table.is_null() {
        return EFI_LOAD_ERROR;
    }

    unsafe {
        write_uefi(system_table, BOOT_MESSAGE.as_ptr());
    }

    let boot_services = unsafe { (*system_table).boot_services };
    if boot_services.is_null() {
        return EFI_LOAD_ERROR;
    }

    let kernel = unsafe {
        read_file(
            image,
            boot_services,
            KERNEL_PATH.as_ptr(),
            KERNEL_READ_LIMIT,
        )
    };
    if kernel.status != EFI_SUCCESS {
        return kernel.status;
    }

    let entry = unsafe { load_elf(boot_services, kernel.ptr.cast::<u8>(), kernel.size) };
    if entry == 0 {
        return EFI_LOAD_ERROR;
    }

    unsafe {
        set_graphics_mode(boot_services, 1024, 768);
        BOOT_INFO = framebuffer_info(boot_services);
        let initrd = read_file(
            image,
            boot_services,
            INITRD_PATH.as_ptr(),
            INITRD_READ_LIMIT,
        );
        if initrd.status == EFI_SUCCESS {
            BOOT_INFO.initrd_base = initrd.ptr as u64;
            BOOT_INFO.initrd_size = initrd.size;
        }
    }

    unsafe {
        write_uefi(system_table, JUMP_MESSAGE.as_ptr());
        serial_write(SERIAL_JUMP_MESSAGE);
    }

    let status = unsafe { exit_boot_services(image, boot_services) };
    if status != EFI_SUCCESS {
        return status;
    }

    let kernel_entry: extern "sysv64" fn(*const BootInfo) -> ! =
        unsafe { core::mem::transmute(entry) };
    kernel_entry(&raw const BOOT_INFO);
}

struct FileBuffer {
    status: EfiStatus,
    ptr: *mut c_void,
    size: usize,
}

unsafe fn read_file(
    image: EfiHandle,
    boot_services: *mut EfiBootServices,
    path: *const Char16,
    read_limit: usize,
) -> FileBuffer {
    let mut loaded_image = core::ptr::null_mut();
    let mut fs = core::ptr::null_mut();
    let mut root = core::ptr::null_mut();
    let mut file = core::ptr::null_mut();
    let mut buffer = core::ptr::null_mut();

    let status = unsafe {
        ((*boot_services).handle_protocol)(image, &LOADED_IMAGE_PROTOCOL_GUID, &mut loaded_image)
    };
    if status != EFI_SUCCESS {
        return FileBuffer {
            status,
            ptr: core::ptr::null_mut(),
            size: 0,
        };
    }

    let device = unsafe { (*(loaded_image as *mut EfiLoadedImageProtocol)).device_handle };
    let status = unsafe {
        ((*boot_services).handle_protocol)(device, &SIMPLE_FILE_SYSTEM_PROTOCOL_GUID, &mut fs)
    };
    if status != EFI_SUCCESS {
        return FileBuffer {
            status,
            ptr: core::ptr::null_mut(),
            size: 0,
        };
    }

    let status =
        unsafe { ((*(fs as *mut EfiSimpleFileSystemProtocol)).open_volume)(fs.cast(), &mut root) };
    if status != EFI_SUCCESS {
        return FileBuffer {
            status,
            ptr: core::ptr::null_mut(),
            size: 0,
        };
    }

    let status = unsafe { ((*root).open)(root, &mut file, path, EFI_FILE_MODE_READ, 0) };
    if status != EFI_SUCCESS {
        return FileBuffer {
            status,
            ptr: core::ptr::null_mut(),
            size: 0,
        };
    }

    let status =
        unsafe { ((*boot_services).allocate_pool)(EFI_LOADER_DATA, read_limit, &mut buffer) };
    if status != EFI_SUCCESS {
        return FileBuffer {
            status,
            ptr: core::ptr::null_mut(),
            size: 0,
        };
    }

    let mut size = read_limit;
    let status = unsafe { ((*file).read)(file, &mut size, buffer) };
    unsafe {
        ((*file).close)(file);
        ((*root).close)(root);
    }

    FileBuffer {
        status: if size == 0 { EFI_NOT_FOUND } else { status },
        ptr: buffer,
        size,
    }
}

unsafe fn load_elf(boot_services: *mut EfiBootServices, elf: *const u8, size: usize) -> u64 {
    if size < size_of::<Elf64Header>() {
        return 0;
    }

    let header = unsafe { &*(elf.cast::<Elf64Header>()) };
    if &header.ident[0..4] != ELF_MAGIC || header.ident[4] != 2 || header.machine != 0x3E {
        return 0;
    }

    let phoff = header.phoff as usize;
    let phentsize = header.phentsize as usize;
    let phnum = header.phnum as usize;
    if phoff + phentsize * phnum > size || phentsize < size_of::<Elf64ProgramHeader>() {
        return 0;
    }

    for index in 0..phnum {
        let ph_ptr = unsafe {
            elf.add(phoff + index * phentsize)
                .cast::<Elf64ProgramHeader>()
        };
        let ph = unsafe { &*ph_ptr };
        if ph.typ != PT_LOAD || ph.memsz == 0 {
            continue;
        }
        if ph.offset as usize + ph.filesz as usize > size || ph.filesz > ph.memsz {
            return 0;
        }

        let start = ph.paddr;
        let pages = ph.memsz.div_ceil(4096);
        let mut address = start;
        let status = unsafe {
            ((*boot_services).allocate_pages)(
                EFI_ALLOCATE_ADDRESS,
                EFI_LOADER_DATA,
                pages as usize,
                &mut address,
            )
        };
        if status != EFI_SUCCESS {
            return 0;
        }

        let destination = start as *mut u8;
        unsafe {
            zero_bytes(destination, ph.memsz as usize);
            copy_nonoverlapping(elf.add(ph.offset as usize), destination, ph.filesz as usize);
        }
    }

    header.entry
}

unsafe fn exit_boot_services(image: EfiHandle, boot_services: *mut EfiBootServices) -> EfiStatus {
    let mut memory_map_size = 0usize;
    let mut map_key = 0usize;
    let mut descriptor_size = 0usize;
    let mut descriptor_version = 0u32;

    unsafe {
        ((*boot_services).get_memory_map)(
            &mut memory_map_size,
            core::ptr::null_mut(),
            &mut map_key,
            &mut descriptor_size,
            &mut descriptor_version,
        );
    }

    memory_map_size += descriptor_size * 8;
    let mut memory_map = core::ptr::null_mut();
    let status = unsafe {
        ((*boot_services).allocate_pool)(
            EFI_RUNTIME_SERVICES_DATA,
            memory_map_size,
            &mut memory_map,
        )
    };
    if status != EFI_SUCCESS {
        return status;
    }

    let status = unsafe {
        ((*boot_services).get_memory_map)(
            &mut memory_map_size,
            memory_map.cast(),
            &mut map_key,
            &mut descriptor_size,
            &mut descriptor_version,
        )
    };
    if status != EFI_SUCCESS {
        return status;
    }

    unsafe {
        BOOT_INFO.memory_map_base = memory_map as u64;
        BOOT_INFO.memory_map_size = memory_map_size;
        BOOT_INFO.memory_descriptor_size = descriptor_size;
    }

    unsafe { ((*boot_services).exit_boot_services)(image, map_key) }
}

unsafe fn framebuffer_info(boot_services: *mut EfiBootServices) -> BootInfo {
    let Some(gop) = (unsafe { locate_gop(boot_services) }) else {
        return empty_boot_info();
    };

    let mode = unsafe { (*gop).mode };
    if mode.is_null() {
        return empty_boot_info();
    }

    let info = unsafe { (*mode).info };
    if info.is_null() {
        return empty_boot_info();
    }

    BootInfo {
        framebuffer_base: unsafe { (*mode).framebuffer_base },
        framebuffer_size: unsafe { (*mode).framebuffer_size },
        horizontal_resolution: unsafe { (*info).horizontal_resolution as usize },
        vertical_resolution: unsafe { (*info).vertical_resolution as usize },
        pixels_per_scan_line: unsafe { (*info).pixels_per_scan_line as usize },
        pixel_format: unsafe { (*info).pixel_format },
        initrd_base: 0,
        initrd_size: 0,
        memory_map_base: 0,
        memory_map_size: 0,
        memory_descriptor_size: 0,
    }
}

unsafe fn set_graphics_mode(boot_services: *mut EfiBootServices, width: u32, height: u32) {
    let Some(gop) = (unsafe { locate_gop(boot_services) }) else {
        return;
    };
    let mode = unsafe { (*gop).mode };
    if mode.is_null() {
        return;
    }

    let max_mode = unsafe { (*mode).max_mode };
    for mode_number in 0..max_mode {
        let mut info_size = 0usize;
        let mut info = core::ptr::null_mut();
        let status = unsafe { ((*gop).query_mode)(gop, mode_number, &mut info_size, &mut info) };
        if status != EFI_SUCCESS || info.is_null() {
            continue;
        }
        let mode_info = unsafe { &*info };
        if mode_info.horizontal_resolution == width && mode_info.vertical_resolution == height {
            unsafe {
                ((*gop).set_mode)(gop, mode_number);
            }
            return;
        }
    }
}

unsafe fn locate_gop(
    boot_services: *mut EfiBootServices,
) -> Option<*mut EfiGraphicsOutputProtocol> {
    let mut gop = core::ptr::null_mut();
    let status = unsafe {
        ((*boot_services).locate_protocol)(
            &GRAPHICS_OUTPUT_PROTOCOL_GUID,
            core::ptr::null_mut(),
            &mut gop,
        )
    };
    if status == EFI_SUCCESS && !gop.is_null() {
        Some(gop.cast())
    } else {
        None
    }
}

const fn empty_boot_info() -> BootInfo {
    BootInfo {
        framebuffer_base: 0,
        framebuffer_size: 0,
        horizontal_resolution: 0,
        vertical_resolution: 0,
        pixels_per_scan_line: 0,
        pixel_format: 0,
        initrd_base: 0,
        initrd_size: 0,
        memory_map_base: 0,
        memory_map_size: 0,
        memory_descriptor_size: 0,
    }
}

unsafe fn zero_bytes(destination: *mut u8, length: usize) {
    for index in 0..length {
        unsafe {
            destination.add(index).write_volatile(0);
        }
    }
}

unsafe fn write_uefi(system_table: *mut EfiSystemTable, message: *const Char16) {
    if system_table.is_null() {
        return;
    }

    let stdout = unsafe { (*system_table).con_out };
    if stdout.is_null() {
        return;
    }

    unsafe {
        ((*stdout).output_string)(stdout, message);
    }
}

const fn ascii_to_uefi<const N: usize>(input: &str) -> [Char16; N] {
    let bytes = input.as_bytes();
    let mut output = [0; N];
    let mut index = 0;

    while index < bytes.len() {
        output[index] = bytes[index] as Char16;
        index += 1;
    }

    output
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

unsafe fn serial_write(bytes: &[u8]) {
    for byte in bytes {
        unsafe {
            outb(COM1, *byte);
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        spin_loop();
    }
}
