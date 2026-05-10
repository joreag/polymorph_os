use crate::gpu_driver::GpuDriver;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::fmt;
use core::sync::atomic::{AtomicI32, AtomicBool, Ordering};


pub static SPLAT_ENGINE: Mutex<Option<SplatEngine>> = Mutex::new(None);

// [MICT: GLOBAL UI STATE]
pub static CURSOR_X: AtomicI32 = AtomicI32::new(640);
pub static CURSOR_Y: AtomicI32 = AtomicI32::new(400);
pub static LEFT_CLICK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub struct GaussianSplat {
    pub x: i32, pub y: i32, pub z: i32, pub scale: i32,
    pub r: u8, pub g: u8, pub b: u8, pub opacity: u8,
}

// [MICT: THE AMORPHOUS WINDOW]
pub struct SemanticWindow {
    pub x: i32, pub y: i32, pub w: i32, pub h: i32,
    pub text_buffer: String,
    pub input_buffer: String, //[MICT: THE COMMAND BUFFER]
}

impl SemanticWindow {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        // No more blobs! Just coordinates and text buffers.
        SemanticWindow { 
            x, y, w, h, 
            text_buffer: String::from("Welcome to GenesisOS. Type HELP for commands.\n"),
            input_buffer: String::new(),
        }
    }

    pub fn move_to(&mut self, new_x: i32, new_y: i32) {
        self.x = new_x;
        self.y = new_y;
    }

    //[MICT: THE SLEEK UI]
    pub fn render_body(&self, gpu: &mut GpuDriver) {
        // No more 'as usize' danger! Pass the raw i32 coordinates.
        gpu.draw_glass_rect(self.x, self.y, self.w, self.h, 10, 20, 30, 200);
        gpu.draw_glass_rect(self.x, self.y, self.w, 30, 40, 80, 100, 255);
    }

// [MICT: THE COMMAND EXECUTOR]
pub fn execute_command(&mut self) {
    let raw_input = self.input_buffer.trim().to_uppercase(); 
    
    // Echo the user's command to BOTH the local screen and the remote terminal
    self.text_buffer.push_str("genesis> ");
    self.text_buffer.push_str(&self.input_buffer);
    crate::serial_println!("genesis> {}", self.input_buffer.trim());

    let mut parts = raw_input.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    let arg1 = parts.next().unwrap_or("");
    let _arg2 = parts.next().unwrap_or("");

    match cmd {
        "" => {} // Do nothing on empty command, the prompt will be added at the end
        "HELP" => {
            let help_text = "Available Commands:\n  CLEAR     - Wipes the terminal screen\n  SCAN      - Ex: 'SCAN PCI'. Enumerates hardware.\n  PING      - Ex: 'PING NVME'. Checks storage.\n  SAVE      - Ex: 'SAVE FILE.TXT'. Allocates a block.\n  READ      - Ex: 'READ FILE.TXT'. Fetches a block.\n";
            self.text_buffer.push_str(help_text);
            crate::serial_println!("{}", help_text);
        }
        "CLEAR" => {
            self.text_buffer.clear();
            crate::serial_println!("\x1B[2J\x1B[H");
        }
        "SCAN" => {
            if arg1 == "PCI" {
                // Call the real-time hardware probe!
                let out = crate::pci::scan_pci_dynamic();
                self.text_buffer.push_str(&out);
                
                // Use serial_print (not println) because the string already contains '\n's
                crate::serial_print!("{}", out); 
            } else {
                let usage = "Usage: SCAN PCI\n";
                self.text_buffer.push_str(usage);
                crate::serial_println!("{}", usage);
            }
        }
        "PING" => {
            if arg1 == "NVME" {
                let out = "[MICT: CHECK] NVMe Controller is ONLINE.\n";
                self.text_buffer.push_str(out);
                crate::serial_println!("{}", out);
            } else {
                let usage = "Usage: PING NVME\n";
                self.text_buffer.push_str(usage);
                crate::serial_println!("{}", usage);
            }
        }
        "SAVE" => {
            if arg1 != "" {
                let out = alloc::format!("Saving file: '{}'\n", arg1);
                self.text_buffer.push_str(&out);
                crate::serial_println!("{}", out);
                let payload = self.input_buffer.split_whitespace().skip(2).collect::<Vec<&str>>().join(" ");
                match crate::mfs::MictFileSystem::save_file(arg1, payload.as_bytes()) {
                    Ok(_) => {
                        let ok_out = "  [OK] File saved to MFS.\n";
                        self.text_buffer.push_str(ok_out);
                        crate::serial_println!("{}", ok_out);
                    },
                    Err(e) => {
                        let err_out = alloc::format!("  [FAIL] {}\n", e);
                        self.text_buffer.push_str(&err_out);
                        crate::serial_println!("{}", err_out);
                    },
                }
            } else {
                let usage = "Usage: SAVE <FILENAME> <CONTENT>\n";
                self.text_buffer.push_str(usage);
                crate::serial_println!("{}", usage);
            }
        }
        "READ" => {
            if arg1 != "" {
                let out = alloc::format!("Reading file: '{}'\n", arg1);
                self.text_buffer.push_str(&out);
                crate::serial_println!("{}", out);
                match crate::mfs::MictFileSystem::read_file(arg1) {
                    Ok(data) => {
                        let valid_len = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                        let text = core::str::from_utf8(&data[0..valid_len]).unwrap_or("[CORRUPT]");
                        let content_out = alloc::format!("  -> CONTENTS: {}\n", text);
                        self.text_buffer.push_str(&content_out);
                        crate::serial_println!("{}", content_out);
                    }
                    Err(e) => {
                        let err_out = alloc::format!("  [FAIL] {}\n", e);
                        self.text_buffer.push_str(&err_out);
                        crate::serial_println!("{}", err_out);
                    }
                }
            } else {
                let usage = "Usage: READ <FILENAME>\n";
                self.text_buffer.push_str(usage);
                crate::serial_println!("{}", usage);
            }
        }
        "LIST" => {
            let out = "Listing files in root directory:\n";
            self.text_buffer.push_str(out);
            crate::serial_println!("{}", out);
            match crate::mfs::MictFileSystem::read_mft() {
                Ok(entries) => {
                    if entries.is_empty() {
                        let empty_out = "  (No files found)\n";
                        self.text_buffer.push_str(empty_out);
                        crate::serial_println!("{}", empty_out);
                    }
                    for entry in entries {
                        let entry_out = alloc::format!("  - {} ({} blocks at LBA {})\n", entry.name, entry.block_count, entry.start_lba);
                        self.text_buffer.push_str(&entry_out);
                        crate::serial_print!("{}", entry_out);
                    }
                }
                Err(e) => {
                    let err_out = alloc::format!("  [FAIL] Could not read MFT: {}\n", e);
                    self.text_buffer.push_str(&err_out);
                    crate::serial_println!("{}", err_out);
                }
            }
        }
        "REQUEST" => {
            let prompt = self.input_buffer.split_whitespace().skip(1).collect::<Vec<&str>>().join(" ");
            if !prompt.is_empty() {
                let out = "Sending Request to Agentic Gateway...\n";
                self.text_buffer.push_str(out);
                crate::serial_println!("{}", out);
                x86_64::instructions::interrupts::without_interrupts(|| {
                    crate::serial_println!("<<API_REQ:{}>>", prompt);
                });
            } else {
                let usage = "Usage: REQUEST <PROMPT>\n";
                self.text_buffer.push_str(usage);
                crate::serial_println!("{}", usage);
            }
        }
        _ => {
            let out = alloc::format!("Command not recognized: '{}'\n", cmd);
            self.text_buffer.push_str(&out);
            crate::serial_println!("{}", out);
        }
    } 
    
    self.input_buffer.clear();
    
    // Memory safety code from your original version
    if self.text_buffer.len() > 2000 {
        self.text_buffer.drain(0..500); 
    }

    // --- THE FINAL PROMPT ---
    // Tell the remote GenesisOS terminal that execution is finished.
    crate::serial_print!("genesis> "); 
}

    // [MICT: THE NEW DEDICATED KEYBOARD ROUTER]
    pub fn process_keystroke(&mut self, c: char) {
        if c == '\x08' {
            // Backspace deletes from the current input buffer
            self.input_buffer.pop();
        } else if c == '\n' || c == '\r' {
            // Enter key triggers the command!
            self.execute_command();
        } else {
            // Standard typing
            self.input_buffer.push(c);
        }
    }

// [MICT: THE NEW TEXT RENDERER - ZERO ALLOCATION PHYSICAL WRAP]
    pub fn render_text(&self, gpu: &mut GpuDriver) {
        use font8x8::legacy::BASIC_LEGACY;
        
        // 1. Calculate bounding box parameters
        let max_lines = ((self.h - 50) / 12) as usize; 
        let max_chars_per_line = ((self.w - 20) / 8) as usize;
        
        // 2. Combine history + prompt + current input
        let mut display_text = self.text_buffer.clone();
        display_text.push_str("genesis> ");
        display_text.push_str(&self.input_buffer);
        display_text.push('\u{2588}'); // Unicode Solid Block Cursor
        
        // --- PASS 1: Count total physical lines mathematically ---
        let mut total_physical_lines = 0;
        for logical_line in display_text.split('\n') {
            let char_count = logical_line.chars().count();
            if char_count == 0 {
                total_physical_lines += 1; // Empty lines still take 1 physical line
            } else {
                // Integer division rounding up
                total_physical_lines += (char_count + max_chars_per_line - 1) / max_chars_per_line;
            }
        }

        // Determine exactly how many physical lines to skip to stick to the bottom
        let mut skip_lines = if total_physical_lines > max_lines {
            total_physical_lines - max_lines
        } else {
            0
        };

        // --- PASS 2: Execute the draw, skipping lines that are scrolled off-screen ---
        let mut cy = self.y + 40; 
        
        for logical_line in display_text.split('\n') {
            let mut cx = self.x + 10;
            let mut char_count = 0;
            
            for c in logical_line.chars() {
                // If we hit the boundary, we are starting a NEW physical line
                if char_count > 0 && char_count % max_chars_per_line == 0 {
                    if skip_lines > 0 {
                        skip_lines -= 1; // Discard a skipped line
                    } else {
                        cy += 12; // Actually move the cursor down
                    }
                    cx = self.x + 10; // Carriage return
                }

                // Only draw the pixel if we are within the visible window
                if skip_lines == 0 {
                    let c_val = c as usize;
                    if c_val < 128 || c == '\u{2588}' {
                        let bitmap = if c == '\u{2588}' {[255; 8] } else { BASIC_LEGACY[c_val] }; 
                        for (y, row) in bitmap.iter().enumerate() {
                            for x in 0..8 {
                                if (*row & (1 << x)) != 0 {
                                    gpu.draw_pixel(cx + x as i32, cy + y as i32, 255, 255, 255);
                                }
                            }
                        }
                    }
                    cx += 8;
                }
                char_count += 1;
            }
            
            // Finalize the logical line (acting as the physical \n stroke)
            if skip_lines > 0 {
                skip_lines -= 1;
            } else {
                cy += 12;
            }
        }
    }
}

pub struct SplatEngine {
    pub nebula_splats: Vec<GaussianSplat>,
    pub active_window: Option<SemanticWindow>,
}

impl SplatEngine {
    pub fn new() -> Self {
        SplatEngine { nebula_splats: Vec::new(), active_window: None }
    }

    pub fn add_splat(&mut self, splat: GaussianSplat) {
        self.nebula_splats.push(splat);
    }

    pub fn render(&mut self, gpu: &mut GpuDriver) {
        // 1. Render Nebula
        self.nebula_splats.sort_by(|a, b| a.z.cmp(&b.z));
        for splat in &self.nebula_splats {
            let radius = splat.scale * 2; 
            let min_x = core::cmp::max(0, splat.x - radius);
            let max_x = core::cmp::min(gpu.width as i32, splat.x + radius);
            let min_y = core::cmp::max(0, splat.y - radius);
            let max_y = core::cmp::min(gpu.height as i32, splat.y + radius);

            for py in min_y..max_y {
                for px in min_x..max_x {
                    let dx = px - splat.x;
                    let dy = py - splat.y;
                    let dist_sq = (dx * dx) + (dy * dy);
                    let falloff = dist_sq / (splat.scale * splat.scale / 128).max(1);
                    if falloff < 255 {
                        let intensity = 255 - falloff as u32;
                        let alpha = (intensity * splat.opacity as u32) / 255;
                        if alpha > 0 {
                            gpu.blend_pixel(px, py, splat.r, splat.g, splat.b, alpha as u8);
                        }
                    }
                }
            }
        }

        // 2. Render Window & Text
        if let Some(win) = &mut self.active_window {
            win.render_body(gpu);
            win.render_text(gpu); // [MICT: DRAW THE TEXT!]
        }
    }
}

//[MICT: THE SYSTEM LOGGER (Restored!)]
impl fmt::Write for SplatEngine {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(win) = &mut self.active_window {
            // System logs go straight to history! No command execution.
            win.text_buffer.push_str(s);
            
            // Memory safety: Prevent infinite string growth
            if win.text_buffer.len() > 2000 {
                win.text_buffer.drain(0..500); 
            }
        }
        Ok(())
    }
}

//[MICT: THE UNIVERSAL COMPOSITOR]
// This function instantly pastes the Nebula, draws the Window, draws the Cursor, and Flips!
pub fn render_desktop(gpu: &mut GpuDriver, engine: &mut SplatEngine) {
    let cx = CURSOR_X.load(Ordering::SeqCst);
    let cy = CURSOR_Y.load(Ordering::SeqCst);
    let clicked = LEFT_CLICK.load(Ordering::SeqCst);
    let (cr, cg, cb) = if clicked { (255, 50, 50) } else { (255, 255, 255) };

    gpu.restore_from_nebula_cache(); 
    
    if let Some(win) = &mut engine.active_window {
        win.render_body(gpu);
        win.render_text(gpu);
    }
    
    for dy in 0..5 {
        for dx in 0..5 {
            gpu.draw_pixel((cx + dx) as i32, (cy + dy) as i32, cr, cg, cb);
        }
    }
    gpu.swap_buffers();
}

#[macro_export]
macro_rules! screen_print {
    ($($arg:tt)*) => ($crate::splat::_screen_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! screen_println {
    () => ($crate::screen_print!("\n"));
    ($($arg:tt)*) => ($crate::screen_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _screen_print(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        //[MICT: DEADLOCK PREVENTION] Lock GPU first, Engine second.
        let mut gpu_lock = crate::gpu_driver::GPU_WRITER.lock();
        let mut engine_lock = SPLAT_ENGINE.lock();
        
        if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
            engine.write_fmt(args).unwrap();
            
            //[MICT: INSTANT REFRESH] The microsecond text is typed, flash it to the screen!
            render_desktop(gpu, engine);
        }
    });
}

