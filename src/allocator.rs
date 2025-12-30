//! Linked List Heap Allocator
//!
//! A simple first-fit linked list allocator implemented from scratch.
//! Supports allocation and deallocation with proper alignment handling.
//! Each allocation includes a header with task ID for memory attribution.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::task::TaskId;

const ALIGNMENT: usize = 8;

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

const HEADER_MAGIC: u32 = u32::from_le_bytes(*b"RLPH");
const KERNEL_TASK_ID_SENTINEL: u32 = u32::MAX;

#[inline]
fn encode_task_id(task_id: Option<TaskId>) -> u32 {
    match task_id {
        None => KERNEL_TASK_ID_SENTINEL,
        Some(id) => id,
    }
}

#[inline]
fn decode_task_id(raw: u32) -> Option<TaskId> {
    if raw == KERNEL_TASK_ID_SENTINEL {
        None
    } else {
        Some(raw)
    }
}

/// Header placed at the start of each allocated block.
#[repr(C)]
struct AllocationHeader {
    magic: u32,
    task_id: u32,       // TaskId or sentinel
    block_size: usize,  // Total bytes consumed, 8-byte aligned
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
        assert!(heap_start % ALIGNMENT == 0);
        assert!(heap_size % ALIGNMENT == 0);
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
        assert!(layout.align() <= ALIGNMENT);

        // We need space for header + user data, rounded so blocks always remain 8-byte aligned.
        let user_size = layout.size().max(1);
        let total_size = Self::align_up(HEADER_SIZE + user_size, ALIGNMENT).max(MIN_BLOCK_SIZE);

        // First-fit search
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(block_ptr) = current {
            let block = unsafe { block_ptr.as_ref() };
            let block_start = block_ptr.as_ptr() as usize;
            let block_size = block.size;
            debug_assert!(block_start % ALIGNMENT == 0);
            debug_assert!(block_size % ALIGNMENT == 0);

            // Check if block is large enough
            if block_size >= total_size {
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

                // Handle leftover space at the end.
                //
                // If the tail is too small to hold a FreeBlock header, we "eat" it
                // as part of this allocation so the heap still partitions cleanly.
                let (alloc_block_size, remaining) = if block_size - total_size >= MIN_BLOCK_SIZE {
                    (total_size, block_size - total_size)
                } else {
                    (block_size, 0)
                };
                let used_end = block_start + alloc_block_size;
                if remaining >= MIN_BLOCK_SIZE {
                    // Create a new free block for remaining space
                    debug_assert!(used_end % ALIGNMENT == 0);
                    let new_block = unsafe { FreeBlock::new(used_end, remaining) };
                    self.add_free_block(new_block);
                }

                // Write the allocation header at the start of the block
                let header = block_start as *mut AllocationHeader;
                unsafe {
                    (*header).magic = HEADER_MAGIC;
                    (*header).task_id = encode_task_id(get_current_task_id());
                    (*header).block_size = alloc_block_size;
                }

                // Notify memory visualizer of allocation (from block_start)
                crate::memvis::on_alloc(block_start, alloc_block_size);

                let user_addr = Self::align_up(block_start + HEADER_SIZE, ALIGNMENT);
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

        // Header is immediately before user data, and lives at block_start.
        let header = (user_addr - HEADER_SIZE) as *mut AllocationHeader;
        if unsafe { (*header).magic } != HEADER_MAGIC {
            panic!("Invalid heap allocation header");
        }

        let block_start = header as usize;
        let block_size = unsafe { (*header).block_size };
        debug_assert!(block_start % ALIGNMENT == 0);
        debug_assert!(block_size % ALIGNMENT == 0);

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

/// Check if interrupts are currently enabled
fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem, preserves_flags));
    }
    flags & 0x200 != 0  // IF flag is bit 9
}

/// Disable interrupts and return whether they were enabled
fn disable_interrupts() -> bool {
    let was_enabled = interrupts_enabled();
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
    was_enabled
}

/// Re-enable interrupts
fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

/// Simple spinlock implementation that disables interrupts while held
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
        // Disable interrupts first to prevent interrupt handlers from trying to acquire
        let interrupts_were_enabled = disable_interrupts();

        // In single-threaded cooperative scheduling with interrupts disabled,
        // contention should never occur.
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
            // Re-enable interrupts before panicking
            if interrupts_were_enabled {
                enable_interrupts();
            }
            panic!("Allocator lock contention - this should never happen!");
        }
        SpinlockGuard {
            lock: self,
            interrupts_were_enabled,
        }
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
    interrupts_were_enabled: bool,
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

        // Restore interrupt state
        if self.interrupts_were_enabled {
            enable_interrupts();
        }
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

    let mut cursor = allocator.heap_start;
    while cursor < allocator.heap_end {
        // Free block?
        let mut current = allocator.head;
        let mut free_size = None;
        while let Some(block_ptr) = current {
            let block_start = block_ptr.as_ptr() as usize;
            if block_start == cursor {
                let block = unsafe { block_ptr.as_ref() };
                free_size = Some(block.size);
                break;
            }
            current = unsafe { block_ptr.as_ref().next };
        }

        if let Some(size) = free_size {
            let end = cursor + size;
            if addr >= cursor && addr < end {
                return None;
            }
            cursor = end;
            continue;
        }

        // Allocated block.
        let header = unsafe { &*(cursor as *const AllocationHeader) };
        if header.magic != HEADER_MAGIC {
            return None;
        }
        if header.block_size < MIN_BLOCK_SIZE || header.block_size % ALIGNMENT != 0 {
            return None;
        }
        let end = cursor.saturating_add(header.block_size);
        if end > allocator.heap_end {
            return None;
        }
        if addr >= cursor && addr < end {
            return Some((cursor, end));
        }
        cursor = end;
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

    let mut cursor = allocator.heap_start;
    while cursor < allocator.heap_end {
        // Free block?
        let mut current = allocator.head;
        let mut free_size = None;
        while let Some(block_ptr) = current {
            let block_start = block_ptr.as_ptr() as usize;
            if block_start == cursor {
                let block = unsafe { block_ptr.as_ref() };
                free_size = Some(block.size);
                break;
            }
            current = unsafe { block_ptr.as_ref().next };
        }

        if let Some(size) = free_size {
            let end = cursor + size;
            if addr >= cursor && addr < end {
                return Some((cursor, end));
            }
            cursor = end;
            continue;
        }

        // Allocated block.
        let header = unsafe { &*(cursor as *const AllocationHeader) };
        if header.magic != HEADER_MAGIC {
            return None;
        }
        if header.block_size < MIN_BLOCK_SIZE || header.block_size % ALIGNMENT != 0 {
            return None;
        }
        let end = cursor.saturating_add(header.block_size);
        if end > allocator.heap_end {
            return None;
        }
        cursor = end;
    }

    None
}

/// Find which task owns the allocation at the given address
///
/// Returns Some(task_id) if the address is in an allocation,
/// where task_id is None for kernel/boot allocations.
/// Returns None if the address is not in an allocation.
pub fn find_allocation_owner(addr: usize) -> Option<Option<TaskId>> {
    let allocator = ALLOCATOR.inner.lock();

    if addr < allocator.heap_start || addr >= allocator.heap_end {
        return None;
    }

    let mut cursor = allocator.heap_start;
    while cursor < allocator.heap_end {
        // Free block?
        let mut current = allocator.head;
        let mut free_size = None;
        while let Some(block_ptr) = current {
            let block_start = block_ptr.as_ptr() as usize;
            if block_start == cursor {
                let block = unsafe { block_ptr.as_ref() };
                free_size = Some(block.size);
                break;
            }
            current = unsafe { block_ptr.as_ref().next };
        }

        if let Some(size) = free_size {
            let end = cursor + size;
            if addr >= cursor && addr < end {
                return None;
            }
            cursor = end;
            continue;
        }

        // Allocated block.
        let header = unsafe { &*(cursor as *const AllocationHeader) };
        if header.magic != HEADER_MAGIC {
            return None;
        }
        if header.block_size < MIN_BLOCK_SIZE || header.block_size % ALIGNMENT != 0 {
            return None;
        }
        let end = cursor.saturating_add(header.block_size);
        if end > allocator.heap_end {
            return None;
        }

        if addr >= cursor && addr < end {
            return Some(decode_task_id(header.task_id));
        }

        cursor = end;
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

    // Track bytes per task (up to 8 tasks)
    const MAX_TASKS: usize = 8;
    let mut task_bytes: [(Option<TaskId>, usize); MAX_TASKS] = [(None, 0); MAX_TASKS];
    let mut num_tasks = 0;

    let mut cursor = allocator.heap_start;
    while cursor < allocator.heap_end {
        // Free block?
        let mut current = allocator.head;
        let mut free_size = None;
        while let Some(block_ptr) = current {
            let block_start = block_ptr.as_ptr() as usize;
            if block_start == cursor {
                let block = unsafe { block_ptr.as_ref() };
                free_size = Some(block.size);
                break;
            }
            current = unsafe { block_ptr.as_ref().next };
        }

        if let Some(size) = free_size {
            cursor += size;
            continue;
        }

        // Allocated block.
        let header = unsafe { &*(cursor as *const AllocationHeader) };
        if header.magic != HEADER_MAGIC {
            break;
        }
        if header.block_size < MIN_BLOCK_SIZE || header.block_size % ALIGNMENT != 0 {
            break;
        }
        let alloc_start = cursor;
        let alloc_end = cursor.saturating_add(header.block_size);
        if alloc_end > allocator.heap_end {
            break;
        }

        // Calculate overlap with range
        if alloc_end > range_start && alloc_start < range_end {
            let overlap_start = alloc_start.max(range_start);
            let overlap_end = alloc_end.min(range_end);
            let overlap_bytes = overlap_end - overlap_start;
            if overlap_bytes > 0 {
                let task_id = decode_task_id(header.task_id);

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
        }

        cursor = alloc_end;
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

/// Snapshot heap allocations for a task into a caller-provided buffer.
///
/// Returns the number of entries written (truncates to `out.len()`).
fn snapshot_task_heap_allocations(task_id: Option<TaskId>, out: &mut [(usize, usize)]) -> usize {
    let allocator = ALLOCATOR.inner.lock();
    let mut written = 0usize;
    let want = encode_task_id(task_id);

    let mut cursor = allocator.heap_start;
    while cursor < allocator.heap_end {
        // Free block?
        let mut current = allocator.head;
        let mut free_size = None;
        while let Some(block_ptr) = current {
            let block_start = block_ptr.as_ptr() as usize;
            if block_start == cursor {
                let block = unsafe { block_ptr.as_ref() };
                free_size = Some(block.size);
                break;
            }
            current = unsafe { block_ptr.as_ref().next };
        }

        if let Some(size) = free_size {
            cursor += size;
            continue;
        }

        // Allocated block.
        let header = unsafe { &*(cursor as *const AllocationHeader) };
        if header.magic != HEADER_MAGIC {
            break;
        }
        if header.block_size < MIN_BLOCK_SIZE || header.block_size % ALIGNMENT != 0 {
            break;
        }
        let alloc_start = cursor;
        let alloc_end = cursor.saturating_add(header.block_size);
        if alloc_end > allocator.heap_end {
            break;
        }

        if header.task_id == want {
            if written >= out.len() {
                break;
            }
            out[written] = (alloc_start, header.block_size);
            written += 1;
        }

        cursor = alloc_end;
    }

    written
}

/// Get all heap allocations for a specific task
///
/// Returns a list of (start_addr, size) for all heap allocations made by the task.
/// Use task_id = None to get kernel/boot allocations.
pub fn get_task_heap_allocations(task_id: Option<TaskId>) -> alloc::vec::Vec<(usize, usize)> {
    // Important: don't allocate while holding the allocator lock.
    // Otherwise, we'd re-enter the global allocator and trigger lock contention.
    const MAX_SNAPSHOT_ALLOCS: usize = 256;
    let mut snapshot = [(0usize, 0usize); MAX_SNAPSHOT_ALLOCS];
    let count = snapshot_task_heap_allocations(task_id, &mut snapshot);
    snapshot[..count].to_vec()
}
