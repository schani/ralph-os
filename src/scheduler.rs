//! Cooperative multitasking scheduler
//!
//! Provides round-robin scheduling with sleep support.
//! Tasks yield voluntarily via yield_now() or sleep_ms().

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::task::{Task, TaskId, TaskState, Context};
use crate::context_switch::switch_context;
use crate::timer;

/// Single-threaded scheduler cell with initialization guard.
///
/// This provides safe access to the global scheduler by:
/// - Enforcing single initialization (panics if init called twice)
/// - Enforcing initialization before use (panics if used before init)
/// - Centralizing the unsafe into a single, documented location
struct SchedulerCell {
    inner: UnsafeCell<Option<Scheduler>>,
    initialized: AtomicBool,
}

// Safety: Ralph OS is single-threaded with cooperative scheduling.
// Only one task runs at a time, and tasks cannot be preempted.
// The SchedulerCell enforces initialization ordering.
unsafe impl Sync for SchedulerCell {}

impl SchedulerCell {
    const fn new() -> Self {
        SchedulerCell {
            inner: UnsafeCell::new(None),
            initialized: AtomicBool::new(false),
        }
    }

    /// Initialize the scheduler. Panics if called more than once.
    fn init(&self) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            panic!("Scheduler already initialized");
        }
        // Safety: We just set initialized to true, and this is the only
        // place that writes to inner. Single-threaded access guaranteed.
        unsafe {
            *self.inner.get() = Some(Scheduler::new());
        }
    }

    /// Access the scheduler mutably via closure. Panics if not initialized.
    fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Scheduler) -> R,
    {
        assert!(
            self.initialized.load(Ordering::SeqCst),
            "Scheduler not initialized"
        );
        // Safety: Single-threaded cooperative scheduling means only one
        // task executes at a time. The closure-based API prevents holding
        // references across yield points.
        unsafe {
            let sched = (*self.inner.get()).as_mut().unwrap();
            f(sched)
        }
    }

    /// Access for the run() function which needs special handling.
    /// Returns a raw pointer - caller must ensure safe usage.
    unsafe fn get_mut(&self) -> &mut Scheduler {
        assert!(
            self.initialized.load(Ordering::SeqCst),
            "Scheduler not initialized"
        );
        (*self.inner.get()).as_mut().unwrap()
    }
}

static SCHEDULER: SchedulerCell = SchedulerCell::new();

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

    /// Remove finished tasks from the task list to free memory.
    /// Adjusts the current index to maintain correct task tracking.
    fn reap_finished_tasks(&mut self) {
        // Count finished tasks before current for index adjustment
        let finished_before_current = self.tasks[..self.current]
            .iter()
            .filter(|t| t.state == TaskState::Finished)
            .count();

        // Remove all finished tasks
        self.tasks.retain(|t| t.state != TaskState::Finished);

        // Adjust current index to account for removed tasks
        if self.current >= finished_before_current {
            self.current -= finished_before_current;
        }

        // Ensure current index is valid
        if self.current >= self.tasks.len() && !self.tasks.is_empty() {
            self.current = 0;
        }
    }

    /// Schedule and switch to the next task
    fn schedule(&mut self) {
        // Poll timer
        timer::poll();

        // Wake sleeping tasks
        self.wake_sleeping_tasks();

        // Periodically reap finished tasks to free memory.
        // Only reap if there are finished tasks to avoid the overhead.
        if self.tasks.iter().any(|t| t.state == TaskState::Finished) {
            self.reap_finished_tasks();
        }

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
                // Busy-wait until a sleeping task wakes.
                //
                // NOTE: This burns CPU at 100%. We cannot use HLT here because:
                // 1. No interrupt handlers are installed (no IDT)
                // 2. HLT waits for interrupts, but PIT interrupts would triple-fault
                // 3. The only fix is implementing proper interrupt handling
                //
                // For a cooperative OS without interrupts, this is unavoidable.
                // Poll the timer to track time and check for wake conditions.
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
    SCHEDULER.init();
}

/// Spawn a new task
pub fn spawn(name: &'static str, entry: fn()) -> TaskId {
    SCHEDULER.with(|sched| sched.spawn(name, entry))
}

/// Run the scheduler (never returns)
pub fn run() -> ! {
    // run() is special - it never returns and needs direct access
    // Safety: Single-threaded, and run() takes control of execution
    unsafe { SCHEDULER.get_mut().run() }
}

/// Yield the current task to let other tasks run
pub fn yield_now() {
    SCHEDULER.with(|sched| {
        if sched.current < sched.tasks.len() {
            sched.tasks[sched.current].state = TaskState::Ready;
        }
        sched.schedule();
    });
}

/// Sleep for the specified number of ticks
pub fn sleep_ticks(ticks: u64) {
    SCHEDULER.with(|sched| {
        let wake_at = timer::ticks() + ticks;
        if sched.current < sched.tasks.len() {
            sched.tasks[sched.current].wake_at = wake_at;
            sched.tasks[sched.current].state = TaskState::Sleeping;
        }
        sched.schedule();
    });
}

/// Sleep for the specified number of milliseconds
pub fn sleep_ms(ms: u64) {
    sleep_ticks(timer::ms_to_ticks(ms));
}

/// Exit the current task
pub fn exit_task() {
    SCHEDULER.with(|sched| {
        if sched.current < sched.tasks.len() {
            let name = sched.tasks[sched.current].name;
            crate::println!("[{}] Task finished", name);
            sched.tasks[sched.current].state = TaskState::Finished;
        }
        sched.schedule();
    });

    // Should never reach here, but halt if we do
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

/// Get information about running tasks (for debugging)
pub fn task_count() -> usize {
    SCHEDULER.with(|sched| sched.tasks.len())
}
