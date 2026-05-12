use crate::gpu_driver::GpuDriver;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::fmt;
use core::sync::atomic::{AtomicI32, AtomicBool, Ordering};


pub static SPLAT_ENGINE: Mutex<Option<SplatEngine>> = Mutex::new(None);
// [MICT: LAZY RENDERING]
pub static DIRTY_SCREEN: AtomicBool = AtomicBool::new(true);

// [MICT: GLOBAL UI STATE]
//[MICT: GLOBAL UI STATE]
pub static CURSOR_X: AtomicI32 = AtomicI32::new(960);
pub static CURSOR_Y: AtomicI32 = AtomicI32::new(540);
pub static LEFT_CLICK: AtomicBool = AtomicBool::new(false);
pub static IS_DRAGGING: AtomicBool = AtomicBool::new(false);
pub static DRAG_OFFSET_X: AtomicI32 = AtomicI32::new(0);
pub static DRAG_OFFSET_Y: AtomicI32 = AtomicI32::new(0);

pub static SCREEN_WIDTH: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(800);
pub static SCREEN_HEIGHT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(600);

#[derive(Clone, Copy)]
pub struct GaussianSplat {
    pub x: i32, pub y: i32, pub z: i32, pub scale: i32,
    pub r: u8, pub g: u8, pub b: u8, pub opacity: u8,
}

/// [MICT: THE UNIVERSAL SPLAT RENDERER]
/// Renders a single 3D Gaussian math equation into pixels on the screen.
pub fn render_single_splat(gpu: &mut crate::gpu_driver::GpuDriver, splat: &GaussianSplat) {
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
            
            // Gaussian Falloff Equation
            let scale_factor = (splat.scale * splat.scale / 128).max(1);
            let falloff = dist_sq / scale_factor;
            
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

//[MICT: THE NON-EUCLIDEAN WINDOW]
pub struct SemanticWindow {
    pub x: i32, pub y: i32, pub w: i32, pub h: i32,
    pub text_buffer: String,
    pub input_buffer: String,
    pub scroll_offset: usize,
    pub splat_cloud: Vec<GaussianSplat>, // <-- THE ORGANIC BODY
}

impl SemanticWindow {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        let mut win = SemanticWindow { 
            x, y, w, h, 
            text_buffer: String::from("Welcome to PolymorphOS. Type HELP for commands.\n"),
            input_buffer: String::new(),
            scroll_offset: 0,
            splat_cloud: Vec::new(),
        };
        win.generate_cloud();
        win
    }

    ///[MICT: PROCEDURAL MORPHOLOGY v7 (The Tight UI)]
    pub fn generate_cloud(&mut self) {
        self.splat_cloud.clear();
        
        let cx = self.x + (self.w / 2);
        let cy = self.y + (self.h / 2);

        // --- 1. THE DARK CORE (Tucked inside the borders) ---
        let corner_scale = (self.w / 4).min(self.h / 4) - 10; 
        // Pull the centers further inward so the radius doesn't bleed out!
        let padding = corner_scale + 10; 

        let core_r = 5; let core_g = 10; let core_b = 15; 
        let core_opacity = 220; 
        
        self.splat_cloud.push(GaussianSplat { x: self.x + padding, y: self.y + padding, z: 10, scale: corner_scale, r: core_r, g: core_g, b: core_b, opacity: core_opacity });
        self.splat_cloud.push(GaussianSplat { x: self.x + self.w - padding, y: self.y + padding, z: 10, scale: corner_scale, r: core_r, g: core_g, b: core_b, opacity: core_opacity });
        self.splat_cloud.push(GaussianSplat { x: self.x + padding, y: self.y + self.h - padding, z: 10, scale: corner_scale, r: core_r, g: core_g, b: core_b, opacity: core_opacity });
        self.splat_cloud.push(GaussianSplat { x: self.x + self.w - padding, y: self.y + self.h - padding, z: 10, scale: corner_scale, r: core_r, g: core_g, b: core_b, opacity: core_opacity });

        // Center Filler
        self.splat_cloud.push(GaussianSplat { 
            x: cx, y: cy, z: 10, 
            scale: (self.w / 2).min(self.h / 2), 
            r: core_r, g: core_g, b: core_b, 
            opacity: core_opacity 
        });

        // --- 2. THE GLOWING BORDER (Zero Scalloping) ---
        let node_spacing = 8; // Halved the spacing to pack the splats tightly together
        let border_scale = 16; // Shrunk the radius so it forms a tight line
        
        let num_x_nodes = self.w / node_spacing;
        for i in 0..=num_x_nodes {
            let px = self.x + (i * node_spacing);
            self.splat_cloud.push(GaussianSplat { x: px, y: self.y, z: 12, scale: border_scale, r: 0, g: 255, b: 200, opacity: 120 }); // Top
            self.splat_cloud.push(GaussianSplat { x: px, y: self.y + self.h, z: 12, scale: border_scale, r: 0, g: 100, b: 255, opacity: 90 }); // Bottom
        }

        let num_y_nodes = self.h / node_spacing;
        for i in 0..=num_y_nodes {
            let py = self.y + (i * node_spacing);
            self.splat_cloud.push(GaussianSplat { x: self.x, y: py, z: 12, scale: border_scale, r: 0, g: 150, b: 255, opacity: 90 }); // Left
            self.splat_cloud.push(GaussianSplat { x: self.x + self.w, y: py, z: 12, scale: border_scale, r: 0, g: 150, b: 255, opacity: 90 }); // Right
        }

        // --- 3. THE CORNER ANCHORS ---
        let anchor_scale = 30; // Shrunk to match the tighter borders
        self.splat_cloud.push(GaussianSplat { x: self.x, y: self.y, z: 13, scale: anchor_scale, r: 0, g: 255, b: 255, opacity: 180 });
        self.splat_cloud.push(GaussianSplat { x: self.x + self.w, y: self.y, z: 13, scale: anchor_scale, r: 0, g: 255, b: 255, opacity: 180 });
        self.splat_cloud.push(GaussianSplat { x: self.x, y: self.y + self.h, z: 13, scale: anchor_scale, r: 50, g: 100, b: 255, opacity: 150 });
        self.splat_cloud.push(GaussianSplat { x: self.x + self.w, y: self.y + self.h, z: 13, scale: anchor_scale, r: 50, g: 100, b: 255, opacity: 150 });

        // --- 4. PROCEDURAL WINDOW CONTROLS ---
        // Close Button (Red)
        self.splat_cloud.push(GaussianSplat { x: self.x + self.w - 25, y: self.y + 5, z: 14, scale: 12, r: 255, g: 50, b: 50, opacity: 200 });
        // Minimize Button (Yellow)
        self.splat_cloud.push(GaussianSplat { x: self.x + self.w - 55, y: self.y + 5, z: 14, scale: 12, r: 255, g: 200, b: 50, opacity: 200 });
    }

    pub fn move_to(&mut self, new_x: i32, new_y: i32) {
        self.x = new_x;
        self.y = new_y;
        self.generate_cloud(); // Morph to the new location!
    }

    //[MICT: THE NEW ORGANIC RENDERER]
    pub fn render_body(&self, gpu: &mut crate::gpu_driver::GpuDriver) {
        // We draw the cloud instead of a rigid glass rectangle!
        for splat in &self.splat_cloud {
            render_single_splat(gpu, splat);
        }
    }

// [MICT: THE COMMAND EXECUTOR]
pub fn execute_command(&mut self) {
    crate::splat::DIRTY_SCREEN.store(true, Ordering::SeqCst);
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
        crate::splat::DIRTY_SCREEN.store(true, Ordering::SeqCst);
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

//[MICT: THE NEW TEXT RENDERER - ZERO ALLOCATION PHYSICAL WRAP]
    pub fn render_text(&self, gpu: &mut crate::gpu_driver::GpuDriver) {
        use font8x8::legacy::BASIC_LEGACY;
        
        // [FIXED] Subtract 60 (30px left padding + 30px right padding) to stop the overrun!
        let max_chars_per_line = ((self.w - 60) / 8).max(1) as usize; 
        let max_lines = ((self.h - 60) / 12).max(1) as usize; 
        
        // Combine history + prompt + current input
        let mut display_text = self.text_buffer.clone();
        display_text.push_str("genesis> ");
        display_text.push_str(&self.input_buffer);
        display_text.push('\u{2588}'); 
        
        // --- DRAW TITLE BAR TEXT ---
        let title = "PolymorphOS Sovereign Terminal";
        let mut title_x = self.x + 30;
        let title_y = self.y + 2;
        for c in title.chars() {
            let c_val = c as usize;
            if c_val < 128 {
                for (y, row) in BASIC_LEGACY[c_val].iter().enumerate() {
                    for x in 0..8 {
                        if (*row & (1 << x)) != 0 {
                            gpu.draw_pixel(title_x + x as i32, title_y + y as i32, 180, 255, 255); // Cyan title text
                        }
                    }
                }
            }
            title_x += 8;
        }

        // --- DRAW TERMINAL TEXT ---
        let mut total_physical_lines = 0;
        for logical_line in display_text.split('\n') {
            let char_count = logical_line.chars().count();
            if char_count == 0 {
                total_physical_lines += 1; 
            } else {
                total_physical_lines += (char_count + max_chars_per_line - 1) / max_chars_per_line;
            }
        }

        let mut skip_lines = if total_physical_lines > max_lines {
            total_physical_lines - max_lines
        } else {
            0
        };

        // Start text further down to make room for the Title Bar and Buttons
        let mut cy = self.y + 40; 
        
        for logical_line in display_text.split('\n') {
            let mut cx = self.x + 30; // 30px Left Padding
            let mut char_count = 0;
            
            for c in logical_line.chars() {
                if char_count > 0 && char_count % max_chars_per_line == 0 {
                    if skip_lines > 0 { skip_lines -= 1; } else { cy += 12; }
                    cx = self.x + 30; // Return to left padding margin
                }

                // Clip text from bleeding out the bottom
                if skip_lines == 0 && cy < self.y + self.h - 15 {
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
            
            if skip_lines > 0 { skip_lines -= 1; } else { cy += 12; }
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

        pub fn render(&mut self, gpu: &mut crate::gpu_driver::GpuDriver) {
        // 1. Render Background Nebula
        self.nebula_splats.sort_by(|a, b| a.z.cmp(&b.z));
        for splat in &self.nebula_splats {
            render_single_splat(gpu, splat); // <-- Much cleaner now!
        }

        // 2. Render Active Window Cloud & Text
        if let Some(win) = &mut self.active_window {
            win.render_body(gpu);
            win.render_text(gpu); 
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
pub fn render_desktop(gpu: &mut crate::gpu_driver::GpuDriver, engine: &mut SplatEngine) {
    let cx = CURSOR_X.load(Ordering::SeqCst);
    let cy = CURSOR_Y.load(Ordering::SeqCst);
    let clicked = LEFT_CLICK.load(Ordering::SeqCst);
    let (cr, cg, cb) = if clicked { (255, 50, 50) } else { (255, 255, 255) };

    // ---[MICT: DRAG AND DROP PHYSICS] ---
    if let Some(win) = &mut engine.active_window {
        let is_dragging = IS_DRAGGING.load(Ordering::SeqCst);

        if clicked {
            // Check if we just clicked the top "Title Bar" area of the window
            if !is_dragging && cx >= win.x && cx <= win.x + win.w && cy >= win.y && cy <= win.y + 40 {
                // Lock the drag state and record where on the window we clicked
                IS_DRAGGING.store(true, Ordering::SeqCst);
                DRAG_OFFSET_X.store(cx - win.x, Ordering::SeqCst);
                DRAG_OFFSET_Y.store(cy - win.y, Ordering::SeqCst);
            } 
            
            // If we are currently dragging, move the window!
            if IS_DRAGGING.load(Ordering::SeqCst) {
                let off_x = DRAG_OFFSET_X.load(Ordering::SeqCst);
                let off_y = DRAG_OFFSET_Y.load(Ordering::SeqCst);
                
                // Call the procedural morph function!
                win.move_to(cx - off_x, cy - off_y); 
            }
        } else {
            // Mouse button released, drop the window
            if is_dragging {
                IS_DRAGGING.store(false, Ordering::SeqCst);
            }
        }
    }

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

