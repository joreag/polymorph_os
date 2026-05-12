// kernel/src/virtio_gpu.rs

use core::sync::atomic::{AtomicU32, Ordering};
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::{VirtAddr, PhysAddr};
use x86_64::structures::paging::{FrameAllocator, Size4KiB};
pub static VIRTIO_BACKING_VIRT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);


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
        vq.available.ring[avail_idx % 64] = head_idx as u16;

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
    pub unsafe fn get_display_info(&mut self) -> Result<(u32, u32), &'static str> {
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
        vq.available.ring[avail_idx % 64] = head_idx as u16;

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
        
        // Temporarily hold the result so we can clean up memory before returning!
        let result = if response.header.ty == CommandTy::RespOkDisplayInfo {
            let display = &response.display_info[0];
            crate::serial_println!(
                "[VIRTIO] 📬 GPU Reply Received! Monitor 0 is {}x{} at (X:{}, Y:{})",
                display.rect.width, display.rect.height, display.rect.x, display.rect.y
            );
            Ok((display.rect.width, display.rect.height))
        } else {
            crate::serial_println!("[VIRTIO] 🛑 GPU returned error code: {:?}", response.header.ty);
            Err("Failed to get display info")
        };

        //[MICT: RECLAIM DESCRIPTORS]
        vq.descriptors[next_idx].next = vq.free_head;
        vq.free_head = head_idx as u16;
        vq.num_free += 2;

        // Finally, return the tuple!
        result
    }
    
    // Helper to generate unique IDs for our canvases
    pub fn next_resource_id(&self) -> u32 {
        self.resource_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// [MICT: SEIZE THE CANVAS]
    /// Commands the GPU to create a 2D resource (a canvas) in its VRAM.
    pub unsafe fn create_2d_canvas(&mut self, resource_id: u32, width: u32, height: u32) -> Result<(), &'static str> {
        let mailbox_virt = self.mailbox_virt.expect("Mailbox missing!");
        let mailbox_phys = self.mailbox_phys.expect("Mailbox missing!");
        let vq_virt = self.control_queue_virt.expect("Control Queue missing!");
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        crate::serial_println!("[VIRTIO-GPU] Forging 2D Canvas ({}x{}) with Resource ID: {}", width, height, resource_id);

        // 1. DRAFT THE COMMAND
        let cmd_ptr = mailbox_virt as *mut ResourceCreate2d;
        *cmd_ptr = ResourceCreate2d {
            header: ControlHeader::with_ty(CommandTy::ResourceCreate2d),
            resource_id,
            format: ResourceFormat::Bgrx, // Standard 32-bit color format
            width,
            height,
        };

        // 2. PREPARE THE REPLY ENVELOPE (Offset by 512 bytes)
        let resp_phys = mailbox_phys + 512;
        let resp_virt = mailbox_virt + 512;
        core::ptr::write_bytes(resp_virt as *mut u8, 0, core::mem::size_of::<ControlHeader>());

        // 3. LOAD DESCRIPTORS & SEND
        let head_idx = vq.free_head as usize;
        let next_idx = vq.descriptors[head_idx].next as usize;
        vq.free_head = vq.descriptors[next_idx].next;
        vq.num_free -= 2;

        // Desc 1: Command
        vq.descriptors[head_idx].addr = mailbox_phys;
        vq.descriptors[head_idx].len = core::mem::size_of::<ResourceCreate2d>() as u32;
        vq.descriptors[head_idx].flags = crate::virtqueue::VIRTQ_DESC_F_NEXT;
        vq.descriptors[head_idx].next = next_idx as u16;

        // Desc 2: Reply
        vq.descriptors[next_idx].addr = resp_phys;
        vq.descriptors[next_idx].len = core::mem::size_of::<ControlHeader>() as u32;
        vq.descriptors[next_idx].flags = crate::virtqueue::VIRTQ_DESC_F_WRITE;

        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 64] = head_idx as u16;

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        vq.available.idx.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        // RING DOORBELL
        if let Some(notify_addr) = self.notify_address {
            core::ptr::write_volatile(notify_addr as *mut u16, 0);
        }

        // WAIT FOR REPLY
        let starting_used_idx = vq.last_used_idx;
        loop {
            if starting_used_idx != vq.used.idx.load(core::sync::atomic::Ordering::Acquire) { break; }
        }
        vq.last_used_idx = vq.used.idx.load(core::sync::atomic::Ordering::Acquire);

        let response = &*(resp_virt as *const ControlHeader);
        if response.ty == CommandTy::RespOkNodata {
            crate::serial_println!("  -> [OK] Canvas Created!");
            // [MICT: RECLAIM DESCRIPTORS]
        vq.descriptors[next_idx].next = vq.free_head;
        vq.free_head = head_idx as u16;
        vq.num_free += 2;
            Ok(())
        } else {
            Err("GPU failed to create 2D canvas.")
        }
        
    }

    ///[MICT: THE DMA UMBILICAL CORD]
    /// Tells the GPU the exact physical RAM address of our Splat Engine's back_buffer.
    pub unsafe fn attach_backing(&mut self, resource_id: u32, back_buffer_phys: u64, buffer_length: u32) -> Result<(), &'static str> {
        let mailbox_virt = self.mailbox_virt.expect("Mailbox missing!");
        let mailbox_phys = self.mailbox_phys.expect("Mailbox missing!");
        let vq_virt = self.control_queue_virt.expect("Control Queue missing!");
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        crate::serial_println!("[VIRTIO-GPU] Attaching MICT Splat RAM to GPU Canvas...");

        // 1. DRAFT THE COMMAND (AttachBacking struct followed immediately by MemEntry structs)
        // We write the command header to the start of the mailbox
        let cmd_ptr = mailbox_virt as *mut AttachBacking;
        *cmd_ptr = AttachBacking {
            header: ControlHeader::with_ty(CommandTy::ResourceAttachBacking),
            resource_id,
            num_entries: 1, // We are providing 1 contiguous chunk of physical RAM
        };

        // We write the Memory Entry immediately after the command struct in the mailbox!
        let entry_ptr = (mailbox_virt + core::mem::size_of::<AttachBacking>() as u64) as *mut MemEntry;
        *entry_ptr = MemEntry {
            address: back_buffer_phys,
            length: buffer_length,
            padding: 0,
        };

        let total_cmd_size = core::mem::size_of::<AttachBacking>() + core::mem::size_of::<MemEntry>();

        // 2. PREPARE REPLY ENVELOPE (Offset by 512 bytes to be safe)
        let resp_phys = mailbox_phys + 512;
        let resp_virt = mailbox_virt + 512;
        core::ptr::write_bytes(resp_virt as *mut u8, 0, core::mem::size_of::<ControlHeader>());

        // 3. LOAD DESCRIPTORS & SEND
        let head_idx = vq.free_head as usize;
        let next_idx = vq.descriptors[head_idx].next as usize;
        vq.free_head = vq.descriptors[next_idx].next;
        vq.num_free -= 2;

        vq.descriptors[head_idx].addr = mailbox_phys;
        vq.descriptors[head_idx].len = total_cmd_size as u32;
        vq.descriptors[head_idx].flags = crate::virtqueue::VIRTQ_DESC_F_NEXT;
        vq.descriptors[head_idx].next = next_idx as u16;

        vq.descriptors[next_idx].addr = resp_phys;
        vq.descriptors[next_idx].len = core::mem::size_of::<ControlHeader>() as u32;
        vq.descriptors[next_idx].flags = crate::virtqueue::VIRTQ_DESC_F_WRITE;

        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 64] = head_idx as u16;

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        vq.available.idx.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        // RING DOORBELL
        if let Some(notify_addr) = self.notify_address {
            core::ptr::write_volatile(notify_addr as *mut u16, 0);
        }

        // WAIT FOR REPLY
        let starting_used_idx = vq.last_used_idx;
        loop {
            if starting_used_idx != vq.used.idx.load(core::sync::atomic::Ordering::Acquire) { break; }
        }
        vq.last_used_idx = vq.used.idx.load(core::sync::atomic::Ordering::Acquire);

        let response = &*(resp_virt as *const ControlHeader);
        if response.ty == CommandTy::RespOkNodata {
            crate::serial_println!("  -> [OK] DMA Backing Attached!");
            // [MICT: RECLAIM DESCRIPTORS]
        vq.descriptors[next_idx].next = vq.free_head;
        vq.free_head = head_idx as u16;
        vq.num_free += 2;
            Ok(())
        } else {
            Err("GPU failed to attach backing memory.")
        }
    }

    ///[MICT: SEIZE THE MONITOR]
    /// Binds our 2D Canvas to a physical display output (Scanout 0).
    pub unsafe fn set_scanout(&mut self, scanout_id: u32, resource_id: u32, width: u32, height: u32) -> Result<(), &'static str> {
        let mailbox_virt = self.mailbox_virt.expect("Mailbox missing!");
        let mailbox_phys = self.mailbox_phys.expect("Mailbox missing!");
        let vq_virt = self.control_queue_virt.expect("Queue missing!");
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        crate::serial_println!("[VIRTIO-GPU] Binding Canvas to Monitor {}...", scanout_id);

        let cmd_ptr = mailbox_virt as *mut SetScanout;
        *cmd_ptr = SetScanout {
            header: ControlHeader::with_ty(CommandTy::SetScanout),
            rect: GpuRect { x: 0, y: 0, width, height },
            scanout_id,
            resource_id,
        };

        let resp_phys = mailbox_phys + 512;
        let resp_virt = mailbox_virt + 512;
        core::ptr::write_bytes(resp_virt as *mut u8, 0, core::mem::size_of::<ControlHeader>());

        let head_idx = vq.free_head as usize;
        let next_idx = vq.descriptors[head_idx].next as usize;
        vq.free_head = vq.descriptors[next_idx].next;
        vq.num_free -= 2;

        vq.descriptors[head_idx].addr = mailbox_phys;
        vq.descriptors[head_idx].len = core::mem::size_of::<SetScanout>() as u32;
        vq.descriptors[head_idx].flags = crate::virtqueue::VIRTQ_DESC_F_NEXT;
        vq.descriptors[head_idx].next = next_idx as u16;

        vq.descriptors[next_idx].addr = resp_phys;
        vq.descriptors[next_idx].len = core::mem::size_of::<ControlHeader>() as u32;
        vq.descriptors[next_idx].flags = crate::virtqueue::VIRTQ_DESC_F_WRITE;

        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 64] = head_idx as u16;

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        vq.available.idx.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        if let Some(notify_addr) = self.notify_address { core::ptr::write_volatile(notify_addr as *mut u16, 0); }

        let starting_used_idx = vq.last_used_idx;
        loop { if starting_used_idx != vq.used.idx.load(core::sync::atomic::Ordering::Acquire) { break; } }
        vq.last_used_idx = vq.used.idx.load(core::sync::atomic::Ordering::Acquire);

        let response = &*(resp_virt as *const ControlHeader);
        if response.ty == CommandTy::RespOkNodata {
            crate::serial_println!("  -> [OK] Scanout Locked!");
            // [MICT: RECLAIM DESCRIPTORS]
        vq.descriptors[next_idx].next = vq.free_head;
        vq.free_head = head_idx as u16;
        vq.num_free += 2;
            Ok(())
        } else {
            Err("GPU failed to set scanout.")
        }
    }

    ///[MICT: NON-BLOCKING HARDWARE RENDER]
    /// Drops the Transfer and Flush commands into the mailbox and walks away.
    pub unsafe fn flush_to_screen(&mut self, resource_id: u32, width: u32, height: u32) {
        let mailbox_virt = self.mailbox_virt.unwrap();
        let mailbox_phys = self.mailbox_phys.unwrap();
        let vq = &mut *(self.control_queue_virt.unwrap() as *mut crate::virtqueue::VirtQueue);

        // We need 4 descriptors for this combined command
        if vq.num_free < 4 { return; } // Drop the frame if the queue is full, don't crash!

        // --- COMMAND 1: TRANSFER TO HOST ---
        let cmd1_ptr = mailbox_virt as *mut TransferToHost2d;
        *cmd1_ptr = TransferToHost2d {
            header: ControlHeader::with_ty(CommandTy::TransferToHost2d),
            rect: GpuRect { x: 0, y: 0, width, height },
            offset: 0,
            resource_id,
            padding: 0,
        };

        // --- COMMAND 2: RESOURCE FLUSH ---
        // Place it right after the first command in our mailbox
        let cmd2_offset = core::mem::size_of::<TransferToHost2d>() as u64;
        let cmd2_ptr = (mailbox_virt + cmd2_offset) as *mut ResourceFlush;
        *cmd2_ptr = ResourceFlush {
            header: ControlHeader::with_ty(CommandTy::ResourceFlush),
            rect: GpuRect { x: 0, y: 0, width, height },
            resource_id,
            padding: 0,
        };

        // Empty response envelopes
        let resp1_phys = mailbox_phys + 512;
        let resp2_phys = mailbox_phys + 528; // Shifted over for the second response
        
        // Grab 4 linked descriptors
        let head_idx = vq.free_head as usize;
        let idx2 = vq.descriptors[head_idx].next as usize;
        let idx3 = vq.descriptors[idx2].next as usize;
        let tail_idx = vq.descriptors[idx3].next as usize;
        
        vq.free_head = vq.descriptors[tail_idx].next;
        vq.num_free -= 4;

        // Desc 1: Transfer Command
        vq.descriptors[head_idx].addr = mailbox_phys;
        vq.descriptors[head_idx].len = core::mem::size_of::<TransferToHost2d>() as u32;
        vq.descriptors[head_idx].flags = crate::virtqueue::VIRTQ_DESC_F_NEXT;
        
        // Desc 2: Transfer Response
        vq.descriptors[idx2].addr = resp1_phys;
        vq.descriptors[idx2].len = core::mem::size_of::<ControlHeader>() as u32;
        // NOTE: We don't use NEXT here, this concludes the first command packet
        vq.descriptors[idx2].flags = crate::virtqueue::VIRTQ_DESC_F_WRITE;

        // Desc 3: Flush Command
        vq.descriptors[idx3].addr = mailbox_phys + cmd2_offset;
        vq.descriptors[idx3].len = core::mem::size_of::<ResourceFlush>() as u32;
        vq.descriptors[idx3].flags = crate::virtqueue::VIRTQ_DESC_F_NEXT;

        // Desc 4: Flush Response
        vq.descriptors[tail_idx].addr = resp2_phys;
        vq.descriptors[tail_idx].len = core::mem::size_of::<ControlHeader>() as u32;
        vq.descriptors[tail_idx].flags = crate::virtqueue::VIRTQ_DESC_F_WRITE;

        // Put BOTH command heads in the available ring
        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 64] = head_idx as u16;
        vq.available.ring[(avail_idx + 1) % 64] = idx3 as u16;
        
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        
        // We added 2 commands, so increment index by 2
        vq.available.idx.fetch_add(2, core::sync::atomic::Ordering::SeqCst);

        // RING THE DOORBELL
        if let Some(n) = self.notify_address { core::ptr::write_volatile(n as *mut u16, 0); }

        // --- THE CRITICAL FIX: DO NOT WAIT ---
        // We immediately reclaim the descriptors, assuming the GPU will process them instantly.
        // In a true Phase 4 driver, we would reclaim these inside a hardware interrupt handler,
        // but for now, this blind reclaim un-bricks the OS.
        vq.descriptors[idx2].next = vq.free_head;
        vq.free_head = head_idx as u16;
        
        vq.descriptors[tail_idx].next = vq.free_head;
        vq.free_head = idx3 as u16;
        
        vq.num_free += 4;
    }
}