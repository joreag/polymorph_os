use core::{pin::Pin, task::{Poll, Context}};
use futures_util::stream::Stream;
use futures_util::task::AtomicWaker;

const BUF_SIZE: usize = 65536;
static mut BUFFER:[u8; BUF_SIZE] = [0; BUF_SIZE];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;

static WAKER: AtomicWaker = AtomicWaker::new();

/// Called by hardware interrupt IRQ 12! Zero allocation.
pub(crate) fn add_byte(byte: u8) {
    unsafe {
        let next_head = (HEAD + 1) % BUF_SIZE;
        if next_head != TAIL {
            BUFFER[HEAD] = byte;
            HEAD = next_head;
            WAKER.wake(); // Wake the UI Compositor
        }
    }
}

// [MICT: THE RENDER THROTTLE PEEK]
pub fn has_data() -> bool {
    unsafe { HEAD != TAIL }
}

pub struct MouseStream;

impl MouseStream {
    pub fn new() -> Self {
        MouseStream
    }
}

impl Stream for MouseStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        unsafe {
            if HEAD != TAIL {
                let byte = BUFFER[TAIL];
                TAIL = (TAIL + 1) % BUF_SIZE;
                return Poll::Ready(Some(byte));
            }
        }

        WAKER.register(&cx.waker());
        
        unsafe {
            if HEAD != TAIL {
                WAKER.take();
                let byte = BUFFER[TAIL];
                TAIL = (TAIL + 1) % BUF_SIZE;
                Poll::Ready(Some(byte))
            } else {
                Poll::Pending
            }
        }
    }
}