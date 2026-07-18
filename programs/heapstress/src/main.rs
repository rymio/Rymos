#![no_main]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use rymos_user as rt;

const PAGE_SIZE: usize = 4096;

#[unsafe(no_mangle)]
fn rymos_main() -> i32 {
    rt::println("heapstress: memory smoke test");
    rt::print("pid: ");
    rt::print_usize(rt::pid() as usize);
    rt::write(b"\n");

    let pages = 2048usize;
    let Some(address) = rt::mem_map_pages(pages, rt::MEM_MAP_GUARD) else {
        rt::println("heapstress: mem_map_pages failed");
        return 1;
    };
    rt::print("heapstress mapped ");
    rt::print_usize(pages * PAGE_SIZE / 1024 / 1024);
    rt::print(" MiB at ");
    rt::print_hex_usize(address);
    rt::write(b"\n");

    unsafe {
        for page in 0..pages {
            let ptr = (address + page * PAGE_SIZE) as *mut u64;
            ptr.write_volatile(0xA110_C000_0000_0000u64 | page as u64);
        }
        let first = (address as *const u64).read_volatile();
        let last = ((address + (pages - 1) * PAGE_SIZE) as *const u64).read_volatile();
        if first != 0xA110_C000_0000_0000u64
            || last != 0xA110_C000_0000_0000u64 | (pages - 1) as u64
        {
            rt::println("heapstress: mmap readback failed");
            return 1;
        }
    }

    if !rt::mem_unmap_pages(address, pages) {
        rt::println("heapstress: mem_unmap_pages failed");
        return 1;
    }
    rt::println("heapstress unmapped guarded region");

    let mut data = Vec::new();
    for index in 0..(2 * 1024 * 1024usize) {
        data.push((index & 0xFF) as u8);
    }
    let checksum = data[0] as usize + data[data.len() - 1] as usize + data.len();
    rt::print("heapstress vec ");
    rt::print_usize(data.len());
    rt::print(" checksum ");
    rt::print_usize(checksum);
    rt::write(b"\n");

    rt::println("heapstress: ok");
    0
}
