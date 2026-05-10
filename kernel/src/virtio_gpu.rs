// kernel/src/virtio_gpu.rs

use core::sync::atomic::{AtomicU32, Ordering};
use alloc::vec::Vec;
use spin::Mutex;

pub static VIRTIO_GPU: Mutex<Option<VirtioGpuDriver>> = Mutex::new(None);

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u32)]
pub enum CommandTy {
    Undefined = 0,
    GetDisplayInfo = 0x0100,
    ResourceCreate2d,
    ResourceUnref,
    SetScanout,
    ResourceFlush,
    TransferToHost2d,
    ResourceAttachBacking,
    ResourceDetachBacking,
    
    // Responses
    RespOkNodata = 0x1100,
    RespOkDisplayInfo,
}

#[derive(Debug)]
#[repr(C)]
pub struct ControlHeader {
    pub ty: CommandTy,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub ring_index: u8,
    padding: [u8; 3],
}

impl ControlHeader {
    pub fn with_ty(ty: CommandTy) -> Self {
        Self { ty, flags: 0, fence_id: 0, ctx_id: 0, ring_index: 0, padding: [0; 3] }
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct GpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum ResourceFormat {
    Bgrx = 2,
    Xrgb = 4,
}

#[derive(Debug)]
#[repr(C)]
pub struct ResourceCreate2d {
    pub header: ControlHeader,
    pub resource_id: u32,
    pub format: ResourceFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct AttachBacking {
    pub header: ControlHeader,
    pub resource_id: u32,
    pub num_entries: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct MemEntry {
    pub address: u64,
    pub length: u32,
    pub padding: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct SetScanout {
    pub header: ControlHeader,
    pub rect: GpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct TransferToHost2d {
    pub header: ControlHeader,
    pub rect: GpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct ResourceFlush {
    pub header: ControlHeader,
    pub rect: GpuRect,
    pub resource_id: u32,
    pub padding: u32,
}

// --- THE POLYMORPH OS DRIVER ---

pub struct VirtioGpuDriver {
    mmio_base: usize,
    resource_id_counter: AtomicU32,
}

impl VirtioGpuDriver {
    pub unsafe fn new(mmio_base: usize) -> Self {
        crate::serial_println!("[VIRTIO-GPU] Initializing at {:#X}", mmio_base);
        
        // In the full implementation, we will initialize the Virtqueues here
        // and negotiate features with the hypervisor.
        
        VirtioGpuDriver {
            mmio_base,
            resource_id_counter: AtomicU32::new(1),
        }
    }
    
    // Helper to generate unique IDs for our canvases
    pub fn next_resource_id(&self) -> u32 {
        self.resource_id_counter.fetch_add(1, Ordering::SeqCst)
    }
}