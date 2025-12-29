//! Linked List Heap Allocator
//!
//! A simple first-fit linked list allocator implemented from scratch.
//! Supports allocation and deallocation with proper alignment handling.
//! Each allocation includes a header with task ID for memory attribution.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::task::TaskId;

/// Flag to track whether scheduler is initialized
/// Used to safely get current task ID (before scheduler exists, all allocs are "kernel")
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

/// Mark the scheduler as ready (called after scheduler::init)
pub fn mark_scheduler_ready() {
    SCHEDULER_READY.store(true, Ordering::Release);
}

/// Get the current task ID if scheduler is running
fn get_current_task_id() -> Option<TaskId> {
    if SCHEDULER_READY.load(Ordering::Acquire) {
        crate::scheduler::current_task_id()
    } else {
        None // Kernel/boot allocation
    }
}

/// Header placed immediately before user data in each allocation
/// Size: 32 bytes (padded to 16-byte multiple so header is always at block_start
/// for typical 8/16-byte alignments)
#[repr(C)]
struct AllocationHeader {
    /// Start of the memory block (for returning to free list on dealloc)
    block_start: usize,
    /// Size of the user data (not including header or padding)
    size: usize,
    /// Task that owns this allocation (None = kernel/boot)
    task_id: Option<TaskId>,
    /// Padding to make header 32 bytes (16-byte aligned)
    _padding: usize,
}

const HEADER_SIZE: usize = core::mem::size_of::<AllocationHeader>();

/// A free memory block in the linked list
#[repr(C)]
struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

/// Minimum block size (must fit a FreeBlock header)
const MIN_BLOCK_SIZE: usize = core::mem::size_of::<FreeBlock>();

impl FreeBlock {
    /// Create a new free block at the given address
    ///
    /// # Safety
    /// The address must be valid and properly aligned for FreeBlock
    unsafe fn new(addr: usize, size: usize) -> NonNull<FreeBlock> {
        let block = addr as *mut FreeBlock;
        (*block).size = size;
        (*block).next = None;
        NonNull::new_unchecked(block)
    }
}

/// Linked list allocator
pub struct LinkedListAllocator {
    head: Option<NonNull<FreeBlock>>,
    heap_start: usize,
    heap_end: usize,
}

// Safety: We use spinlocks to protect access in the global allocator wrapper
unsafe impl Send for LinkedListAllocator {}

impl LinkedListAllocator {
    /// Create a new empty allocator
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: None,
            heap_start: 0,
            heap_end: 0,
        }
    }

    /// Initialize the allocator with a memory region
    ///
    /// # Safety
    /// - The memory region must be valid and not used by anything else
    /// - This must only be called once
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;

        // Create initial free block spanning entire heap
        let block = FreeBlock::new(heap_start, heap_size);
        self.head = Some(block);
    }

    /// Align the given address upward to the given alignment
    fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }

    /// Allocate memory with the given layout
    pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
        // We need space for header + user data
        let user_size = layout.size().max(1);
        let user_align = layout.align().max(core::mem::align_of::<usize>());

        // First-fit search
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(block_ptr) = current {
            let block = unsafe { block_ptr.as_ref() };
            let block_start = block_ptr.as_ptr() as usize;
            let block_size = block.size;

            // Calculate where user data would go (aligned)
            let user_addr = Self::align_up(block_start + HEADER_SIZE, user_align);
            // Header goes immediately before user data
            let header_addr = user_addr - HEADER_SIZE;
            // Total space needed from block start to end of user data
            let total_size = (user_addr - block_start) + user_size;

            // Check if block is large enough
            if block_size >= total_size.max(MIN_BLOCK_SIZE) {
                // This block works! Remove it from the free list
                let next = block.next;

                // Update previous block's next pointer (or head)
                match prev {
                    Some(mut prev_ptr) => unsafe {
                        prev_ptr.as_mut().next = next;
                    },
                    None => {
                        self.head = next;
                    }
                }

                // Handle leftover space at the end
                let used_end = user_addr + user_size;
                let remaining = block_start + block_size - used_end;
                if remaining >= MIN_BLOCK_SIZE {
                    // Create a new free block for remaining space
                    let new_block = unsafe { FreeBlock::new(used_end, remaining) };
                    self.add_free_block(new_block);
                }

                // Write the allocation header (immediately before user data)
                let header = header_addr as *mut AllocationHeader;
                unsafe {
                    (*header).block_start = block_start;
                    (*header).size = user_size;
                    (*header).task_id = get_current_task_id();
                    (*header)._padding = 0;
                }

                // Notify memory visualizer of allocation (from block_start)
                let alloc_size = used_end - block_start;
                crate::memvis::on_alloc(block_start, alloc_size);

                return user_addr as *mut u8;
            }

            prev = current;
            current = block.next;
        }

        // No suitable block found
        ptr::null_mut()
    }

    /// Deallocate memory
    ///
    /// # Safety
    /// - ptr must have been allocated by this allocator
    /// - layout must match the original allocation
    pub unsafe fn deallocate(&mut self, ptr: *mut u8, _layout: Layout) {
        let user_addr = ptr as usize;

        // Header is immediately before user data
        let header = (user_addr - HEADER_SIZE) as *mut AllocationHeader;
        let block_start = (*header).block_start;
        let user_size = (*header).size;

        // Calculate total block size (from block_start to end of user data)
        let block_size = (user_addr + user_size) - block_start;

        // Notify memory visualizer of deallocation
        crate::memvis::on_dealloc(block_start, block_size);

        // Create a new free block
        let block = FreeBlock::new(block_start, block_size.max(MIN_BLOCK_SIZE));
        self.add_free_block(block);

        // Try to merge adjacent blocks
        self.merge_free_blocks();
    }

    /// Add a free block to the list (sorted by address for merging)
    fn add_free_block(&mut self, new_block: NonNull<FreeBlock>) {
        let new_addr = new_block.as_ptr() as usize;

        // Find insertion point (keep list sorted by address)
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(block_ptr) = current {
            let block_addr = block_ptr.as_ptr() as usize;
            if block_addr > new_addr {
                break;
            }
            prev = current;
            current = unsafe { block_ptr.as_ref().next };
        }

        // Insert the new block
        unsafe {
            (*new_block.as_ptr()).next = current;
        }

        match prev {
            Some(mut prev_ptr) => unsafe {
                prev_ptr.as_mut().next = Some(new_block);
            },
            None => {
                self.head = Some(new_block);
            }
        }
    }

    /// Merge adjacent free blocks
    fn merge_free_blocks(&mut self) {
        let mut current = self.head;

        while let Some(mut block_ptr) = current {
            let block = unsafe { block_ptr.as_mut() };
            let block_end = block_ptr.as_ptr() as usize + block.size;

            if let Some(next_ptr) = block.next {
                let next_addr = next_ptr.as_ptr() as usize;

                // Check if blocks are adjacent
                if block_end == next_addr {
                    // Merge: extend current block and skip next
                    let next = unsafe { next_ptr.as_ref() };
                    block.size += next.size;
                    block.next = next.next;
                    // Don't advance - check if we can merge more
                    continue;
                }
            }

            current = block.next;
        }
    }

    /// Get the header for an allocation at the given user address
    fn get_header(user_addr: usize) -> &'static AllocationHeader {
        unsafe { &*((user_addr - HEADER_SIZE) as *const AllocationHeader) }
    }
}

/// Global allocator wrapper with spinlock protection
pub struct LockedAllocator {
    inner: Spinlock<LinkedListAllocator>,
}

impl LockedAllocator {
    pub const fn new() -> Self {
        LockedAllocator {
            inner: Spinlock::new(LinkedListAllocator::new()),
        }
    }

    /// Initialize the allocator
    ///
    /// # Safety
    /// Must only be called once with valid memory region
    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        self.inner.lock().init(heap_start, heap_size);
    }
}

unsafe impl GlobalAlloc for LockedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner.lock().allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.lock().deallocate(ptr, layout);
    }
}

/// Simple spinlock implementation
pub struct Spinlock<T> {
    locked: core::sync::atomic::AtomicBool,
    data: core::cell::UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}
unsafe impl<T: Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Spinlock {
            locked: core::sync::atomic::AtomicBool::new(false),
            data: core::cell::UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        // In single-threaded cooperative scheduling, contention should never occur.
        // If it does, our invariants are violated - panic immediately rather than deadlock.
        if self
            .locked
            .compare_exchange(
                false,
                true,
                core::sync::atomic::Ordering::Acquire,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            panic!("Allocator lock contention - cooperative scheduling invariant violated!");
        }
        SpinlockGuard { lock: self }
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
}

impl<T> core::ops::Deref for SpinlockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> core::ops::DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock
            .locked
            .store(false, core::sync::atomic::Ordering::Release);
    }
}

// Global allocator instance
#[global_allocator]
static ALLOCATOR: LockedAllocator = LockedAllocator::new();

/// Initialize the heap allocator
///
/// # Safety
/// Must be called exactly once during kernel initialization
pub unsafe fn init_heap(heap_start: usize, heap_size: usize) {
    ALLOCATOR.init(heap_start, heap_size);
}

/// Get current heap usage statistics
///
/// Returns (used_bytes, free_bytes)
pub fn get_heap_stats() -> (usize, usize) {
    let allocator = ALLOCATOR.inner.lock();

    // Walk the free list to count free bytes
    let mut free = 0;
    let mut current = allocator.head;

    while let Some(block_ptr) = current {
        let block = unsafe { block_ptr.as_ref() };
        free += block.size;
        current = block.next;
    }

    let total = allocator.heap_end - allocator.heap_start;
    let used = total - free;

    (used, free)
}

/// Find the allocation that contains the given address
///
/// Returns Some((start, end, task_id)) if the address is in an allocated region,
/// or None if the address is in a free region or outside the heap.
pub fn find_allocation(addr: usize) -> Option<(usize, usize)> {
    let allocator = ALLOCATOR.inner.lock();

    // Check if address is even in the heap
    if addr < allocator.heap_start || addr >= allocator.heap_end {
        return None;
    }

    // Walk the free list to find allocated gaps
    // Allocations are the spaces BETWEEN free blocks
    let mut prev_end = allocator.heap_start;
    let mut current = allocator.head;

    while let Some(block_ptr) = current {
        let block = unsafe { block_ptr.as_ref() };
        let block_start = block_ptr.as_ptr() as usize;
        let block_end = block_start + block.size;

        // There's an allocation from prev_end to block_start
        if prev_end < block_start {
            if addr >= prev_end && addr < block_start {
                // Found it - address is in this allocated region
                return Some((prev_end, block_start));
            }
        }

        // Check if address is in this free block
        if addr >= block_start && addr < block_end {
            // Address is in a free region
            return None;
        }

        prev_end = block_end;
        current = block.next;
    }

    // Check for allocation after the last free block
    if prev_end < allocator.heap_end && addr >= prev_end && addr < allocator.heap_end {
        return Some((prev_end, allocator.heap_end));
    }

    None
}

/// Find the free region that contains the given address
///
/// Returns Some((start, end)) if the address is in a free region,
/// or None if the address is allocated or outside the heap.
pub fn find_free_region(addr: usize) -> Option<(usize, usize)> {
    let allocator = ALLOCATOR.inner.lock();

    // Check if address is even in the heap
    if addr < allocator.heap_start || addr >= allocator.heap_end {
        return None;
    }

    let mut current = allocator.head;

    while let Some(block_ptr) = current {
        let block = unsafe { block_ptr.as_ref() };
        let block_start = block_ptr.as_ptr() as usize;
        let block_end = block_start + block.size;

        if addr >= block_start && addr < block_end {
            return Some((block_start, block_end));
        }

        current = block.next;
    }

    None
}

/// Find which task owns the allocation at the given address
///
/// Returns Some(task_id) if the address is in an allocation,
/// where task_id is None for kernel/boot allocations.
/// Returns None if the address is not in an allocation.
pub fn find_allocation_owner(addr: usize) -> Option<Option<TaskId>> {
    // First check if this is in an allocation
    if let Some((alloc_start, _alloc_end)) = find_allocation(addr) {
        // Read the header to get task ID
        // Header is at the start of the allocation
        let header = unsafe { &*(alloc_start as *const AllocationHeader) };
        return Some(header.task_id);
    }
    None
}

/// Find which task owns the majority of a memory range
///
/// Returns Some((task_id, bytes)) where task_id is the task that owns
/// the most bytes in the given range (None = kernel/boot allocations).
/// Returns None if no allocations overlap the range.
pub fn find_majority_owner(range_start: usize, range_end: usize) -> Option<(Option<TaskId>, usize)> {
    let allocator = ALLOCATOR.inner.lock();

    // Check bounds
    if range_start >= allocator.heap_end || range_end <= allocator.heap_start {
        return None;
    }

    // Walk through allocated regions (gaps between free blocks)
    let mut prev_end = allocator.heap_start;
    let mut current = allocator.head;

    // Track bytes per task (up to 8 tasks)
    const MAX_TASKS: usize = 8;
    let mut task_bytes: [(Option<TaskId>, usize); MAX_TASKS] = [(None, 0); MAX_TASKS];
    let mut num_tasks = 0;

    loop {
        let (alloc_start, alloc_end) = if let Some(block_ptr) = current {
            let block = unsafe { block_ptr.as_ref() };
            let block_start = block_ptr.as_ptr() as usize;

            // Allocation is from prev_end to block_start
            let alloc = (prev_end, block_start);
            prev_end = block_start + block.size;
            current = block.next;
            alloc
        } else {
            // Last allocation: from prev_end to heap_end
            if prev_end < allocator.heap_end {
                let alloc = (prev_end, allocator.heap_end);
                prev_end = allocator.heap_end; // Mark as done
                alloc
            } else {
                break;
            }
        };

        // Skip if no actual allocation
        if alloc_start >= alloc_end {
            continue;
        }

        // Calculate overlap with range
        if alloc_end <= range_start || alloc_start >= range_end {
            continue; // No overlap
        }

        let overlap_start = alloc_start.max(range_start);
        let overlap_end = alloc_end.min(range_end);
        let overlap_bytes = overlap_end - overlap_start;

        if overlap_bytes == 0 {
            continue;
        }

        // Get task ID from allocation header
        let header = unsafe { &*(alloc_start as *const AllocationHeader) };
        let task_id = header.task_id;

        // Add to task_bytes
        let mut found = false;
        for (tid, bytes) in task_bytes.iter_mut().take(num_tasks) {
            if *tid == task_id {
                *bytes += overlap_bytes;
                found = true;
                break;
            }
        }
        if !found && num_tasks < MAX_TASKS {
            task_bytes[num_tasks] = (task_id, overlap_bytes);
            num_tasks += 1;
        }
    }

    // Find task with most bytes
    let mut best: Option<(Option<TaskId>, usize)> = None;
    for (tid, bytes) in task_bytes.iter().take(num_tasks) {
        if *bytes > 0 {
            match best {
                None => best = Some((*tid, *bytes)),
                Some((_, best_bytes)) if *bytes > best_bytes => {
                    best = Some((*tid, *bytes));
                }
                _ => {}
            }
        }
    }

    best
}

/// Get all heap allocations for a specific task
///
/// Returns a list of (start_addr, size) for all heap allocations made by the task.
/// Use task_id = None to get kernel/boot allocations.
pub fn get_task_heap_allocations(task_id: Option<TaskId>) -> alloc::vec::Vec<(usize, usize)> {
    let allocator = ALLOCATOR.inner.lock();
    let mut result = alloc::vec::Vec::new();

    // Walk through allocated regions (gaps between free blocks)
    let mut prev_end = allocator.heap_start;
    let mut current = allocator.head;

    loop {
        let (alloc_start, alloc_end) = if let Some(block_ptr) = current {
            let block = unsafe { block_ptr.as_ref() };
            let block_start = block_ptr.as_ptr() as usize;

            let alloc = (prev_end, block_start);
            prev_end = block_start + block.size;
            current = block.next;
            alloc
        } else {
            if prev_end < allocator.heap_end {
                let alloc = (prev_end, allocator.heap_end);
                prev_end = allocator.heap_end;
                alloc
            } else {
                break;
            }
        };

        if alloc_start >= alloc_end {
            continue;
        }

        // Read header to check task ID
        let header = unsafe { &*(alloc_start as *const AllocationHeader) };
        if header.task_id == task_id {
            // Return user-visible size, not including header
            result.push((alloc_start + HEADER_SIZE, header.size));
        }
    }

    result
}
