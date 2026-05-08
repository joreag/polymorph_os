use super::{Task, TaskId};
use alloc::{collections::BTreeMap, sync::Arc, task::Wake};
use core::task::{Context, Poll, Waker};
use crossbeam_queue::ArrayQueue;

/// The GenesisOS MICT Task Scheduler (Physical Substrate)
pub struct MictExecutor {
    process_registry: BTreeMap<TaskId, Task>,
    ready_queue: Arc<ArrayQueue<TaskId>>, // Aligns with Scheduler.mdo `ready_queue`
    waker_cache: BTreeMap<TaskId, Waker>,
    pub current_tick: u64, // The OS Heartbeat
}

impl MictExecutor {
    pub fn new() -> Self {
        MictExecutor {
            process_registry: BTreeMap::new(),
            // Capacity of 100 concurrent async tasks in the ready queue
            ready_queue: Arc::new(ArrayQueue::new(100)),
            waker_cache: BTreeMap::new(),
            current_tick: 0,
        }
    }

    pub fn spawn(&mut self, task: Task) {
        let task_id = task.id;
        if self.process_registry.insert(task.id, task).is_some() {
            panic!("[DISSONANCE] Process with same ID already exists in registry");
        }
        self.ready_queue.push(task_id).expect("[DISSONANCE] Scheduler Ready Queue full");
    }

    pub fn run(&mut self) -> ! {
        loop {
            self.current_tick += 1;
            self.run_ready_tasks();
            self.sleep_if_idle();
        }
    }

    fn run_ready_tasks(&mut self) {
        // Destructure `self` to satisfy the borrow checker's strict aliasing rules
        let Self {
            process_registry,
            ready_queue,
            waker_cache,
            current_tick: _,
        } = self;

        // [MICT: MAP] - Continuously map the ready_queue for awaiting processes
        while let Some(task_id) = ready_queue.pop() {
            
            //[MICT: ITERATE] - Fetch the process and its hardware Waker context
            let task = match process_registry.get_mut(&task_id) {
                Some(task) => task,
                None => continue, // Task was terminated
            };
            
            let waker = waker_cache
                .entry(task_id)
                .or_insert_with(|| TaskWaker::new(task_id, ready_queue.clone()));
            let mut context = Context::from_waker(waker);
            
            // [MICT: CHECK] - Poll the task. Has the hardware data arrived?
            match task.poll(&mut context) {
                Poll::Ready(()) => {
                    // [MICT: TRANSFORM] - Process completed. Reclaim resources.
                    process_registry.remove(&task_id);
                    waker_cache.remove(&task_id);
                }
                Poll::Pending => {
                    // [MICT: TRANSFORM] - Process still waiting (Blocked). 
                    // Do nothing. The hardware Waker will push it back to the ready_queue later.
                }
            }
        }
    }

    /// [SCHEDULER GOVERNANCE] - Thermal & Load Management
    fn sleep_if_idle(&self) {
        use x86_64::instructions::interrupts::{self, enable_and_hlt};

        // Disable interrupts briefly so a hardware interrupt doesn't fire 
        // *between* checking the queue and going to sleep.
        interrupts::disable();
        
        if self.ready_queue.is_empty() {
            // [TRANSFORM] - Put the silicon to sleep until the next interrupt (e.g. keystroke)
            // This is the physical implementation of Load Throttling.
            enable_and_hlt();
        } else {
            // Tasks were added right as we checked, re-enable and keep looping.
            interrupts::enable();
        }
    }
}

// =========================================================================
// THE HARDWARE INTERRUPT HOOK (Waker)
// =========================================================================

struct TaskWaker {
    task_id: TaskId,
    ready_queue: Arc<ArrayQueue<TaskId>>,
}

impl TaskWaker {
    fn new(task_id: TaskId, ready_queue: Arc<ArrayQueue<TaskId>>) -> Waker {
        Waker::from(Arc::new(TaskWaker {
            task_id,
            ready_queue,
        }))
    }

    fn wake_task(&self) {
        // A hardware interrupt called this! Push the task back onto the Ready Queue.
        // Because ArrayQueue is lock-free, this is perfectly safe inside an interrupt.
        self.ready_queue.push(self.task_id).expect("ready_queue full");
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_task();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wake_task();
    }
}