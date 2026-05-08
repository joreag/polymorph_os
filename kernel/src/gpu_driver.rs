use core::ptr;
//use core::fmt;
use bootloader_api::info::FrameBuffer;
//use font8x8::legacy::BASIC_LEGACY;
use spin::Mutex;
//use x86_64::instructions::interrupts;
use alloc::vec::Vec; // [MICT: Import Vec]
use alloc::vec;

pub static GPU_WRITER: Mutex<Option<GpuDriver>> = Mutex::new(None);

pub struct GpuDriver {
    framebuffer_ptr: *mut u8,
    pub back_buffer: Vec<u8>,
    pub nebula_buffer: Vec<u8>, // [MICT: THE NEW CACHE]
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
            nebula_buffer: vec![0; size], // Initialize 4MB Cache
            width: info.width,
            height: info.height,
            bytes_per_pixel: info.bytes_per_pixel,
        }
    }

    //[MICT: CACHE MANAGEMENT]
    pub fn save_to_nebula_cache(&mut self) {
        self.nebula_buffer.copy_from_slice(&self.back_buffer);
    }

    pub fn restore_from_nebula_cache(&mut self) {
        // This takes ~1ms instead of the 500ms it takes to do the splat math!
        self.back_buffer.copy_from_slice(&self.nebula_buffer);
    }

    // [MICT: BRING BACK THE FROSTED GLASS]
    pub fn draw_glass_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: u8, g: u8, b: u8, alpha: u8) {
        let end_y = core::cmp::min(y + h, self.height);
        let end_x = core::cmp::min(x + w, self.width);
        for py in y..end_y {
            for px in x..end_x {
                self.blend_pixel(px, py, r, g, b, alpha);
            }
        }
    }

    // [MICT: DRAW TO THE HIDDEN CANVAS, NOT THE SCREEN]
    pub fn draw_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.width || y >= self.height { return; }
        let offset = (y * self.width + x) * self.bytes_per_pixel;
        
        // Write to our Vec in RAM!
        self.back_buffer[offset] = b;
        self.back_buffer[offset + 1] = g;
        self.back_buffer[offset + 2] = r;
    }

    pub fn read_pixel(&self, x: usize, y: usize) -> (u8, u8, u8) {
        if x >= self.width || y >= self.height { return (0, 0, 0); }
        let offset = (y * self.width + x) * self.bytes_per_pixel;
        
        // Read from the hidden canvas
        let b = self.back_buffer[offset];
        let g = self.back_buffer[offset + 1];
        let r = self.back_buffer[offset + 2];
        (r, g, b)
    }

    pub fn clear_screen(&mut self, r: u8, g: u8, b: u8) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.draw_pixel(x, y, r, g, b);
            }
        }
    }

    pub fn blend_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, alpha: u8) {
        if alpha == 255 {
            self.draw_pixel(x, y, r, g, b);
            return;
        }
        if alpha == 0 { return; }

        let (bg_r, bg_g, bg_b) = self.read_pixel(x, y);
        let inv_alpha = 255 - alpha;

        let out_r = ((r as u16 * alpha as u16) + (bg_r as u16 * inv_alpha as u16)) / 255;
        let out_g = ((g as u16 * alpha as u16) + (bg_g as u16 * inv_alpha as u16)) / 255;
        let out_b = ((b as u16 * alpha as u16) + (bg_b as u16 * inv_alpha as u16)) / 255;

        self.draw_pixel(x, y, out_r as u8, out_g as u8, out_b as u8);
    }

    // [MICT: THE FLIP] Instantly copies the 4MB hidden canvas to the physical monitor!
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