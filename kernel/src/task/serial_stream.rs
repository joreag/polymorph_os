use core::{pin::Pin, task::{Poll, Context}};
use futures_util::{stream::Stream, task::AtomicWaker};

const BUF_SIZE: usize = 65536;
static mut BUFFER: [u8; BUF_SIZE] = [0; BUF_SIZE];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;

static WAKER: AtomicWaker = AtomicWaker::new();

/// Called by the hardware interrupt! Ultra-fast, zero allocation.
pub(crate) fn add_byte(byte: u8) {
    unsafe {
        let next_head = (HEAD + 1) % BUF_SIZE;
        if next_head != TAIL {
            BUFFER[HEAD] = byte;
            HEAD = next_head;
            WAKER.wake(); // Wake the Executor!
        }
    }
}

pub struct SerialStream;

impl SerialStream {
    pub fn new() -> Self {
        SerialStream
    }
}

impl Stream for SerialStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        // [MICT: CHECK] - Is there data in the ring buffer?
        unsafe {
            if HEAD != TAIL {
                let byte = BUFFER[TAIL];
                TAIL = (TAIL + 1) % BUF_SIZE;
                return Poll::Ready(Some(byte));
            }
        }

        // [MICT: TRANSFORM] - No data. Sleep and wait for hardware wake.
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