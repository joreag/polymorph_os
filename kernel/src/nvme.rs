// In kernel/src/nvme.rs

use crate::serial_println;
use core::ptr::{read_volatile, write_volatile};
use x86_64::{VirtAddr, structures::paging::Translate};
use spin::Mutex;
// [MICT: GLOBAL HARDWARE ACCESS]
pub static NVME_DRIVE: Mutex<Option<NvmeController>> = Mutex::new(None);

///[MICT: ZERO-TRUST SILICON WRAPPER]
/// Forces the compiler to actually read/write the electrical pins, bypassing all optimizations.
#[repr(transparent)]
pub struct Volatile<T>(T);

impl<T> Volatile<T> {
    pub fn read(&self) -> T {
        unsafe { read_volatile(&self.0) }
    }
    pub fn write(&mut self, value: T) {
        unsafe { write_volatile(&mut self.0, value) }
    }
}

///[MICT: EXTRACTED HARDWARE TOPOLOGY]
/// The exact memory layout of the NVMe Controller's brain.
#[repr(C)]
pub struct NvmeRegs {
    pub cap_low: Volatile<u32>,   // 0x00: Capabilities Low
    pub cap_high: Volatile<u32>,  // 0x04: Capabilities High
    pub vs: Volatile<u32>,        // 0x08: Version
    pub intms: Volatile<u32>,     // 0x0C: Interrupt Mask Set
    pub intmc: Volatile<u32>,     // 0x10: Interrupt Mask Clear
    pub cc: Volatile<u32>,        // 0x14: Controller Configuration
    pub _rsvd: Volatile<u32>,     // 0x18: Reserved
    pub csts: Volatile<u32>,      // 0x1C: Controller Status
    pub nssr: Volatile<u32>,      // 0x20: Subsystem Reset
    pub aqa: Volatile<u32>,       // 0x24: Admin Queue Attributes
    pub asq_low: Volatile<u32>,   // 0x28: Admin Submission Queue Base Low
    pub asq_high: Volatile<u32>,  // 0x2C: Admin Submission Queue Base High
    pub acq_low: Volatile<u32>,   // 0x30: Admin Completion Queue Base Low
    pub acq_high: Volatile<u32>,  // 0x34: Admin Completion Queue Base High
}

pub struct NvmeController {
    regs: &'static mut NvmeRegs,
    mmio_base: usize,
    admin_sq_tail: u16,
    admin_cq_head: u16,
    
    // [MICT: THE I/O STATE TRACKERS]
    io_sq_tail: u16,
    io_cq_head: u16,
    io_phase: u16, // Starts at 1, flips to 0 when the queue wraps!
    
    io_buf_phys: u64,
}

impl NvmeController {
    pub unsafe fn new(mmio_base: usize) -> Self {
        unsafe {
            NvmeController {
                regs: &mut *(mmio_base as *mut NvmeRegs),
                mmio_base,
                admin_sq_tail: 0,
                admin_cq_head: 0,
                io_sq_tail: 0,
                io_cq_head: 0,
                io_phase: 1, // [CRITICAL] NVMe Phase tags always start at 1
                io_buf_phys: 0, 
            }
        }
    }
    /// [MICT: ITERATE] - The Hardware Ping
    pub fn ping(&self) {
        serial_println!("[MICT: ITERATE] Pinging NVMe Controller Registers...");
        //screen_println!("[MICT: ITERATE] Pinging NVMe Controller Registers...");

        let version = self.regs.vs.read();
        let status = self.regs.csts.read();
        let capabilities_low = self.regs.cap_low.read();

        // NVMe version is stored as: Major (2 bytes), Minor (1 byte), Tertiary (1 byte)
        let major = (version >> 16) & 0xFFFF;
        let minor = (version >> 8) & 0xFF;
        let tertiary = version & 0xFF;

        serial_println!("   -> NVMe Version: {}.{}.{}", major, minor, tertiary);
        //screen_println!("   -> NVMe Version: {}.{}.{}", major, minor, tertiary);
        
        serial_println!("   -> Controller Status (CSTS): {:#010X}", status);
        //screen_println!("   -> Controller Status (CSTS): {:#010X}", status);

        serial_println!("   -> Capabilities Low: {:#010X}", capabilities_low);
        //screen_println!("   -> Capabilities Low: {:#010X}", capabilities_low);

        if status == 0xFFFFFFFF {
            serial_println!("[DISSONANCE] Hardware returned 0xFFFFFFFF. Drive is dead or unmapped.");
            //screen_println!("[DISSONANCE] Hardware returned 0xFFFFFFFF. Drive is dead or unmapped.");
        } else {
            serial_println!("[MICT: CHECK] Two-way silicon communication established!");
            //screen_println!("[MICT: CHECK] Two-way silicon communication established!");
        }
    }

    /// [MICT: TRANSFORM] - Force the NVMe Controller to shut down
    pub fn disable(&mut self) {
        serial_println!("[MICT: TRANSFORM] Disabling NVMe Controller...");
        
        // Read the current Controller Configuration (CC)
        let mut cc = self.regs.cc.read();
        
        // Clear Bit 0 (Enable bit) to 0
        cc &= !1; 
        self.regs.cc.write(cc);

        // [MICT: CHECK] - Spin until the controller confirms it is offline
        serial_println!("   -> Waiting for CSTS.RDY to drop to 0...");
        loop {
            let csts = self.regs.csts.read();
            if (csts & 1) == 0 {
                break; // Drive is officially offline
            }
            core::hint::spin_loop(); // Polite bare-metal waiting
        }
        serial_println!("   [OK] NVMe Controller is OFFLINE.");
    }

    // 3. Update configure_and_enable to ask the Mapper for translations!
    pub fn configure_and_enable(&mut self, mapper: &impl Translate) {
        crate::serial_println!("[MICT: TRANSFORM] Configuring Admin Queues...");
        
        
            // [MICT: HARDWARE TRANSLATION] 
            // Ask the CPU Page Tables for the true physical silicon address!
            let sq_virt = VirtAddr::new(core::ptr::addr_of!(ADMIN_SQ) as u64);
            let cq_virt = VirtAddr::new(core::ptr::addr_of!(ADMIN_CQ) as u64);
            
            let sq_phys = mapper.translate_addr(sq_virt).expect("Failed to translate ASQ").as_u64();
            let cq_phys = mapper.translate_addr(cq_virt).expect("Failed to translate ACQ").as_u64();

            // Write the Queue Sizes to AQA (Size - 1)
            let aqa_val = ((ADMIN_QUEUE_SIZE as u32 - 1) << 16) | (ADMIN_QUEUE_SIZE as u32 - 1);
            self.regs.aqa.write(aqa_val);

            // Write the Physical Addresses to ASQ and ACQ registers
            self.regs.asq_low.write((sq_phys & 0xFFFFFFFF) as u32);
            self.regs.asq_high.write((sq_phys >> 32) as u32);
            
            self.regs.acq_low.write((cq_phys & 0xFFFFFFFF) as u32);
            self.regs.acq_high.write((cq_phys >> 32) as u32);

            crate::serial_println!("   -> ASQ Physical Addr: {:#X}", sq_phys);
            crate::serial_println!("   -> ACQ Physical Addr: {:#X}", cq_phys);
        

        crate::serial_println!("[MICT: TRANSFORM] Enabling NVMe Controller...");
        
        let mut cc = self.regs.cc.read();
        cc &= 0xFF00000F; // Clear middle bits
        cc |= (4 << 20) | (6 << 16) | 1; // Set IOSQES=6, IOCQES=4, Enable=1
        
        self.regs.cc.write(cc);

        loop {
            if (self.regs.csts.read() & 1) == 1 { break; }
            core::hint::spin_loop(); 
        }
        crate::serial_println!("   [OK] NVMe Controller is ONLINE with Admin Queues wired!");
    }

        // [MICT: THE DOORBELLS]
    unsafe fn sq_doorbell(&self, qid: u16, value: u16) {
        let cap_high = self.regs.cap_high.read();
        let dstrd = cap_high & 0b1111;
        let offset = 0x1000 + ((2 * qid as usize) * (4 << dstrd));
        let ptr = (self.mmio_base + offset) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value as u32); }
    }

    unsafe fn cq_doorbell(&self, qid: u16, value: u16) {
        let cap_high = self.regs.cap_high.read();
        let dstrd = cap_high & 0b1111;
        let offset = 0x1000 + (((2 * qid as usize) + 1) * (4 << dstrd));
        let ptr = (self.mmio_base + offset) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value as u32); }
    }

    //[MICT: THE COMMAND EXECUTION]
    pub fn identify_controller(&mut self, mapper: &impl Translate) {
        crate::serial_println!("[MICT: ITERATE] Issuing IDENTIFY CONTROLLER Command...");
        
        unsafe {
            let buf_virt = VirtAddr::new(core::ptr::addr_of!(IDENTIFY_BUF) as u64);
            let buf_phys = mapper.translate_addr(buf_virt).expect("Translation failed").as_u64();

            let mut cmd = NvmeCmd::empty();
            cmd.opcode = 0x06; 
            cmd.cdw10 = 1;     
            cmd.dptr[0] = buf_phys; 

            // [MICT: RUST 2024 FIX & STATE SYNCHRONIZATION]
            // We now use the unified helper function to guarantee the 
            // Head/Tail pointers stay mathematically synced with the silicon!
            if let Err(status) = self.submit_admin_cmd(cmd) {
                crate::serial_println!("[DISSONANCE] Identify Command Failed! Status: {:#X}", status);
                return;
            }

            // Read the Data!
            // We must use addr_of! to safely read the static buffer
            let buf_ptr = core::ptr::addr_of!(IDENTIFY_BUF.0) as *const [u8; 4096];
            
            //[MICT: RUST 2024 FIX] Explicitly wrap the raw pointer dereference in unsafe{}
            let model_bytes =  &(&(*buf_ptr))[24..64];
            
            let model_str = core::str::from_utf8(model_bytes).unwrap_or("UNKNOWN");
            
            crate::serial_println!("   [OK] DMA Transfer Complete.");
            crate::serial_println!("   *** NVMe DRIVE MODEL: {} ***", model_str.trim());
            //crate::screen_println!("   *** NVMe DRIVE MODEL: {} ***", model_str.trim());
        }
    }

    // [MICT: HELPER] Safely executes an Admin Command using the Head/Tail state
    unsafe fn submit_admin_cmd(&mut self, cmd: NvmeCmd) -> Result<(), u16> {
        unsafe {
            let tail = self.admin_sq_tail as usize;
            
            // [MICT: RUST 2024 FIX] Safely write to static array
            let sq_ptr = core::ptr::addr_of_mut!(ADMIN_SQ.0) as *mut[NvmeCmd; ADMIN_QUEUE_SIZE];
            (*sq_ptr)[tail] = cmd;
            
            self.admin_sq_tail += 1;
            self.sq_doorbell(0, self.admin_sq_tail);

            loop {
                let head = self.admin_cq_head as usize;
                
                // [MICT: RUST 2024 FIX] Safely read from static array
                let cq_ptr = core::ptr::addr_of!(ADMIN_CQ.0) as *const [NvmeComp; ADMIN_QUEUE_SIZE];
                let comp = core::ptr::read_volatile(&((*cq_ptr)[head]));
                
                let status = { comp.status };
                
                if (status & 1) == 1 {
                    self.admin_cq_head += 1;
                    self.cq_doorbell(0, self.admin_cq_head);
                    
                    let success = (status >> 1) == 0;
                    if success { return Ok(()); } else { return Err(status); }
                }
                core::hint::spin_loop();
            }
        }
    }

//[MICT: GENERALIZED I/O COMMAND EXECUTOR]
    unsafe fn submit_io_cmd(&mut self, cmd: NvmeCmd) -> Result<(), u16> {
        //[MICT: RUST 2024 FIX] Wrap the inner body in unsafe!
        unsafe {
            let tail = self.io_sq_tail as usize;
            
            let sq_ptr = core::ptr::addr_of_mut!(IO_SQ.0) as *mut[NvmeCmd; IO_QUEUE_SIZE];
            (*sq_ptr)[tail] = cmd;
            
            self.io_sq_tail += 1;
            if self.io_sq_tail as usize == IO_QUEUE_SIZE { self.io_sq_tail = 0; }
            
            self.sq_doorbell(1, self.io_sq_tail);

            loop {
                let head = self.io_cq_head as usize;
                let cq_ptr = core::ptr::addr_of!(IO_CQ.0) as *const[NvmeComp; IO_QUEUE_SIZE];
                let comp = core::ptr::read_volatile(&((*cq_ptr)[head]));
                let status = { comp.status };
                
                if (status & 1) == self.io_phase {
                    self.io_cq_head += 1;
                    if self.io_cq_head as usize == IO_QUEUE_SIZE {
                        self.io_cq_head = 0;
                        self.io_phase = !self.io_phase & 1; 
                    }
                    
                    self.cq_doorbell(1, self.io_cq_head);
                    
                    let success = (status >> 1) == 0;
                    if success { return Ok(()); } else { return Err(status); }
                }
                core::hint::spin_loop();
            }
        }
    }

    // [MICT: TRANSFORM] - Forge the High-Speed Data Lanes
    pub fn setup_io_queues(&mut self, mapper: &impl Translate) {
        crate::serial_println!("[MICT: TRANSFORM] Forging NVMe I/O Data Queues...");
        
        unsafe {
            let cq_virt = VirtAddr::new(core::ptr::addr_of!(IO_CQ) as u64);
            let sq_virt = VirtAddr::new(core::ptr::addr_of!(IO_SQ) as u64);
            let cq_phys = mapper.translate_addr(cq_virt).unwrap().as_u64();
            let sq_phys = mapper.translate_addr(sq_virt).unwrap().as_u64();
            //[MICT: CACHE THE DATA BUFFER ADDRESS]
            let buf_virt = VirtAddr::new(core::ptr::addr_of!(IO_BUF) as u64);
            self.io_buf_phys = mapper.translate_addr(buf_virt).unwrap().as_u64();

            // 1. Create I/O Completion Queue (Opcode 0x05)
            let mut cmd_cq = NvmeCmd::empty();
            cmd_cq.opcode = 0x05;
            cmd_cq.dptr[0] = cq_phys;
            cmd_cq.cdw10 = ((IO_QUEUE_SIZE as u32 - 1) << 16) | 1; // Size in Upper 16, Queue ID 1 in Lower
            cmd_cq.cdw11 = 1; // Physically Contiguous = 1
            
            self.submit_admin_cmd(cmd_cq).expect("Failed to create I/O CQ");
            crate::serial_println!("   [OK] I/O Completion Queue (QID 1) wired.");

            // 2. Create I/O Submission Queue (Opcode 0x01)
            let mut cmd_sq = NvmeCmd::empty();
            cmd_sq.opcode = 0x01;
            cmd_sq.dptr[0] = sq_phys;
            cmd_sq.cdw10 = ((IO_QUEUE_SIZE as u32 - 1) << 16) | 1; // Size in Upper 16, Queue ID 1 in Lower
            cmd_sq.cdw11 = (1 << 16) | 1; // Associated CQID 1 in Upper 16, Contiguous = 1
            
            self.submit_admin_cmd(cmd_sq).expect("Failed to create I/O SQ");
            crate::serial_println!("   [OK] I/O Submission Queue (QID 1) wired.");
        }
    }

//[MICT: EXECUTE] - Generalized Block Write
    pub fn write_block(&mut self, lba: u64, data: &[u8]) -> Result<(), u16> {
        if data.len() > 4096 { return Err(0xFFFF); }

        unsafe {
            let buf_ptr = core::ptr::addr_of_mut!(IO_BUF.0) as *mut u8;
            core::ptr::write_bytes(buf_ptr, 0, 4096); 
            core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, data.len()); 

            let mut cmd = NvmeCmd::empty();
            cmd.opcode = 0x01; // Write
            cmd.nsid = 1;      
            cmd.dptr[0] = self.io_buf_phys; 
            cmd.cdw10 = (lba & 0xFFFFFFFF) as u32; 
            cmd.cdw11 = (lba >> 32) as u32;
            cmd.cdw12 = 0; 

            self.submit_io_cmd(cmd) // Use the new synchronized helper!
        }
    }

    // [MICT: EXECUTE] - Generalized Block Read
    pub fn read_block(&mut self, lba: u64, dest_buffer: &mut[u8; 4096]) -> Result<(), u16> {
        unsafe {
            let buf_ptr = core::ptr::addr_of_mut!(IO_BUF.0) as *mut u8;
            core::ptr::write_bytes(buf_ptr, 0, 4096);

            let mut cmd = NvmeCmd::empty();
            cmd.opcode = 0x02; // Read
            cmd.nsid = 1;      
            cmd.dptr[0] = self.io_buf_phys; 
            cmd.cdw10 = (lba & 0xFFFFFFFF) as u32; 
            cmd.cdw11 = (lba >> 32) as u32;
            cmd.cdw12 = 0; 

            self.submit_io_cmd(cmd)?; // Execute and wait
            
            // If successful, copy data to user
            core::ptr::copy_nonoverlapping(buf_ptr, dest_buffer.as_mut_ptr(), 4096);
            Ok(())
        }
    }

}


// =====================================================================
//[MICT: THE NVME DATA STRUCTURES (Extracted from Redox OS)]
// =====================================================================

/// A submission queue entry. Exactly 64 bytes.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub struct NvmeCmd {
    pub opcode: u8,
    pub flags: u8,
    pub cid: u16,
    pub nsid: u32,
    pub _rsvd: u64,
    pub mptr: u64,
    pub dptr: [u64; 2],
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl NvmeCmd {
    pub const fn empty() -> Self {
        NvmeCmd {
            opcode: 0, flags: 0, cid: 0, nsid: 0, _rsvd: 0, mptr: 0,
            dptr: [0; 2], cdw10: 0, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }
}

/// A completion queue entry. Exactly 16 bytes.
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct NvmeComp {
    pub command_specific: u32,
    pub _rsvd: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub cid: u16,
    pub status: u16,
}

impl NvmeComp {
    pub const fn empty() -> Self {
        NvmeComp {
            command_specific: 0, _rsvd: 0, sq_head: 0, sq_id: 0, cid: 0, status: 0,
        }
    }
}

// =====================================================================
//[MICT: THE ADMIN MAILBOXES (Page-Aligned Physical RAM)]
// ==========================================

const ADMIN_QUEUE_SIZE: usize = 64; // Can hold 64 commands at a time

#[repr(C, align(4096))] // Force the compiler to align this to a 4KB Physical Page!
struct AdminSubmissionQueue([NvmeCmd; ADMIN_QUEUE_SIZE]);

#[repr(C, align(4096))]
struct AdminCompletionQueue([NvmeComp; ADMIN_QUEUE_SIZE]);

static mut ADMIN_SQ: AdminSubmissionQueue = AdminSubmissionQueue([NvmeCmd::empty(); ADMIN_QUEUE_SIZE]);
static mut ADMIN_CQ: AdminCompletionQueue = AdminCompletionQueue([NvmeComp::empty(); ADMIN_QUEUE_SIZE]);

//[MICT: THE DMA BUFFER]
// Exactly 4096 bytes, Page-Aligned, to catch the hard drive's response
#[repr(C, align(4096))]
struct DmaBuffer([u8; 4096]);
static mut IDENTIFY_BUF: DmaBuffer = DmaBuffer([0; 4096]);

const IO_QUEUE_SIZE: usize = 64;

#[repr(C, align(4096))]
struct IoSubmissionQueue([NvmeCmd; IO_QUEUE_SIZE]);

#[repr(C, align(4096))]
struct IoCompletionQueue([NvmeComp; IO_QUEUE_SIZE]);

static mut IO_SQ: IoSubmissionQueue = IoSubmissionQueue([NvmeCmd::empty(); IO_QUEUE_SIZE]);
static mut IO_CQ: IoCompletionQueue = IoCompletionQueue([NvmeComp::empty(); IO_QUEUE_SIZE]);

// This is the physical 4KB landing pad for Reading/Writing real files!
static mut IO_BUF: DmaBuffer = DmaBuffer([0; 4096]);