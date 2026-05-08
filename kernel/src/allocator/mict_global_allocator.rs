use alloc::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU8, Ordering};
use crate::allocator::Locked; // Import the spin::Mutex wrapper from your allocator.rs
use x86_64::instructions::interrupts;

pub const BLOCK_SIZE: usize = 64; // 1 Hardware Cache Line
pub const MAX_HEAP_BLOCKS: usize = 500_000; // Cap at ~6.4MB for the Heatmap array size

pub struct MictGlobalAllocator {
    heap_start: usize,
    heap_size: usize,
    // The Heatmap: 1 bit per BLOCK_SIZE.
    heatmap:[AtomicU8; MAX_HEAP_BLOCKS / 8], 
}

impl MictGlobalAllocator {
    pub const fn new() -> Self {
        // Rust requires a const value to initialize an array of non-Copy types
        const EMPTY: AtomicU8 = AtomicU8::new(0);
        MictGlobalAllocator {
            heap_start: 0,
            heap_size: 0,
            heatmap: [EMPTY; MAX_HEAP_BLOCKS / 8], 
        }
    }

    // Called by init_heap in allocator.rs once the physical pages are mapped
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_size = heap_size;
    }

    // Helper to flip a specific bit atomically
    fn set_bit(&self, bit_idx: usize, hot: bool) {
        let byte_idx = bit_idx / 8;
        let bit_mask = 1 << (bit_idx % 8);
        if hot {
            self.heatmap[byte_idx].fetch_or(bit_mask, Ordering::SeqCst);
        } else {
            self.heatmap[byte_idx].fetch_and(!bit_mask, Ordering::SeqCst);
        }
    }

    // Helper to check if a block is in use
    fn is_hot(&self, bit_idx: usize) -> bool {
        let byte_idx = bit_idx / 8;
        let bit_mask = 1 << (bit_idx % 8);
        (self.heatmap[byte_idx].load(Ordering::SeqCst) & bit_mask) != 0
    }
}

unsafe impl GlobalAlloc for Locked<MictGlobalAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Wrap the ENTIRE allocation process in an interrupt guard
        interrupts::without_interrupts(|| {
            let allocator = self.lock(); // Now safe from mouse/keyboard interrupts!

            let blocks_needed = (layout.size() + BLOCK_SIZE - 1) / BLOCK_SIZE;
            let total_blocks = allocator.heap_size / BLOCK_SIZE;

            let align_blocks = if layout.align() > BLOCK_SIZE {
                layout.align() / BLOCK_SIZE
            } else {
                1
            };

            let mut current_consecutive = 0;
            let mut start_bit_idx = 0;

            for i in 0..total_blocks {
                if current_consecutive == 0 && (i % align_blocks) != 0 {
                    continue;
                }

                if !allocator.is_hot(i) {
                    if current_consecutive == 0 {
                        start_bit_idx = i;
                    }
                    current_consecutive += 1;

                    if current_consecutive == blocks_needed {
                        for j in 0..blocks_needed {
                            allocator.set_bit(start_bit_idx + j, true);
                        }
                        let ptr_offset = start_bit_idx * BLOCK_SIZE;
                        return (allocator.heap_start + ptr_offset) as *mut u8;
                    }
                } else {
                    current_consecutive = 0;
                }
            }
            core::ptr::null_mut()
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Wrap the deallocation in an interrupt guard as well
        interrupts::without_interrupts(|| {
            let allocator = self.lock();
            let addr = ptr as usize;

            if addr < allocator.heap_start || addr >= allocator.heap_start + allocator.heap_size {
                panic!("MictGlobalAllocator: Dissonance! Dealloc outside of heap bounds.");
            }

            let offset = addr - allocator.heap_start;
            let start_bit_idx = offset / BLOCK_SIZE;
            let blocks_to_free = (layout.size() + BLOCK_SIZE - 1) / BLOCK_SIZE;

            for i in 0..blocks_to_free {
                allocator.set_bit(start_bit_idx + i, false);
            }
        })
    }
}