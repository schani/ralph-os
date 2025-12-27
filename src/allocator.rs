//! Linked List Heap Allocator
//!
//! A simple first-fit linked list allocator implemented from scratch.
//! Supports allocation and deallocation with proper alignment handling.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

/// Minimum block size (must fit a FreeBlock header)
const MIN_BLOCK_SIZE: usize = core::mem::size_of::<FreeBlock>();

/// A free memory block in the linked list
#[repr(C)]
struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

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
        let size = layout.size().max(MIN_BLOCK_SIZE);
        let align = layout.align().max(core::mem::align_of::<FreeBlock>());

        // First-fit search
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(block_ptr) = current {
            let block = unsafe { block_ptr.as_ref() };
            let block_start = block_ptr.as_ptr() as usize;
            let block_size = block.size;

            // Calculate aligned start address within this block
            let aligned_start = Self::align_up(block_start, align);
            let alignment_padding = aligned_start - block_start;

            // Check if block is large enough
            if block_size >= alignment_padding + size {
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

                // Handle leftover space at the beginning (due to alignment)
                if alignment_padding >= MIN_BLOCK_SIZE {
                    // Create a new free block for the alignment padding
                    let new_block = unsafe { FreeBlock::new(block_start, alignment_padding) };
                    self.add_free_block(new_block);
                }

                // Handle leftover space at the end
                let used_end = aligned_start + size;
                let remaining = block_start + block_size - used_end;
                if remaining >= MIN_BLOCK_SIZE {
                    // Create a new free block for remaining space
                    let new_block = unsafe { FreeBlock::new(used_end, remaining) };
                    self.add_free_block(new_block);
                }

                return aligned_start as *mut u8;
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
    pub unsafe fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(MIN_BLOCK_SIZE);
        let addr = ptr as usize;

        // Create a new free block
        let block = FreeBlock::new(addr, size);
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
