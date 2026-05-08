use core::{pin::Pin, task::{Poll, Context}};
use futures_util::stream::{Stream, StreamExt};
use futures_util::task::AtomicWaker;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

// Use our custom MICT output macros
//use crate::{screen_print, serial_print};

//[MICT: ZERO-ALLOCATION RING BUFFER]
const BUF_SIZE: usize = 256;
static mut BUFFER:[u8; BUF_SIZE] = [0; BUF_SIZE];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;

static WAKER: AtomicWaker = AtomicWaker::new();

/// Called by the hardware interrupt (IRQ 1)! Ultra-fast, zero allocation.
pub(crate) fn add_scancode(scancode: u8) {
    unsafe {
        let next_head = (HEAD + 1) % BUF_SIZE;
        if next_head != TAIL {
            BUFFER[HEAD] = scancode;
            HEAD = next_head;
            WAKER.wake(); // Wake the MictExecutor!
        } else {
            crate::serial_println!("WARNING: Keyboard buffer full; dropping scancode");
        }
    }
}

pub struct ScancodeStream;

impl ScancodeStream {
    pub fn new() -> Self {
        ScancodeStream
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        //[MICT: CHECK] - Is there data in the ring buffer?
        unsafe {
            if HEAD != TAIL {
                let scancode = BUFFER[TAIL];
                TAIL = (TAIL + 1) % BUF_SIZE;
                return Poll::Ready(Some(scancode));
            }
        }

        // [MICT: TRANSFORM] - No data. Sleep and wait for hardware wake.
        WAKER.register(&cx.waker());
        
        unsafe {
            if HEAD != TAIL {
                WAKER.take();
                let scancode = BUFFER[TAIL];
                TAIL = (TAIL + 1) % BUF_SIZE;
                Poll::Ready(Some(scancode))
            } else {
                Poll::Pending
            }
        }
    }
}

// =====================================================================
//[MICT: TRANSFORM] - The Keyboard Async Task
// =====================================================================
pub async fn print_keypresses() {
    let mut scancodes = ScancodeStream::new();
    
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => {
                        //[MICT: DIRECT UI INTERACTION]
                        x86_64::instructions::interrupts::without_interrupts(|| {
                            let mut gpu_lock = crate::gpu_driver::GPU_WRITER.lock();
                            let mut engine_lock = crate::splat::SPLAT_ENGINE.lock();
                            
                            if let (Some(gpu), Some(engine)) = (gpu_lock.as_mut(), engine_lock.as_mut()) {
                                // 1. Send the keystroke to the active window
                                if let Some(win) = &mut engine.active_window {
                                    win.process_keystroke(character);
                                }
                                // 2. Instantly flip the 4MB screen buffer!
                                crate::splat::render_desktop(gpu, engine);
                            }
                        });
                        
                        // Echo to the serial port for debug logs
                        crate::serial_print!("{}", character);
                    }
                    DecodedKey::RawKey(_key) => {} // Do nothing for Shift/Ctrl
                }
            }
        }
    }
}