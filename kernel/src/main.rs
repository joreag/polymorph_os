#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(polymorph_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use polymorph_os::serial_println;
use polymorph_os::task::{Task, executor::MictExecutor, keyboard};
use core::panic::PanicInfo;
use bootloader_api::{entry_point, BootInfo, config::Mapping};



pub static BOOTLOADER_CONFIG: bootloader_api::BootloaderConfig = {
    let mut config = bootloader_api::BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};


entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use polymorph_os::memory::{self, BootInfoFrameAllocator};
    use polymorph_os::allocator;
    use x86_64::VirtAddr;
    use polymorph_os::gpu_driver::GpuDriver;

    // ==========================================
    // [1] CORE HARDWARE & INTERRUPTS
    // ==========================================
    serial_println!("[GENESIS OS] Initializing MICT Hardware Substrate...");
    polymorph_os::init();

    // ==========================================
    // [2] MICT MEMORY SUBSTRATE
    // ==========================================
    let physical_memory_offset = boot_info.physical_memory_offset.into_option().unwrap();
    let phys_mem_offset = VirtAddr::new(physical_memory_offset);
    
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_regions) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("[DISSONANCE] Fatal Error: MICT Heap initialization failed.");
    
    serial_println!("[GENESIS OS] MictGlobalAllocator (Heatmap) Online.");

    // ==========================================
    // [3] SEIZE THE GPU & INITIALIZE THE 3D UI
    // ==========================================
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let mut gpu = unsafe { GpuDriver::new(framebuffer) };
        gpu.clear_screen(10, 15, 25); // Deep space background
        
        use polymorph_os::splat::{SplatEngine, GaussianSplat};
        let mut engine = SplatEngine::new();

        // Background Nebula
        engine.add_splat(GaussianSplat { x: 400, y: 300, z: -100, scale: 200, r: 0, g: 150, b: 255, opacity: 100 });
        engine.add_splat(GaussianSplat { x: 800, y: 500, z: -100, scale: 250, r: 100, g: 0, b: 200, opacity: 80 });

        // Midground Window Formations
        engine.add_splat(GaussianSplat { x: 640, y: 400, z: 0, scale: 120, r: 0, g: 255, b: 180, opacity: 150 });
        engine.add_splat(GaussianSplat { x: 600, y: 380, z: 10, scale: 80, r: 255, g: 255, b: 255, opacity: 200 });

        // Foreground Action
        engine.add_splat(GaussianSplat { x: 700, y: 450, z: 50, scale: 60, r: 255, g: 50, b: 100, opacity: 220 });

                // Paint the 3D Splats to the physical 2D Framebuffer!
        engine.render(&mut gpu);

        // [MICT: ADD THIS LINE!] Take a snapshot of the heavy math!
        gpu.save_to_nebula_cache();

        // NOW add the window to the engine (so it isn't part of the background cache)
        engine.active_window = Some(polymorph_os::splat::SemanticWindow::new(240, 100, 800, 600));

        // Pass ownership to the global writer
        *polymorph_os::gpu_driver::GPU_WRITER.lock() = Some(gpu);
        *polymorph_os::splat::SPLAT_ENGINE.lock() = Some(engine);
        
        polymorph_os::screen_println!("========================================================");
        polymorph_os::screen_println!("[GENESIS OS] 3D GAUSSIAN SPLAT ENGINE ONLINE        ");
        polymorph_os::screen_println!("========================================================");
    }

    // ==========================================
    //[4] HARDWARE RADAR & NVMe STORAGE ENGINE
    // ==========================================
    polymorph_os::pci::enumerate_buses();
    polymorph_os::screen_println!("[MICT: MAP] PCIe Hardware Radar complete.");

    // Map the NVMe MMIO Space
    let nvme_physical_bar0 = 0x0000000800000000;
    let nvme_mmio_size = 0x4000; // 16 KB

    let nvme_virtual_addr = unsafe {
        polymorph_os::memory::map_mmio(
            nvme_physical_bar0, 
            nvme_mmio_size, 
            &mut mapper, 
            &mut frame_allocator
        ).expect("[DISSONANCE] Failed to map NVMe MMIO space!")
    };
    
    polymorph_os::screen_println!("[MICT: TRANSFORM] NVMe MMIO mapped (NO_CACHE) at: {:#X}", nvme_virtual_addr.as_u64());

    // Power Cycle & Configure NVMe Controller
        let mut nvme_drive = unsafe { 
        polymorph_os::nvme::NvmeController::new(nvme_virtual_addr.as_u64() as usize) 
    };
    
    nvme_drive.ping();
    nvme_drive.disable();
    nvme_drive.configure_and_enable(&mapper);
    nvme_drive.identify_controller(&mapper);
    nvme_drive.setup_io_queues(&mapper);

    //[MICT: LOCK THE HARD DRIVE INTO GLOBAL STATE]
    *polymorph_os::nvme::NVME_DRIVE.lock() = Some(nvme_drive);

    #[cfg(test)]
    test_main();

    // ==========================================
    // [5] THE ASYNC EVENT LOOP
    // ==========================================
    serial_println!("[GENESIS OS] Spawning Primordial Tasks...");
    
    let mut executor = MictExecutor::new();
    executor.spawn(Task::new(genesis_init_task()));
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.spawn(Task::new(desktop_compositor_task()));

    serial_println!("[GENESIS OS] Entering MICT Event Loop. System Sovereign.");
    
    // [MICT: PRE-FLIGHT RENDER] 
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut gpu_lock = polymorph_os::gpu_driver::GPU_WRITER.lock();
        let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
        if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
            gpu.clear_screen(10, 15, 25);
            
            //[MICT: THE GHOST WINDOW FIX] 
            // 1. Temporarily extract the window from the engine
            let temp_window = engine.active_window.take(); 
            
            // 2. Render ONLY the pure Nebula
            engine.render(gpu); 
            
            // 3. Save the pure Nebula to the cache
            gpu.save_to_nebula_cache(); 
            
            // 4. Put the window back into the engine
            engine.active_window = temp_window;

            // Now do a full desktop render (Nebula + Window + Text)
            polymorph_os::splat::render_desktop(gpu, engine);
        }
    });

    executor.run();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("\n<MICT_CYCLE_INTERRUPT>");
    serial_println!("  <CHECK_FAILURE>");
    serial_println!("[DISSONANCE DETECTED]: {}", info);
    serial_println!("    [ACTION]: Halting CPU to preserve memory state.");
    serial_println!("  </CHECK_FAILURE>");
    serial_println!("</MICT_CYCLE_INTERRUPT>");
    polymorph_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    polymorph_os::test_panic_handler(info)
}


/// The Primordial Background Task (The Neural Link)
async fn genesis_init_task() {
    polymorph_os::serial_println!("[KERNEL] Neural Link Task is online. Awaiting Payloads...");
    
    use polymorph_os::task::serial_stream::SerialStream;
    use futures_util::stream::StreamExt;
    use alloc::string::String;
    
    let mut serial_stream = SerialStream::new();
    let mut json_buffer = String::new();
    let mut receiving_json = false;
    let mut payload_count = 0;

    while let Some(byte) = serial_stream.next().await {
        let char = byte as char;
        
        // Only capture data if it starts with a JSON brace
        if char == '{' && !receiving_json {
            receiving_json = true;
            json_buffer.clear();
            json_buffer.push(char);
        } 
        else if receiving_json {
            json_buffer.push(char);
            
            // Wait for the backtick terminator
            if char == '`' {
                receiving_json = false;
                json_buffer.pop(); // Remove backtick

                // --- CASE 1: React UI Command ---
                if json_buffer.contains("\"command\"") {
                    if let Some(cmd_idx) = json_buffer.find("\"command\":\"") {
                        let start = cmd_idx + 11; 
                        if let Some(end) = json_buffer[start..].find("\"") {
                            let command_str = &json_buffer[start..start + end];
                            
                            let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
                            if let Some(win) = engine_lock.as_mut().and_then(|e| e.active_window.as_mut()) {
                                win.input_buffer.clear();
                                win.input_buffer.push_str(command_str);
                                win.input_buffer.push('\n'); // Trigger execution
                                win.execute_command();
                            }
                        }
                    }
                } 
                // --- CASE 2: AI Payload ---
                else if json_buffer.contains("\"payload\"") {
                    let mut filename;
                    loop {
                        filename = alloc::format!("payload_{}.mdo", payload_count);
                        if polymorph_os::mfs::MictFileSystem::find_file(&filename).is_none() { break; }
                        payload_count += 1;
                    }
                    
                    match polymorph_os::mfs::MictFileSystem::save_file(&filename, json_buffer.as_bytes()) {
                        Ok(_) => {
                            let out = alloc::format!("\n[KERNEL] AI Response saved to {}\ngenesis> ", filename);
                            polymorph_os::screen_println!("{}", out);
                            polymorph_os::serial_print!("{}", out); // Echo to React
                        },
                        Err(e) => {
                            let out = alloc::format!("\n[KERNEL] MFS SAVE FAILED: {}\ngenesis> ", e);
                            polymorph_os::screen_println!("{}", out);
                            polymorph_os::serial_print!("{}", out); // Echo to React
                        },
                    }
                }
                
                json_buffer.clear();
            }
        }
        // If receiving_json is false, we just drop the bytes. 
        // This naturally ignores the kernel's own serial echoes!
    }
}

    

async fn desktop_compositor_task() {
    use polymorph_os::task::mouse_stream::MouseStream;
    use futures_util::stream::StreamExt;
    use core::sync::atomic::Ordering;
    
    polymorph_os::serial_println!("[TASK] Desktop Compositor (Mouse/UI) Online.");
    let mut mouse_stream = MouseStream::new();
    
    let mut is_dragging = false;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;
    let mut was_left_click = false;

    let mut packet = [0u8; 3];
    let mut byte_idx = 0;

    while let Some(byte) = mouse_stream.next().await {
        if byte_idx == 0 {
            if (byte & 0x08) != 0 {
                packet[0] = byte;
                byte_idx += 1;
            }
        } else if byte_idx == 1 {
            packet[1] = byte;
            byte_idx += 1;
        } else if byte_idx == 2 {
            packet[2] = byte;
            byte_idx = 0; 

            let flags = packet[0];
            let mut x_mov = packet[1] as i32;
            let mut y_mov = packet[2] as i32;
            let left_click = (flags & 0x01) != 0;

            if (flags & 0x10) != 0 { x_mov |= !0xFF; }
            if (flags & 0x20) != 0 { y_mov |= !0xFF; }

            if x_mov != 0 || y_mov != 0 || left_click != was_left_click {
                
                // 1. Math Phase (Update Globals)
                let mut cx = polymorph_os::splat::CURSOR_X.load(Ordering::SeqCst);
                let mut cy = polymorph_os::splat::CURSOR_Y.load(Ordering::SeqCst);

                cx += x_mov;
                cy -= y_mov;
                cx = cx.clamp(0, 1274);
                cy = cy.clamp(0, 794);

                polymorph_os::splat::CURSOR_X.store(cx, Ordering::SeqCst);
                polymorph_os::splat::CURSOR_Y.store(cy, Ordering::SeqCst);
                polymorph_os::splat::LEFT_CLICK.store(left_click, Ordering::SeqCst);

                // [MICT: THE RENDER THROTTLE]
                // If there are MORE bytes waiting in the hardware queue, skip the 
                // heavy 4MB screen redraw and loop back to do the math instantly!
                if polymorph_os::task::mouse_stream::has_data() {
                    was_left_click = left_click;
                    continue; 
                }

                // 2. State Machine & Rendering Phase
                // This ONLY executes when we have fully caught up to the present moment!
                x86_64::instructions::interrupts::without_interrupts(|| {
                    let mut gpu_lock = polymorph_os::gpu_driver::GPU_WRITER.lock();
                    let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
                    
                    if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
                        
                        if left_click && !was_left_click {
                            if let Some(win) = &mut engine.active_window {
                                if cx >= win.x && cx <= win.x + win.w && cy >= win.y && cy <= win.y + 40 {
                                    is_dragging = true;
                                    drag_offset_x = cx - win.x;
                                    drag_offset_y = cy - win.y;
                                }
                            }
                        }
                        if !left_click { is_dragging = false; }

                        if is_dragging {
                            if let Some(win) = &mut engine.active_window {
                                win.move_to(cx - drag_offset_x, cy - drag_offset_y);
                            }
                        }
                        was_left_click = left_click;

                        // 3. Render Phase (Universal Compositor)
                        polymorph_os::splat::render_desktop(gpu, engine);
                    }
                });
            }
        }
    }
}