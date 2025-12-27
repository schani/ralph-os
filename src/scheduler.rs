//! Cooperative multitasking scheduler
//!
//! Provides round-robin scheduling with sleep support.
//! Tasks yield voluntarily via yield_now() or sleep_ms().

use alloc::boxed::Box;
use alloc::vec::Vec;
use crate::task::{Task, TaskId, TaskState, Context};
use crate::context_switch::switch_context;
use crate::timer;

/// The global scheduler instance (boxed to avoid large stack/static moves)
static mut SCHEDULER: Option<Box<Scheduler>> = None;

/// The scheduler structure
pub struct Scheduler {
    /// All tasks in the system
    tasks: Vec<Task>,
    /// Index of currently running task
    current: usize,
    /// Next task ID to assign
    next_id: TaskId,
    /// Context for the boot/idle thread
    idle_context: Context,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        Scheduler {
            tasks: Vec::new(),
            current: 0,
            next_id: 0,
            idle_context: Context::default(),
        }
    }

    /// Spawn a new task
    pub fn spawn(&mut self, name: &'static str, entry: fn()) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;

        let task = Task::new(id, name, entry);
        self.tasks.push(task);

        id
    }

    /// Wake any sleeping tasks whose wake time has passed
    fn wake_sleeping_tasks(&mut self) {
        let now = timer::ticks();
        for task in &mut self.tasks {
            if task.state == TaskState::Sleeping && now >= task.wake_at {
                task.state = TaskState::Ready;
            }
        }
    }

    /// Find the next ready task (round-robin)
    /// Returns the index, or None if no tasks are ready
    fn find_next_ready(&self) -> Option<usize> {
        let len = self.tasks.len();
        if len == 0 {
            return None;
        }

        // Start searching from the task after current
        for i in 1..=len {
            let idx = (self.current + i) % len;
            if self.tasks[idx].state == TaskState::Ready {
                return Some(idx);
            }
        }

        None
    }

    /// Check if there are any sleeping tasks
    fn has_sleeping_tasks(&self) -> bool {
        self.tasks.iter().any(|t| t.state == TaskState::Sleeping)
    }

    /// Check if there are any living tasks (not Finished)
    fn has_living_tasks(&self) -> bool {
        self.tasks.iter().any(|t| t.state != TaskState::Finished)
    }

    /// Schedule and switch to the next task
    fn schedule(&mut self) {
        // Poll timer
        timer::poll();

        // Wake sleeping tasks
        self.wake_sleeping_tasks();

        // Find next ready task
        loop {
            if let Some(next_idx) = self.find_next_ready() {
                // Found a ready task - switch to it
                let current_idx = self.current;
                self.current = next_idx;
                self.tasks[next_idx].state = TaskState::Running;

                // Get pointers to contexts
                let current_ctx = &mut self.tasks[current_idx].context as *mut Context;
                let next_ctx = &self.tasks[next_idx].context as *const Context;

                // Context switch
                unsafe {
                    switch_context(current_ctx, next_ctx);
                }

                // We return here when we're switched back to
                return;
            }

            // No ready tasks
            if self.has_sleeping_tasks() {
                // Busy-wait until a sleeping task wakes
                timer::poll();
                self.wake_sleeping_tasks();
                core::hint::spin_loop();
            } else if !self.has_living_tasks() {
                // All tasks finished - nothing to do
                return;
            } else {
                // This shouldn't happen in normal operation
                core::hint::spin_loop();
            }
        }
    }

    /// Main scheduler loop - never returns
    pub fn run(&mut self) -> ! {
        if self.tasks.is_empty() {
            crate::println!("No tasks to run!");
            loop {
                unsafe { core::arch::asm!("hlt"); }
            }
        }

        // Start the first task
        self.current = 0;
        self.tasks[0].state = TaskState::Running;

        // Get pointer to first task's context
        let first_ctx = &self.tasks[0].context as *const Context;

        // Switch from idle context to first task
        unsafe {
            switch_context(&mut self.idle_context, first_ctx);
        }

        // Should never reach here, but if we do, halt
        loop {
            unsafe { core::arch::asm!("hlt"); }
        }
    }
}

// ============================================================================
// Public API functions
// ============================================================================

/// Initialize the scheduler
pub fn init() {
    unsafe {
        SCHEDULER = Some(Box::new(Scheduler::new()));
    }
}

/// Spawn a new task
pub fn spawn(name: &'static str, entry: fn()) -> TaskId {
    unsafe {
        SCHEDULER
            .as_mut()
            .expect("Scheduler not initialized")
            .spawn(name, entry)
    }
}

/// Run the scheduler (never returns)
pub fn run() -> ! {
    unsafe {
        SCHEDULER
            .as_mut()
            .expect("Scheduler not initialized")
            .run()
    }
}

/// Yield the current task to let other tasks run
pub fn yield_now() {
    unsafe {
        if let Some(ref mut sched) = SCHEDULER {
            // Mark current task as Ready (not Running)
            if sched.current < sched.tasks.len() {
                sched.tasks[sched.current].state = TaskState::Ready;
            }
            sched.schedule();
        }
    }
}

/// Sleep for the specified number of ticks
pub fn sleep_ticks(ticks: u64) {
    unsafe {
        if let Some(ref mut sched) = SCHEDULER {
            let wake_at = timer::ticks() + ticks;
            if sched.current < sched.tasks.len() {
                sched.tasks[sched.current].wake_at = wake_at;
                sched.tasks[sched.current].state = TaskState::Sleeping;
            }
            sched.schedule();
        }
    }
}

/// Sleep for the specified number of milliseconds
pub fn sleep_ms(ms: u64) {
    sleep_ticks(timer::ms_to_ticks(ms));
}

/// Exit the current task
pub fn exit_task() {
    unsafe {
        if let Some(ref mut sched) = SCHEDULER {
            if sched.current < sched.tasks.len() {
                let name = sched.tasks[sched.current].name;
                crate::println!("[{}] Task finished", name);
                sched.tasks[sched.current].state = TaskState::Finished;
            }
            sched.schedule();
        }
    }

    // Should never reach here
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

/// Get information about running tasks (for debugging)
pub fn task_count() -> usize {
    unsafe {
        SCHEDULER
            .as_ref()
            .map(|s| s.tasks.len())
            .unwrap_or(0)
    }
}
