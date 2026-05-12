use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

// Removed the FixedSizeBlockAllocator import, added MICT
use crate::allocator::mict_global_allocator::MictGlobalAllocator; 

use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, mapper::MapToError,
    },
};

//pub mod bump;
//pub mod fixed_size_block;
//pub mod linked_list;
pub mod mict_global_allocator; // Your custom hardware architecture

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 64 * 1024 * 1024; // 10 MiB

// The Kernel Global Allocator is now powered by MICT-Elastic logic.
#[global_allocator]
static ALLOCATOR: Locked<MictGlobalAllocator> = Locked::new(MictGlobalAllocator::new());

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // 1. Physically map the hardware RAM frames to the Virtual Heap Address
    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    // 2. Initialize the MICT Heatmap tracker with the newly mapped RAM
    
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    

    Ok(())
}

pub struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("dealloc should be never called")
    }
}

/// A wrapper around spin::Mutex to permit trait implementations.
/// NOTE: In the MICT architecture, this lock is primarily used to secure the `init` phase.
/// The `alloc` and `dealloc` fast-paths can bypass this lock by utilizing internal atomics.
pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> spin::MutexGuard<'_, A> {
        self.inner.lock()
    }
}

