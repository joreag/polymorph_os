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
            let color_pattern = [b, g, r, 255]; 
            // Write 4 bytes at a time directly to RAM, bypassing function overhead
            for pixel in self.back_buffer.chunks_exact_mut(4) {
                pixel.copy_from_slice(&color_pattern);
            }
        } else {
            for y in 0..self.height {
                for x in 0..self.width {
                    self.draw_pixel(x, y, r, g, b);
                }
            }
        }
    }

    //[MICT: ROW-CACHED FROSTED GLASS]
    pub fn draw_glass_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: u8, g: u8, b: u8, alpha: u8) {
        if alpha == 0 { return; }
        
        // Strict boundary clamps prevent the Kernel Panic!
        let end_y = core::cmp::min(y + h, self.height);
        let end_x = core::cmp::min(x + w, self.width);
        
        let a = alpha as u16;
        let inv_a = 255 - a;

        for py in y..end_y {
            // Calculate the Y offset ONCE per row, rather than inside the X loop!
            let row_offset = py * self.width * self.bytes_per_pixel;
            
            for px in x..end_x {
                let offset = row_offset + (px * self.bytes_per_pixel);
                
                if alpha == 255 {
                    self.back_buffer[offset] = b;
                    self.back_buffer[offset + 1] = g;
                    self.back_buffer[offset + 2] = r;
                } else {
                    let bg_b = self.back_buffer[offset] as u16;
                    let bg_g = self.back_buffer[offset + 1] as u16;
                    let bg_r = self.back_buffer[offset + 2] as u16;

                    // [FAST MATH]: Bit-shifting (>> 8) divides by 256. 
                    // This takes 1 CPU cycle instead of the 40+ cycles required for `/ 255`!
                    self.back_buffer[offset] = (((b as u16 * a) + (bg_b * inv_a)) >> 8) as u8;
                    self.back_buffer[offset + 1] = (((g as u16 * a) + (bg_g * inv_a)) >> 8) as u8;
                    self.back_buffer[offset + 2] = (((r as u16 * a) + (bg_r * inv_a)) >> 8) as u8;
                }
            }
        }
    }

    pub fn draw_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.width || y >= self.height { return; }
        let offset = (y * self.width + x) * self.bytes_per_pixel;
        
        self.back_buffer[offset] = b;
        self.back_buffer[offset + 1] = g;
        self.back_buffer[offset + 2] = r;
    }

    pub fn read_pixel(&self, x: usize, y: usize) -> (u8, u8, u8) {
        if x >= self.width || y >= self.height { return (0, 0, 0); }
        let offset = (y * self.width + x) * self.bytes_per_pixel;
        
        let b = self.back_buffer[offset];
        let g = self.back_buffer[offset + 1];
        let r = self.back_buffer[offset + 2];
        (r, g, b)
    }

    pub fn blend_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, alpha: u8) {
        if alpha == 255 {
            self.draw_pixel(x, y, r, g, b);
            return;
        }
        if alpha == 0 { return; }
        if x >= self.width || y >= self.height { return; } 

        let offset = (y * self.width + x) * self.bytes_per_pixel;
        let bg_b = self.back_buffer[offset] as u16;
        let bg_g = self.back_buffer[offset + 1] as u16;
        let bg_r = self.back_buffer[offset + 2] as u16;

        let a = alpha as u16;
        let inv_a = 255 - a;

        // [FAST MATH] Bitshift instead of integer division
        self.back_buffer[offset] = (((b as u16 * a) + (bg_b * inv_a)) >> 8) as u8;
        self.back_buffer[offset + 1] = (((g as u16 * a) + (bg_g * inv_a)) >> 8) as u8;
        self.back_buffer[offset + 2] = (((r as u16 * a) + (bg_r * inv_a)) >> 8) as u8;
    }

    pub fn swap_buffers(&mut self) {
        unsafe {
            ptr::copy_nonoverlapping(
                self.back_buffer.as_ptr(),
                self.framebuffer_ptr,
                self.back_buffer.len(),
            );
        }
    }
}