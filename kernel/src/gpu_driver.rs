use core::ptr;
use bootloader_api::info::FrameBuffer;
use spin::Mutex;
use alloc::vec::Vec;
use alloc::vec;

pub static GPU_WRITER: Mutex<Option<GpuDriver>> = Mutex::new(None);

pub struct GpuDriver {
    framebuffer_ptr: *mut u8,
    pub back_buffer: Vec<u8>,
    pub nebula_buffer: Vec<u8>, 
    pub width: usize,
    pub height: usize,
    bytes_per_pixel: usize,
}

unsafe impl Send for GpuDriver {}
unsafe impl Sync for GpuDriver {}

impl GpuDriver {
    pub unsafe fn new(fb: &mut FrameBuffer) -> Self {
        let info = fb.info();
        let size = fb.buffer_mut().len();
        GpuDriver {
            framebuffer_ptr: fb.buffer_mut().as_mut_ptr(),
            back_buffer: vec![0; size],
            nebula_buffer: vec![0; size], 
            width: info.width,
            height: info.height,
            bytes_per_pixel: info.bytes_per_pixel,
        }
    }

    pub fn save_to_nebula_cache(&mut self) {
        self.nebula_buffer.copy_from_slice(&self.back_buffer);
    }

    pub fn restore_from_nebula_cache(&mut self) {
        self.back_buffer.copy_from_slice(&self.nebula_buffer);
    }

    // [MICT: FAST CLEAR] 
    // Replaced nested loops with high-speed memory chunking!
    pub fn clear_screen(&mut self, r: u8, g: u8, b: u8) {
        if self.bytes_per_pixel == 4 {
            let color_pattern =[b, g, r, 255]; 
            // Write 4 bytes at a time directly to RAM, bypassing function overhead
            for pixel in self.back_buffer.chunks_exact_mut(4) {
                pixel.copy_from_slice(&color_pattern);
            }
        } else {
            for y in 0..self.height {
                for x in 0..self.width {
                    // [FIX]: Explicitly cast usize to i32 to match the new safe signature!
                    self.draw_pixel(x as i32, y as i32, r, g, b);
                }
            }
        }
    }

    ///[MICT: DYNAMIC RESOLUTION]
    /// Morphs the software buffers to perfectly match the physical hardware monitor.
    pub fn resize_to_hardware(&mut self, new_width: usize, new_height: usize) {
        if self.width == new_width && self.height == new_height {
            return; // Already perfectly sized
        }
        
        crate::serial_println!("[GPU DRIVER] Morphing internal buffers to {}x{}...", new_width, new_height);
        
        self.width = new_width;
        self.height = new_height;
        let new_size = new_width * new_height * self.bytes_per_pixel;
        
        // Reallocate the vectors in our O(1) MICT Memory!
        self.back_buffer = alloc::vec![0; new_size];
        self.nebula_buffer = alloc::vec![0; new_size];
    }

    //[MICT: OFF-SCREEN SAFE FROSTED GLASS]
    pub fn draw_glass_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8, alpha: u8) {
        if alpha == 0 { return; }
        
        // Safely clamp the starting coordinates to 0 so we never draw off the top/left edge!
        let start_x = core::cmp::max(0, x);
        let start_y = core::cmp::max(0, y);
        
        // Safely clamp the ending coordinates to the screen limits
        let end_x = core::cmp::min(x + w, self.width as i32);
        let end_y = core::cmp::min(y + h, self.height as i32);

        // If the window is completely off-screen, do nothing!
        if start_x >= end_x || start_y >= end_y { return; }

        let a = alpha as u32;
        let inv_a = 255 - a;

        for py in start_y..end_y {
            let row_offset = (py as usize) * self.width * self.bytes_per_pixel;
            
            for px in start_x..end_x {
                let offset = row_offset + ((px as usize) * self.bytes_per_pixel);
                
                if alpha == 255 {
                    self.back_buffer[offset] = b;
                    self.back_buffer[offset + 1] = g;
                    self.back_buffer[offset + 2] = r;
                } else {
                    let bg_b = self.back_buffer[offset] as u32;
                    let bg_g = self.back_buffer[offset + 1] as u32;
                    let bg_r = self.back_buffer[offset + 2] as u32;

                    self.back_buffer[offset] = (((b as u32 * a) + (bg_b * inv_a)) >> 8) as u8;
                    self.back_buffer[offset + 1] = (((g as u32 * a) + (bg_g * inv_a)) >> 8) as u8;
                    self.back_buffer[offset + 2] = (((r as u32 * a) + (bg_r * inv_a)) >> 8) as u8;
                }
            }
        }
    }

    pub fn draw_pixel(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8) {
        // Safe boundaries!
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 { return; }
        
        let offset = ((y as usize) * self.width + (x as usize)) * self.bytes_per_pixel;
        self.back_buffer[offset] = b;
        self.back_buffer[offset + 1] = g;
        self.back_buffer[offset + 2] = r;
    }

    pub fn blend_pixel(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8, alpha: u8) {
        if alpha == 255 {
            self.draw_pixel(x, y, r, g, b);
            return;
        }
        if alpha == 0 { return; }
        
        // Safe boundaries!
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 { return; }

        let offset = ((y as usize) * self.width + (x as usize)) * self.bytes_per_pixel;
        let bg_b = self.back_buffer[offset] as u32;
        let bg_g = self.back_buffer[offset + 1] as u32;
        let bg_r = self.back_buffer[offset + 2] as u32;

        let a = alpha as u32;
        let inv_a = 255 - a;

        self.back_buffer[offset] = (((b as u32 * a) + (bg_b * inv_a)) >> 8) as u8;
        self.back_buffer[offset + 1] = (((g as u32 * a) + (bg_g * inv_a)) >> 8) as u8;
        self.back_buffer[offset + 2] = (((r as u32 * a) + (bg_r * inv_a)) >> 8) as u8;
    }

    // [MICT: THE FLIP] 
    pub fn swap_buffers(&mut self) {
        let virtio_backing = crate::virtio_gpu::VIRTIO_BACKING_VIRT.load(core::sync::atomic::Ordering::SeqCst);
        
        if virtio_backing != 0 {
            // --- VIRTIO HARDWARE ACCELERATION MODE ---
            // 1. Copy our software pixels into the physical DMA RAM
            unsafe {
                ptr::copy_nonoverlapping(
                    self.back_buffer.as_ptr(),
                    virtio_backing as *mut u8,
                    self.back_buffer.len(),
                );
            }
            
            // 2. Ring the VirtIO Doorbell to push the RAM to the physical monitor!
            if let Some(virtio) = crate::virtio_gpu::VIRTIO_GPU.lock().as_mut() {
                unsafe { virtio.flush_to_screen(1, self.width as u32, self.height as u32); }
            }
        //} else {
            // --- LEGACY UEFI GOP FALLBACK ---
           // unsafe {
                //ptr::copy_nonoverlapping(
                    //self.back_buffer.as_ptr(),
                    //self.framebuffer_ptr,
                   // self.back_buffer.len(),
                //);
            //}
        }
    }
}