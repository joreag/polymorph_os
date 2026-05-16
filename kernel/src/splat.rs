// kernel/src/splat.rs

use crate::gpu_driver::GpuDriver;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::fmt;
use core::sync::atomic::{AtomicI32, AtomicBool, Ordering};

pub static SPLAT_ENGINE: Mutex<Option<SplatEngine>> = Mutex::new(None);
pub static DIRTY_SCREEN: AtomicBool = AtomicBool::new(true);

// [MICT: GLOBAL UI STATE]
pub static CURSOR_X: AtomicI32 = AtomicI32::new(400);
pub static CURSOR_Y: AtomicI32 = AtomicI32::new(300);
pub static LEFT_CLICK: AtomicBool = AtomicBool::new(false);
pub static IS_DRAGGING: AtomicBool = AtomicBool::new(false);
pub static DRAG_OFFSET_X: AtomicI32 = AtomicI32::new(0);
pub static DRAG_OFFSET_Y: AtomicI32 = AtomicI32::new(0);

pub static SCREEN_WIDTH: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(800);
pub static SCREEN_HEIGHT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(600);

#[derive(Clone, Copy)]
pub struct GaussianSplat {
    pub x: i32, pub y: i32, pub z: i32, 
    pub scale_x: i32, pub scale_y: i32, // The Anisotropic Upgrade!
    pub r: u8, pub g: u8, pub b: u8, pub opacity: u8,
}

/// [MICT: TRUE 3D HOLOGRAPHIC RENDERER]
pub fn render_single_splat(gpu: &mut GpuDriver, splat: &GaussianSplat) {
    // --- 1. PERSPECTIVE PROJECTION ---
    let focal_length = 500;
    let camera_z = -500; // The camera is sitting 500 units "in front" of the screen
    
    let z_dist = splat.z - camera_z;
    if z_dist <= 0 { return; } // Splat is behind the camera, don't draw it!
    
    let screen_cx = gpu.width as i32 / 2;
    let screen_cy = gpu.height as i32 / 2;

    // Project 3D coordinates into 2D screen space based on distance
    let proj_x = screen_cx + ((splat.x - screen_cx) * focal_length) / z_dist;
    let proj_y = screen_cy + ((splat.y - screen_cy) * focal_length) / z_dist;
    
    let proj_scale_x = (splat.scale_x * focal_length) / z_dist;
    let proj_scale_y = (splat.scale_y * focal_length) / z_dist;

    // --- 2. BOUNDING BOX ---
    let radius_x = proj_scale_x * 2;
    let radius_y = proj_scale_y * 2;
    
    let min_x = core::cmp::max(0, proj_x - radius_x);
    let max_x = core::cmp::min(gpu.width as i32, proj_x + radius_x);
    let min_y = core::cmp::max(0, proj_y - radius_y);
    let max_y = core::cmp::min(gpu.height as i32, proj_y + radius_y);

    // --- 3. ANISOTROPIC COVARIANCE (The Ellipse Math) ---
    for py in min_y..max_y {
        for px in min_x..max_x {
            let dx = px - proj_x;
            let dy = py - proj_y;
            
            let scale_fac_x = (proj_scale_x * proj_scale_x / 128).max(1);
            let scale_fac_y = (proj_scale_y * proj_scale_y / 128).max(1);
            
            // Calculates an ellipse instead of a perfect circle!
            let falloff = (dx * dx) / scale_fac_x + (dy * dy) / scale_fac_y;
            
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

// --- [MICT: THE INSTANTIATION STATE MACHINE] ---
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowMode {
    Instantiated, // Normal floating window
    Maximized,    // Fullscreen
    Dead,         // Ready to be deleted from RAM
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowState {
    Stable(WindowMode),
    Transitioning { 
        progress: i32, max: i32, 
        s_w: i32, s_h: i32, s_x: i32, s_y: i32, // Starting geometry
        t_w: i32, t_h: i32, t_x: i32, t_y: i32, // Target geometry
        final_mode: WindowMode                  // What to become when done
    },
}

pub struct SemanticWindow {
    pub id: u32,
    pub x: i32, pub y: i32, pub w: i32, pub h: i32, 
    pub normal_x: i32, pub normal_y: i32, pub normal_w: i32, pub normal_h: i32, 
    pub base_z: i32, pub target_z: i32, // --- NEW: 3D Depth Tracking ---
    pub state: WindowState,
    pub text_buffer: String,
    pub input_buffer: String,
    pub splat_cloud: Vec<GaussianSplat>,
}

impl SemanticWindow {
    /// Spawn a brand new window from a specific (x,y) origin
    pub fn spawn(id: u32, origin_x: i32, origin_y: i32, target_w: i32, target_h: i32) -> Self {
    let target_x = origin_x - (target_w / 2);
    let target_y = origin_y - (target_h / 2);
    
    let mut win = SemanticWindow { 
        id, x: origin_x, y: origin_y, w: 40, h: 40, 
        normal_x: target_x, normal_y: target_y, normal_w: target_w, normal_h: target_h,
        base_z: 0, target_z: 0, // Initialize at the front of the glass!
            state: WindowState::Transitioning { 
                progress: 0, max: 15, 
                s_w: 40, s_h: 40, s_x: origin_x, s_y: origin_y,
                t_w: target_w, t_h: target_h, t_x: target_x, t_y: target_y,
                final_mode: WindowMode::Instantiated
            },
            text_buffer: alloc::format!("Instance {} Instantiated.\n", id),
            input_buffer: String::new(),
            splat_cloud: Vec::new(),
        };
        win.generate_cloud();
        win
    }

    pub fn update_physics(&mut self) -> bool {
    let mut redrew = false;

        // --- THE Z-AXIS PARALLAX PHYSICS ---
        if self.base_z != self.target_z {
            let diff = self.target_z - self.base_z;
            self.base_z += diff / 4; // Smooth slide into the background
            if diff.abs() < 5 { self.base_z = self.target_z; }
            self.generate_cloud();
            redrew = true;
        }
        if let WindowState::Transitioning { progress, max, s_w, s_h, s_x, s_y, t_w, t_h, t_x, t_y, final_mode } = self.state {
            let p = progress + 1;
            // Integer Ease-Out
            let factor = (p * (2 * max - p) * 100) / (max * max);
            
            self.w = s_w + ((t_w - s_w) * factor) / 100;
            self.h = s_h + ((t_h - s_h) * factor) / 100;
            self.x = s_x + ((t_x - s_x) * factor) / 100;
            self.y = s_y + ((t_y - s_y) * factor) / 100;

            if p >= max {
                self.w = t_w; self.h = t_h; self.x = t_x; self.y = t_y;
                self.state = WindowState::Stable(final_mode);
            } else {
                self.state = WindowState::Transitioning { progress: p, max, s_w, s_h, s_x, s_y, t_w, t_h, t_x, t_y, final_mode };
            }
            self.generate_cloud();
            redrew = true; 
    }
    
    redrew
}

pub fn generate_cloud(&mut self) {
    self.splat_cloud.clear();
    let cx = self.x + (self.w / 2);
    let cy = self.y + (self.h / 2);
    let core_scale = (self.w / 4).min(self.h / 4).max(10);
    
    // Notice how we add self.base_z to the Z coordinates!
    let padding = core_scale; 
    self.splat_cloud.push(GaussianSplat { x: cx, y: cy, z: self.base_z + 10, scale_x: (self.w / 2).max(10), scale_y: (self.h / 2).max(10), r: 5, g: 10, b: 15, opacity: 220 });

    let thickness = 4;
    // Top border
    self.splat_cloud.push(GaussianSplat { x: cx, y: self.y, z: self.base_z + 12, scale_x: self.w / 2, scale_y: thickness, r: 0, g: 255, b: 200, opacity: 120 }); 
    // Bottom border
    self.splat_cloud.push(GaussianSplat { x: cx, y: self.y + self.h, z: self.base_z + 12, scale_x: self.w / 2, scale_y: thickness, r: 0, g: 100, b: 255, opacity: 90 }); 
    // Left border
    self.splat_cloud.push(GaussianSplat { x: self.x, y: cy, z: self.base_z + 12, scale_x: thickness, scale_y: self.h / 2, r: 0, g: 150, b: 255, opacity: 90 }); 
    // Right border
    self.splat_cloud.push(GaussianSplat { x: self.x + self.w, y: cy, z: self.base_z + 12, scale_x: thickness, scale_y: self.h / 2, r: 0, g: 150, b: 255, opacity: 90 });

    // Buttons
    if let WindowState::Stable(mode) = self.state {
        if mode != WindowMode::Dead {
            self.splat_cloud.push(GaussianSplat { x: self.x + self.w - 25, y: self.y + 15, z: self.base_z + 14, scale_x: 12, scale_y: 12, r: 255, g: 50, b: 50, opacity: 200 });
            self.splat_cloud.push(GaussianSplat { x: self.x + self.w - 55, y: self.y + 15, z: self.base_z + 14, scale_x: 12, scale_y: 12, r: 255, g: 200, b: 50, opacity: 200 });
            self.splat_cloud.push(GaussianSplat { x: self.x + self.w - 85, y: self.y + 15, z: self.base_z + 14, scale_x: 12, scale_y: 12, r: 50, g: 255, b: 100, opacity: 200 });
        }
    }
}

    // Action Triggers
    pub fn trigger_close(&mut self, launcher_x: i32, launcher_y: i32) {
        self.state = WindowState::Transitioning { 
            progress: 0, max: 15, 
            s_w: self.w, s_h: self.h, s_x: self.x, s_y: self.y,
            t_w: 40, t_h: 40, t_x: launcher_x, t_y: launcher_y, // Shrink back to launcher
            final_mode: WindowMode::Dead 
        };
    }

    pub fn trigger_maximize(&mut self, screen_w: i32, screen_h: i32) {
        // Save current geometry so we can restore later!
        if self.state == WindowState::Stable(WindowMode::Instantiated) {
            self.normal_x = self.x; self.normal_y = self.y; 
            self.normal_w = self.w; self.normal_h = self.h;
        }
        self.state = WindowState::Transitioning { 
            progress: 0, max: 15, 
            s_w: self.w, s_h: self.h, s_x: self.x, s_y: self.y,
            t_w: screen_w, t_h: screen_h, t_x: 0, t_y: 0,
            final_mode: WindowMode::Maximized 
        };
    }

    pub fn trigger_restore(&mut self) {
        self.state = WindowState::Transitioning { 
            progress: 0, max: 15, 
            s_w: self.w, s_h: self.h, s_x: self.x, s_y: self.y,
            t_w: self.normal_w, t_h: self.normal_h, t_x: self.normal_x, t_y: self.normal_y,
            final_mode: WindowMode::Instantiated 
        };
    }


    pub fn move_to(&mut self, new_x: i32, new_y: i32) {
        self.x = new_x;
        self.y = new_y;
        self.generate_cloud();
    }

    pub fn render_body(&self, gpu: &mut GpuDriver) {
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
                // ... NVMe code ...
            } else if arg1 == "ROUTER" {
                let out = "[MICT] Forging ICMP Echo Request...\n";
                self.text_buffer.push_str(out);
                crate::serial_println!("{}", out);
                
                if let Some(nic) = crate::e1000::E1000_NET.lock().as_mut() {
                    unsafe {
                        // Target the Default QEMU Router MAC Address!
                        nic.send_ping([0x52, 0x54, 0x00, 0x12, 0x34, 0x56],[10, 0, 2, 2], [10, 0, 2, 15]);
                    }
                }
            } else {
                let usage = "Usage: PING[NVME | ROUTER]\n";
                self.text_buffer.push_str(usage);
                crate::serial_println!("{}", usage);
            }
        }
        // Put this right below your PING match arm
        "ARP" => {
            let out = "[MICT] Broadcasting ARP Request to Virtual Router...\n";
            self.text_buffer.push_str(out);
            crate::serial_println!("{}", out);
            
            if let Some(nic) = crate::e1000::E1000_NET.lock().as_mut() {
                unsafe {
                    // Ask "Who has 10.0.2.2?" from "10.0.2.15"
                    nic.send_arp_request([10, 0, 2, 2], [10, 0, 2, 15]);
                }
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
            
            // [MICT: THE IDENTITY BADGE]
            // Construct the execution context for the Virtual Machine.
            // We use 0xAA to simulate the authorized "System_Root" matching the file owner.
            let terminal_context = crate::mdo_vm::MdoContext {
                requestor_id_hash: [0xAA; 32], // I am the authorized owner!
                action_hash: [0x00; 32], 
                owner_id_hash: [0x00; 32], 
                status: 0,
            };

            // Pass the context badge to the MFS gate!
            match crate::mfs::MictFileSystem::read_file(arg1, &terminal_context) {
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
        match self.state {
        WindowState::Stable(WindowMode::Instantiated) | WindowState::Stable(WindowMode::Maximized) => {},
        _ => return, // Hide text during expanding/shrinking!
    } 
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


pub struct AppLauncher {
    pub x: i32, pub y: i32, pub radius: i32,
}

pub struct SplatEngine {
    pub nebula_splats: Vec<GaussianSplat>,
    pub launchers: Vec<AppLauncher>,
    pub windows: Vec<SemanticWindow>,
    pub instance_counter: u32,
}

impl SplatEngine {
    pub fn new() -> Self {
        SplatEngine { 
            nebula_splats: Vec::new(), 
            launchers: Vec::new(),
            windows: Vec::new(),
            instance_counter: 1,
        }
    }

    pub fn tick_physics(&mut self) -> bool {
        let mut redrew = false;
        for win in &mut self.windows {
            if win.update_physics() { redrew = true; }
        }
        // Garbage collection! Remove windows that have finished shrinking to the Dead state.
        self.windows.retain(|w| w.state != WindowState::Stable(WindowMode::Dead));
        redrew
    }
    
    // ... (Keep existing render function, just loop over self.windows instead of active_window)
    pub fn render(&mut self, gpu: &mut GpuDriver) {
        self.nebula_splats.sort_by(|a, b| a.z.cmp(&b.z));
        for splat in &self.nebula_splats { render_single_splat(gpu, splat); }

        // Render Launchers
    for l in &self.launchers {
        render_single_splat(gpu, &GaussianSplat { 
            x: l.x, y: l.y, z: 10, 
            scale_x: l.radius, scale_y: l.radius, // <--- THE FIX
            r: 0, g: 255, b: 200, opacity: 200 
        });
        render_single_splat(gpu, &GaussianSplat { 
            x: l.x, y: l.y, z: 11, 
            scale_x: l.radius/2, scale_y: l.radius/2, // <--- THE FIX
            r: 255, g: 255, b: 255, opacity: 255 
        });
    }
        for win in &mut self.windows {
            win.render_body(gpu);
            if let WindowState::Stable(_) = win.state { win.render_text(gpu); }
        }
    }
}

//[MICT: THE UNIVERSAL COMPOSITOR]
pub fn render_desktop(gpu: &mut GpuDriver, engine: &mut SplatEngine) {
    let cx = CURSOR_X.load(Ordering::SeqCst);
    let cy = CURSOR_Y.load(Ordering::SeqCst);
    let clicked = LEFT_CLICK.load(Ordering::SeqCst);
    let (cr, cg, cb) = if clicked { (255, 50, 50) } else { (255, 255, 255) };

    let sw = SCREEN_WIDTH.load(Ordering::SeqCst);
    let sh = SCREEN_HEIGHT.load(Ordering::SeqCst);
    
    // [FIXED] Declare our tracking variables
    let mut click_handled = false;
    let mut clicked_win_idx = None;

    if clicked {
        // 1. Find which window we clicked (iterate backwards to click top-most)
        for (i, win) in engine.windows.iter_mut().enumerate().rev() {
            if let WindowState::Stable(_) = win.state {
                let close_x = win.x + win.w - 25;
                let rest_x = win.x + win.w - 55;
                let max_x = win.x + win.w - 85;
                let btn_y = win.y + 15;

                // Close Button
                if (cx - close_x).abs() < 15 && (cy - btn_y).abs() < 15 {
                    win.trigger_close(engine.launchers[0].x, engine.launchers[0].y);
                    click_handled = true; 
                    clicked_win_idx = Some(i);
                    break;
                }
                // Restore Button
                else if (cx - rest_x).abs() < 15 && (cy - btn_y).abs() < 15 {
                    win.trigger_restore(); 
                    click_handled = true; 
                    clicked_win_idx = Some(i);
                    break;
                }
                // Maximize Button
                else if (cx - max_x).abs() < 15 && (cy - btn_y).abs() < 15 {
                    win.trigger_maximize(sw, sh); 
                    click_handled = true; 
                    clicked_win_idx = Some(i);
                    break;
                }
                // Window Dragging
                else if cx >= win.x && cx <= win.x + win.w && cy >= win.y && cy <= win.y + 40 {
                    IS_DRAGGING.store(true, Ordering::SeqCst);
                    DRAG_OFFSET_X.store(cx - win.x, Ordering::SeqCst);
                    DRAG_OFFSET_Y.store(cy - win.y, Ordering::SeqCst);
                    click_handled = true; 
                    clicked_win_idx = Some(i);
                    break;
                }
            }
            
            // [NEW] If they just clicked the body of the window to focus it:
            if cx >= win.x && cx <= win.x + win.w && cy >= win.y && cy <= win.y + win.h {
                clicked_win_idx = Some(i);
                click_handled = true;
                break;
            }
        }

        // 2. Window Focus: Pop the clicked window out, push it to the end!
        if let Some(idx) = clicked_win_idx {
            let focused_win = engine.windows.remove(idx);
            engine.windows.push(focused_win);
        } 
        // 3. Check Launchers if no window was clicked
        else if !click_handled && !IS_DRAGGING.load(Ordering::SeqCst) {
            for l in &engine.launchers {
                if (cx - l.x).abs() < l.radius && (cy - l.y).abs() < l.radius {
                    let new_win = SemanticWindow::spawn(engine.instance_counter, l.x, l.y, 800, 600);
                    engine.windows.push(new_win);
                    engine.instance_counter += 1;
                    break;
                }
            }
        }
    } else {
        IS_DRAGGING.store(false, Ordering::SeqCst);
    }

    // --- [NEW] THE 3D PARALLAX MAGIC ---
    // Top window comes to Z=0. Background windows sink to Z=300.
    let win_count = engine.windows.len();
    for (i, win) in engine.windows.iter_mut().enumerate() {
        if i == win_count.saturating_sub(1) {
            win.target_z = 0;   // Come to the front!
        } else {
            win.target_z = 300; // Sink into the background!
        }
    }

    // Apply drag to the *last* window in the vector (the top one)
    if IS_DRAGGING.load(Ordering::SeqCst) {
        if let Some(win) = engine.windows.last_mut() {
            let off_x = DRAG_OFFSET_X.load(Ordering::SeqCst);
            let off_y = DRAG_OFFSET_Y.load(Ordering::SeqCst);
            win.x = cx - off_x; 
            win.y = cy - off_y;
            win.generate_cloud();
        }
    }
// --- [FIXED] THE MISSING RENDER COMMANDS! ---
    
    // 1. Advance the math (handles expansion/shrinking/Z-depth)
    engine.tick_physics(); 

    // 2. Draw the background cache
    gpu.restore_from_nebula_cache(); 
    
    // 3. Draw the launchers and windows
    engine.render(gpu);
    
    // 4. Draw the Cursor
    for dy in 0..5 {
        for dx in 0..5 { 
            gpu.draw_pixel((cx + dx) as i32, (cy + dy) as i32, cr, cg, cb); 
        }
    }
    
    // 5. PUSH TO HARDWARE!
    gpu.swap_buffers();
} 

//[MICT: THE SYSTEM LOGGER]
impl fmt::Write for SplatEngine {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // [FIXED] Send system logs to the top-most (focused) window!
        if let Some(win) = self.windows.last_mut() {
            win.text_buffer.push_str(s);
            
            if win.text_buffer.len() > 2000 {
                win.text_buffer.drain(0..500); 
            }
        }
        Ok(())
    }
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

