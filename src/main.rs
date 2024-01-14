#![no_main]
#![no_std]

use core::mem::{size_of, MaybeUninit};
use core::panic::PanicInfo;
use core::ptr::slice_from_raw_parts;
use core::slice::from_raw_parts_mut;
use uefi::prelude::*;
use uefi::table::boot::{AllocateType, MemoryDescriptor, MemoryType, PAGE_SIZE};
use x86_64::structures::paging::mapper::PageTableFrameMapping;
use x86_64::structures::paging::page_table::PageTableEntry;
use x86_64::structures::paging::{
    FrameAllocator, MappedPageTable, Mapper, Page, PageTable, PageTableFlags, PhysFrame, Size1GiB,
    Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[entry]
fn main(_image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    system_table.boot_services().stall(10_000_000);
    Status::SUCCESS
}

fn map_memory(st: &SystemTable<Boot>) {
    struct AddressMapper(MemoryDescriptor);
    unsafe impl PageTableFrameMapping for AddressMapper {
        fn frame_to_pointer(&self, frame: PhysFrame) -> *mut PageTable {
            let fs = frame.start_address().as_u64();
            let rs = self.0.phys_start;
            assert!(rs <= fs && fs < rs + self.0.page_count * PAGE_SIZE);
            (self.0.virt_start + (fs - rs)) as *mut PageTable
        }
    }
    struct BumpFrameAllocator {
        lowest_address: u64,
        remaining: u64,
    }
    unsafe impl FrameAllocator<Size4KiB> for BumpFrameAllocator {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            if self.remaining > 0 {
                self.remaining -= 1;
                Some(PhysFrame::containing_address(
                    (self.lowest_address + self.remaining * PAGE_SIZE).into(),
                ))
            } else {
                None
            }
        }
    }

    let bs = st.boot_services();
    let frame_count = 1024;
    let frame_buffer_p = bs
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 520)
        .unwrap();
    unsafe {
        let mms = bs.memory_map_size();
        let size = mms.map_size + 2 * mms.entry_size;
        let mem_map_space = bs.allocate_pool(MemoryType::LOADER_DATA, size).unwrap();
        let mut mem_map = bs
            .memory_map(from_raw_parts_mut(mem_map_space, size))
            .unwrap();
        let tmp_region = *mem_map
            .entries()
            .find(|x| {
                x.phys_start <= frame_buffer_p
                    && frame_buffer_p + frame_count * PAGE_SIZE
                        <= x.phys_start + x.page_count * PAGE_SIZE
            })
            .unwrap();
        let frame_buffer_v = (frame_buffer_p - tmp_region.phys_start) + tmp_region.virt_start;
        assert_eq!(size_of::<PageTable>(), PAGE_SIZE);
        let tmp_storage = frame_buffer_v as *mut MaybeUninit<PageTable>;
        let last_physical = mem_map.entries().max_by_key(|x| x.phys_start).unwrap();
        let phys_mem_end = last_physical.phys_start + last_physical.page_count * PAGE_SIZE;
        let last_virtual = mem_map.entries().max_by_key(|x| x.virt_start).unwrap();
        let max_used_virt = last_virtual.virt_start + last_virtual.page_count * PAGE_SIZE;

        let mut frame_allocator = BumpFrameAllocator {
            lowest_address: frame_buffer_p + PAGE_SIZE,
            remaining: frame_count as u64 - 1,
        };
        let l4 = tmp_storage as *mut PageTable;
        l4.write(PageTable::new());
        let mut table = unsafe { MappedPageTable::new(&mut *l4, AddressMapper(tmp_region)) };
        let physical_mem_offset = VirtAddr::new(max_used_virt.next_multiple_of(512 << 30));
        let physical_gigabytes = phys_mem_end.next_multiple_of(1 << 30) >> 30;
        for g in 0..physical_gigabytes {
            table
                .map_to(
                    Page::<Size1GiB>::containing_address((physical_mem_offset + g << 30).into()),
                    PhysFrame::containing_address(PhysAddr::new(g << 30)),
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::USER_ACCESSIBLE
                        | PageTableFlags::NO_EXECUTE,
                    &mut frame_allocator,
                )
                .unwrap()
                .ignore();
        }
        for region in mem_map.entries() {
            let flags = match region.ty {
                MemoryType::BOOT_SERVICES_CODE
                | MemoryType::BOOT_SERVICES_DATA
                | MemoryType::LOADER_CODE
                | MemoryType::LOADER_DATA
                | MemoryType::CONVENTIONAL
                | MemoryType::CONVENTIONAL
                | MemoryType::ACPI_RECLAIM => continue,
                //=>PageTableFlags::empty(),
                MemoryType::RUNTIME_SERVICES_CODE => PageTableFlags::empty(),
                MemoryType::RUNTIME_SERVICES_DATA => {
                    PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE
                }
            } | PageTableFlags::PRESENT
                | PageTableFlags::USER_ACCESSIBLE;
            for i in 0..region.page_count {}
        }

        drop(mem_map);
        bs.free_pool(mem_map_space).unwrap();
    }
}
