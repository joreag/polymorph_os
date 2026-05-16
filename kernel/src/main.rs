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
    engine.nebula_splats.push(GaussianSplat { x: 400, y: 300, z: -100, scale_x: 200, scale_y: 200,r: 0, g: 150, b: 255, opacity: 100 });
    engine.nebula_splats.push(GaussianSplat { x: 800, y: 500, z: -100, scale_x: 250, scale_y:250, r: 100, g: 0, b: 200, opacity: 80 });

    // Midground Window Formations
    engine.nebula_splats.push(GaussianSplat { x: 640, y: 400, z: 0, scale_x: 120, scale_y:120, r: 0, g: 255, b: 180, opacity: 150 });
    engine.nebula_splats.push(GaussianSplat { x: 600, y: 380, z: 10, scale_x: 80, scale_y:80, r: 255, g: 255, b: 255, opacity: 200 });

    // Foreground Action
    engine.nebula_splats.push(GaussianSplat { x: 700, y: 450, z: 50, scale_x: 60, scale_y:60, r: 255, g: 50, b: 100, opacity: 220 });

        // Paint the 3D Splats to the physical 2D Framebuffer!
        engine.render(&mut gpu);

        // Take a snapshot of the heavy math!
        gpu.save_to_nebula_cache();

        // [FIXED] Spawn the App Launcher (The Orb)
        engine.launchers.push(polymorph_os::splat::AppLauncher { 
            x: 960, y: 1000, radius: 25 
        });

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
    polymorph_os::pci::enumerate_buses(&mut mapper, &mut frame_allocator, phys_mem_offset);
    polymorph_os::screen_println!("[MICT: MAP] PCIe Hardware Radar complete.");

    // 1. Write the forged binary file directly to the drive (Gracefully!)
    if let Err(e) = polymorph_os::mfs::MictFileSystem::save_file("vault.mdo", &forge_dummy_mdo_payload()) {
        polymorph_os::serial_println!("[MFS] Notice: 'vault.mdo' skipped: {}", e);
    }

    // 2. Simulate a malicious agent trying to read it (Wrong Requestor Hash)
    let bad_context = polymorph_os::mdo_vm::MdoContext {
        requestor_id_hash: [0xBB; 32], // Fails the 0xAA check!
        action_hash: [0x00; 32], owner_id_hash: [0x00; 32], status: 0,
    };

    // This will hit the 0x42 Dissonance error!
    let _ = polymorph_os::mfs::MictFileSystem::read_file("vault.mdo", &bad_context);

    // 3. Simulate the TRUE OWNER reading it
    let good_context = polymorph_os::mdo_vm::MdoContext {
        requestor_id_hash: [0xAA; 32], // Matches the owner_hash in the header!
        action_hash: [0x00; 32], owner_id_hash: [0x00; 32], status: 0,
    };

    // This will succeed and print the TOP SECRET message!
    if let Ok(data) = polymorph_os::mfs::MictFileSystem::read_file("vault.mdo", &good_context) {
        polymorph_os::serial_println!("Data extracted: {}", core::str::from_utf8(&data).unwrap());
    }


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
    executor.spawn(Task::new(physics_tick_task()));
    
    serial_println!("[GENESIS OS] Entering MICT Event Loop. System Sovereign.");
    
    // [MICT: PRE-FLIGHT RENDER] 
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut gpu_lock = polymorph_os::gpu_driver::GPU_WRITER.lock();
        let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
        if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
            gpu.clear_screen(10, 15, 25);
            
            // [FIXED: THE GHOST WINDOW FIX FOR MULTI-WINDOW] 
            // 1. Temporarily extract the windows and launchers from the engine
            let mut temp_windows = alloc::vec::Vec::new();
            let mut temp_launchers = alloc::vec::Vec::new();
            core::mem::swap(&mut engine.windows, &mut temp_windows);
            core::mem::swap(&mut engine.launchers, &mut temp_launchers);
            
            // 2. Render ONLY the pure Nebula
            engine.render(gpu); 
            
            // 3. Save the pure Nebula to the cache
            gpu.save_to_nebula_cache(); 
            
            // 4. Put the windows and launchers back into the engine
            core::mem::swap(&mut engine.windows, &mut temp_windows);
            core::mem::swap(&mut engine.launchers, &mut temp_launchers);

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
        
        if char == '{' && !receiving_json {
            receiving_json = true;
            json_buffer.clear();
            json_buffer.push(char);
        } 
        else if receiving_json {
            json_buffer.push(char);
            
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
                            // [FIXED] Send commands to the top-most window!
                            if let Some(win) = engine_lock.as_mut().and_then(|e| e.windows.last_mut()) {
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
                            polymorph_os::serial_print!("{}", out); 
                        },
                        Err(e) => {
                            let out = alloc::format!("\n[KERNEL] MFS SAVE FAILED: {}\ngenesis> ", e);
                            polymorph_os::screen_println!("{}", out);
                            polymorph_os::serial_print!("{}", out); 
                        },
                    }
                }
                
                json_buffer.clear();
            }
        }
    }
}

async fn desktop_compositor_task() {
    use polymorph_os::task::mouse_stream::MouseStream;
    use futures_util::stream::StreamExt;
    use core::sync::atomic::Ordering;
    
    polymorph_os::serial_println!("[TASK] Desktop Compositor (Mouse/UI) Online.");
    let mut mouse_stream = MouseStream::new();
    
    let mut was_left_click = false;
    let mut packet = [0u8; 3];
    let mut byte_idx = 0;

    while let Some(byte) = mouse_stream.next().await {
        if byte_idx == 0 {
            if (byte & 0x08) != 0 { packet[0] = byte; byte_idx += 1; }
        } else if byte_idx == 1 {
            packet[1] = byte; byte_idx += 1;
        } else if byte_idx == 2 {
            packet[2] = byte; byte_idx = 0; 

            let flags = packet[0];
            let mut x_mov = packet[1] as i32;
            let mut y_mov = packet[2] as i32;
            let left_click = (flags & 0x01) != 0;

            if (flags & 0x10) != 0 { x_mov |= !0xFF; }
            if (flags & 0x20) != 0 { y_mov |= !0xFF; }

            if x_mov != 0 || y_mov != 0 || left_click != was_left_click {
                
                let mut cx = polymorph_os::splat::CURSOR_X.load(Ordering::SeqCst);
                let mut cy = polymorph_os::splat::CURSOR_Y.load(Ordering::SeqCst);
                
                let max_x = polymorph_os::splat::SCREEN_WIDTH.load(Ordering::SeqCst) - 6; 
                let max_y = polymorph_os::splat::SCREEN_HEIGHT.load(Ordering::SeqCst) - 6;

                cx = (cx + x_mov).clamp(0, max_x);
                cy = (cy - y_mov).clamp(0, max_y);

                polymorph_os::splat::CURSOR_X.store(cx, Ordering::SeqCst);
                polymorph_os::splat::CURSOR_Y.store(cy, Ordering::SeqCst);
                polymorph_os::splat::LEFT_CLICK.store(left_click, Ordering::SeqCst);

                if polymorph_os::task::mouse_stream::has_data() {
                    was_left_click = left_click;
                    continue; 
                }

                x86_64::instructions::interrupts::without_interrupts(|| {
                    let mut gpu_lock = polymorph_os::gpu_driver::GPU_WRITER.lock();
                    let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
                    
                    if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
                        polymorph_os::splat::render_desktop(gpu, engine);
                    }
                });
                was_left_click = left_click;
            }
        }
    }
}

// A test function to physically forge a compiled MDO file
pub fn forge_dummy_mdo_payload() -> alloc::vec::Vec<u8> {
    let mut payload = alloc::vec![0u8; 4096];
    
    // 0..4 Magic Bytes: MDO\x01
    payload[0] = 0x4D; payload[1] = 0x44; payload[2] = 0x4F; payload[3] = 0x01;
    
    // 4..36 Owner Hash (Let's make the "Owner" a hash of all 0xAA)
    for i in 4..36 { payload[i] = 0xAA; }
    
    // 76..79 Bytecode length (8 bytes for our script)
    payload[76] = 8;
    
    // 128..135 The Bytecode Script:
    // PUSH_VAR REQUESTOR (0x10, 0x01)
    // PUSH_VAR OWNER (0x10, 0x03)
    // EQ (0x20)
    // ASSERT_OR_DISSONANCE 0x42 (0x30, 0x42)
    // HALT_OK (0x3F)
    payload[128] = 0x10; payload[129] = 0x01;
    payload[130] = 0x10; payload[131] = 0x03;
    payload[132] = 0x20;
    payload[133] = 0x30; payload[134] = 0x42;
    payload[135] = 0x3F;
    
    // The actual text data payload!
    let msg = b"TOP SECRET: PolyMorphOS Sovereign Kernel Verified!";
    payload[136..136+msg.len()].copy_from_slice(msg);
    
    payload
}

// ==========================================================
// THE ASYNCHRONOUS PHYSICS ENGINE (Zero-CPU Idle)
// ==========================================================
use core::future::Future;
use core::task::{Context, Poll};

/// A helper future that lets a task pause for one executor tick, 
/// allowing the mouse/keyboard interrupts to be processed.
pub struct YieldNow { yielded: bool }
impl Future for YieldNow {
    type Output = ();
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.yielded { return Poll::Ready(()); }
        self.yielded = true;
        cx.waker().wake_by_ref(); // Put myself back in the queue
        Poll::Pending
    }
}
pub fn yield_now() -> YieldNow { YieldNow { yielded: false } }

async fn physics_tick_task() {
    polymorph_os::serial_println!("[TASK] Asynchronous Physics Engine Online.");
    loop {
        let mut physics_active = false;

        // 1. Check if we need to animate anything
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
            if let Some(engine) = engine_lock.as_mut() {
                for win in &engine.windows {
                    if win.base_z != win.target_z { physics_active = true; }
                    if let polymorph_os::splat::WindowState::Transitioning{..} = win.state { physics_active = true; }
                }
            }
        });

        if physics_active {
            // 2. Animate ONE frame safely
            x86_64::instructions::interrupts::without_interrupts(|| {
                let mut gpu_lock = polymorph_os::gpu_driver::GPU_WRITER.lock();
                let mut engine_lock = polymorph_os::splat::SPLAT_ENGINE.lock();
                if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
                    polymorph_os::splat::render_desktop(gpu, engine);
                }
            });

            // 3. Pace the animation safely outside the lock!
            //for _ in 0..50_000 { core::hint::spin_loop(); }
            
            // 4. Yield so the Mouse task can process its hardware packets!
            yield_now().await; 
        } else {
            // If nothing is moving, we can sleep a bit longer so we don't peg the CPU
            // (In the future, we will link this to an AtomicWaker triggered by clicks)
            yield_now().await;
        }
    }
}