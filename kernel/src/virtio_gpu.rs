// kernel/src/virtio_gpu.rs

use core::sync::atomic::{AtomicU32, Ordering};
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::{VirtAddr, PhysAddr};
use x86_64::structures::paging::{FrameAllocator, Size4KiB};


pub static VIRTIO_GPU: Mutex<Option<VirtioGpuDriver>> = Mutex::new(None);
pub const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

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
#[repr(C)]
pub struct DisplayInfo {
    pub rect: GpuRect,
    pub enabled: u32,
    pub flags: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct GetDisplayInfo {
    pub header: ControlHeader,
    pub display_info:[DisplayInfo; VIRTIO_GPU_MAX_SCANOUTS],
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
    resource_id_counter: core::sync::atomic::AtomicU32,
    control_queue_virt: Option<u64>, 
    notify_address: Option<u64>, 
    
    // --- NEW: The DMA Mailbox ---
    mailbox_phys: Option<u64>,
    mailbox_virt: Option<u64>,
}

impl VirtioGpuDriver {
    pub unsafe fn new(mmio_base: usize) -> Self {
        crate::serial_println!("[VIRTIO-GPU] Initializing at {:#X}", mmio_base);
        VirtioGpuDriver {
            mmio_base,
            resource_id_counter: AtomicU32::new(1),
            control_queue_virt: None,
            notify_address: None,
            mailbox_phys: None,
            mailbox_virt: None
        }
    }

    /// [MICT: QUEUE FORGING]
    /// Allocates physical memory for the Control Queue (Queue 0).
    pub unsafe fn setup_control_queue(
        &mut self,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<x86_64::structures::paging::Size4KiB>,
        phys_mem_offset: x86_64::VirtAddr,
    ) -> Result<(u64, u64, u64), &'static str> {
        crate::serial_println!("[VIRTIO-GPU] Forging Control Queue...");

        if let Some((phys_addr, virt_addr)) = crate::memory::allocate_dma_frames(frame_allocator, phys_mem_offset, 3) {
            let vq_ptr = virt_addr.as_mut_ptr::<crate::virtqueue::VirtQueue>();
            *vq_ptr = crate::virtqueue::VirtQueue::new();
            
            // --- [MICT: THE STRUCT ALIGNMENT FIX] ---
            // Calculate the EXACT physical addresses of the 3 rings inside the struct!
            let vq = &*vq_ptr;
            let base_virt = virt_addr.as_u64();
            let base_phys = phys_addr.as_u64();

            // Calculate offset of each field from the start of the struct, add to physical base
            let desc_phys = base_phys + (core::ptr::addr_of!(vq.descriptors) as u64 - base_virt);
            let avail_phys = base_phys + (core::ptr::addr_of!(vq.available) as u64 - base_virt);
            let used_phys = base_phys + (core::ptr::addr_of!(vq.used) as u64 - base_virt);

            self.control_queue_virt = Some(base_virt);
            crate::serial_println!("  -> Control Queue allocated at Phys: {:#X}", base_phys);
            
            // Allocate the Mailbox (1 page)
            if let Some((m_phys, m_virt)) = crate::memory::allocate_dma_frames(frame_allocator, phys_mem_offset, 1) {
                self.mailbox_phys = Some(m_phys.as_u64());
                self.mailbox_virt = Some(m_virt.as_u64());
            } else {
                return Err("Failed to allocate DMA Mailbox.");
            }
            
            Ok((desc_phys, avail_phys, used_phys))
        } else {
            Err("Failed to allocate DMA memory for Control Queue.")
        }
    }

    /// [MICT: THE DOORBELL]
    /// Links the driver to the specific hardware notify register found by the PCI walker.
    pub fn set_notify_address(&mut self, addr: u64) {
        self.notify_address = Some(addr);
    }

    ///[MICT: DMA DISPATCHER]
    /// Pushes a raw byte payload (like a ResourceCreate2d struct) into the Virtqueue
    /// and rings the hardware doorbell.
    pub unsafe fn send_command(&mut self, command_bytes: &[u8]) -> Result<(), &'static str> {
        let vq_virt = self.control_queue_virt.expect("[FATAL] Control Queue not initialized!");
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        // 1. Find the next free descriptor slot
        if vq.num_free == 0 {
            return Err("Control Queue is full!");
        }
        
        let head_idx = vq.free_head as usize;
        let desc = &mut vq.descriptors[head_idx];
        
        // Remove it from the free list
        vq.free_head = desc.next;
        vq.num_free -= 1;

        // 2. We need a physical memory address to hold the actual command_bytes!
        // (In a full implementation, you allocate a small DMA buffer here, copy the bytes into it, 
        // and put the physical address of that buffer into `desc.addr`).
        
        // For demonstration, let's assume we allocated a buffer at physical address `0x2000000`
        let command_buffer_phys = 0x2000000; // MUST BE REPLACED WITH REAL ALLOCATION
        
        // Copy the bytes to our hypothetical buffer (requires virt mapping in reality)
        // core::ptr::copy_nonoverlapping(command_bytes.as_ptr(), mapped_virt_addr, command_bytes.len());

        desc.addr = command_buffer_phys;
        desc.len = command_bytes.len() as u32;
        desc.flags = 0; // Device only needs to read this descriptor

        // 3. Put our descriptor index into the Available Ring
        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 256] = head_idx as u16;

        // [FIX]: Upgrade to a full hardware memory barrier!
        // This forces the CPU to flush the cache to RAM so the GPU can see it.
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        
        vq.available.idx.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        // 5. RING THE DOORBELL!
        if let Some(notify_addr) = self.notify_address {
            crate::serial_println!("[VIRTIO] Ringing Hardware Doorbell...");
            // Writing the Queue Index (0) to the Notify Register wakes the hypervisor
            core::ptr::write_volatile(notify_addr as *mut u16, 0);
        }

        Ok(())
    }

    /// [MICT: THE FIRST MAIL]
    /// Asks the GPU for the physical dimensions of the monitor.
    pub unsafe fn get_display_info(&mut self) -> Result<(), &'static str> {
        let mailbox_virt = self.mailbox_virt.expect("Mailbox missing!");
        let mailbox_phys = self.mailbox_phys.expect("Mailbox missing!");
        let vq_virt = self.control_queue_virt.expect("Control Queue missing!");
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        crate::serial_println!("[VIRTIO] Drafting 'GetDisplayInfo' Command...");

        // 1. DRAFT THE COMMAND (Device Readable)
        let cmd_ptr = mailbox_virt as *mut ControlHeader;
        *cmd_ptr = ControlHeader::with_ty(CommandTy::GetDisplayInfo);

        // 2. PREPARE THE EMPTY ENVELOPE FOR THE REPLY (Device Writable)
        // We place the response envelope 512 bytes deep into our Mailbox page
        let resp_phys = mailbox_phys + 512;
        let resp_virt = mailbox_virt + 512;
        let resp_ptr = resp_virt as *mut GetDisplayInfo;
        
        // Zero out the reply envelope
        core::ptr::write_bytes(resp_ptr as *mut u8, 0, core::mem::size_of::<GetDisplayInfo>());

        // 3. LOAD THE DESCRIPTORS (We need 2 slots)
        let head_idx = vq.free_head as usize;
        let next_idx = vq.descriptors[head_idx].next as usize;
        
        vq.free_head = vq.descriptors[next_idx].next; // Remove from free list
        vq.num_free -= 2;

        // Descriptor 1: The Command (Flags: NEXT = 1)
        vq.descriptors[head_idx].addr = mailbox_phys;
        vq.descriptors[head_idx].len = core::mem::size_of::<ControlHeader>() as u32;
        vq.descriptors[head_idx].flags = crate::virtqueue::VIRTQ_DESC_F_NEXT;
        vq.descriptors[head_idx].next = next_idx as u16;

        // Descriptor 2: The Reply Envelope (Flags: WRITE = 2)
        vq.descriptors[next_idx].addr = resp_phys;
        vq.descriptors[next_idx].len = core::mem::size_of::<GetDisplayInfo>() as u32;
        vq.descriptors[next_idx].flags = crate::virtqueue::VIRTQ_DESC_F_WRITE;

        // 4. PUT IT IN THE OUTBOX (Available Ring)
        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 256] = head_idx as u16;

        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        vq.available.idx.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        // 5. RING THE DOORBELL!
        if let Some(notify_addr) = self.notify_address {
            crate::serial_println!("[VIRTIO] Ringing Doorbell...");
            core::ptr::write_volatile(notify_addr as *mut u16, 0);
        }

        // 6. WAIT FOR THE REPLY (Spin on the Used Ring)
        let starting_used_idx = vq.last_used_idx;
        loop {
            let current_used = vq.used.idx.load(core::sync::atomic::Ordering::Acquire);
            if starting_used_idx != current_used {
                break; // The GPU wrote back!
            }
            // x86_64::instructions::hlt(); // Wait for it
        }
        vq.last_used_idx = vq.used.idx.load(core::sync::atomic::Ordering::Acquire);

        // 7. OPEN THE ENVELOPE!
        let response = &*resp_ptr;
        if response.header.ty == CommandTy::RespOkDisplayInfo {
            let display = &response.display_info[0];
            crate::serial_println!(
                "[VIRTIO] 📬 GPU Reply Received! Monitor 0 is {}x{} at (X:{}, Y:{})",
                display.rect.width, display.rect.height, display.rect.x, display.rect.y
            );
        } else {
            crate::serial_println!("[VIRTIO] 🛑 GPU returned error code: {:?}", response.header.ty);
        }

        Ok(())
    }
    
    // Helper to generate unique IDs for our canvases
    pub fn next_resource_id(&self) -> u32 {
        self.resource_id_counter.fetch_add(1, Ordering::SeqCst)
    }
}