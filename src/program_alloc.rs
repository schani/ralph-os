//! Program Memory Allocator
//!
//! Manages the 12MB program region (0x400000 - 0x1000000) where loaded
//! executables are placed. Uses first-fit allocation with 4KB alignment.

use core::ptr::NonNull;

/// Start of program memory region (4MB)
pub const PROGRAM_REGION_START: usize = 0x400000;

/// End of program memory region (16MB)
pub const PROGRAM_REGION_END: usize = 0x1000000;

/// Size of program memory region (12MB)
pub const PROGRAM_REGION_SIZE: usize = PROGRAM_REGION_END - PROGRAM_REGION_START;

/// Minimum allocation alignment (4KB page)
const PAGE_SIZE: usize = 4096;

/// Minimum block size (must fit FreeRegion header)
const MIN_BLOCK_SIZE: usize = core::mem::size_of::<FreeRegion>();

/// A free region of memory in the linked list
#[repr(C)]
struct FreeRegion {
    /// Size of this free region (including header)
    size: usize,
    /// Pointer to next free region
    next: Option<NonNull<FreeRegion>>,
}

impl FreeRegion {
    /// Create a new free region at the given address
    ///
    /// # Safety
    /// The address must be valid and properly aligned
    unsafe fn new(addr: usize, size: usize) -> NonNull<FreeRegion> {
        let region = addr as *mut FreeRegion;
        (*region).size = size;
        (*region).next = None;
        NonNull::new_unchecked(region)
    }
}

/// Program memory allocator
pub struct ProgramAllocator {
    /// Head of the free list
    head: Option<NonNull<FreeRegion>>,
    /// Total allocated bytes
    allocated: usize,
}

// Safety: Single-threaded cooperative scheduling
unsafe impl Send for ProgramAllocator {}

impl ProgramAllocator {
    /// Create a new uninitialized allocator
    pub const fn new() -> Self {
        ProgramAllocator {
            head: None,
            allocated: 0,
        }
    }

    /// Initialize the allocator with the program memory region
    ///
    /// # Safety
    /// - Must only be called once
    /// - The program region must not be used by anything else
    pub unsafe fn init(&mut self) {
        // Create a single free region spanning the entire program area
        let region = FreeRegion::new(PROGRAM_REGION_START, PROGRAM_REGION_SIZE);
        self.head = Some(region);
        self.allocated = 0;
    }

    /// Align address up to PAGE_SIZE boundary
    fn align_up(addr: usize) -> usize {
        (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
    }

    /// Allocate a region of memory for a program
    ///
    /// Returns the start address of the allocated region, or None if
    /// there isn't enough contiguous free space.
    ///
    /// The size is rounded up to PAGE_SIZE alignment.
    pub fn allocate(&mut self, size: usize) -> Option<usize> {
        // Round up to page alignment
        let size = Self::align_up(size).max(MIN_BLOCK_SIZE);

        // First-fit search
        let mut prev: Option<NonNull<FreeRegion>> = None;
        let mut current = self.head;

        while let Some(region_ptr) = current {
            let region = unsafe { region_ptr.as_ref() };
            let region_start = region_ptr.as_ptr() as usize;
            let region_size = region.size;

            if region_size >= size {
                // This region is big enough
                let next = region.next;

                // Calculate remaining space
                let remaining = region_size - size;

                if remaining >= MIN_BLOCK_SIZE {
                    // Split: create a new free region for the remainder
                    let new_region = unsafe { FreeRegion::new(region_start + size, remaining) };
                    unsafe {
                        (*new_region.as_ptr()).next = next;
                    }

                    // Update the list
                    match prev {
                        Some(mut prev_ptr) => unsafe {
                            prev_ptr.as_mut().next = Some(new_region);
                        },
                        None => {
                            self.head = Some(new_region);
                        }
                    }
                } else {
                    // Use the entire region
                    match prev {
                        Some(mut prev_ptr) => unsafe {
                            prev_ptr.as_mut().next = next;
                        },
                        None => {
                            self.head = next;
                        }
                    }
                }

                self.allocated += size;
                return Some(region_start);
            }

            prev = current;
            current = region.next;
        }

        // No suitable region found
        None
    }

    /// Deallocate a previously allocated region
    ///
    /// # Safety
    /// - addr must have been returned by a previous allocate() call
    /// - size must match the original allocation size (rounded to PAGE_SIZE)
    pub unsafe fn deallocate(&mut self, addr: usize, size: usize) {
        let size = Self::align_up(size).max(MIN_BLOCK_SIZE);

        // Create a new free region
        let new_region = FreeRegion::new(addr, size);

        // Insert into list sorted by address
        self.add_free_region(new_region);

        // Merge adjacent regions
        self.merge_free_regions();

        self.allocated -= size;
    }

    /// Add a free region to the list (sorted by address)
    fn add_free_region(&mut self, new_region: NonNull<FreeRegion>) {
        let new_addr = new_region.as_ptr() as usize;

        // Find insertion point
        let mut prev: Option<NonNull<FreeRegion>> = None;
        let mut current = self.head;

        while let Some(region_ptr) = current {
            let region_addr = region_ptr.as_ptr() as usize;
            if region_addr > new_addr {
                break;
            }
            prev = current;
            current = unsafe { region_ptr.as_ref().next };
        }

        // Insert the new region
        unsafe {
            (*new_region.as_ptr()).next = current;
        }

        match prev {
            Some(mut prev_ptr) => unsafe {
                prev_ptr.as_mut().next = Some(new_region);
            },
            None => {
                self.head = Some(new_region);
            }
        }
    }

    /// Merge adjacent free regions
    fn merge_free_regions(&mut self) {
        let mut current = self.head;

        while let Some(mut region_ptr) = current {
            let region = unsafe { region_ptr.as_mut() };
            let region_end = region_ptr.as_ptr() as usize + region.size;

            if let Some(next_ptr) = region.next {
                let next_addr = next_ptr.as_ptr() as usize;

                // Check if regions are adjacent
                if region_end == next_addr {
                    // Merge: extend current and skip next
                    let next = unsafe { next_ptr.as_ref() };
                    region.size += next.size;
                    region.next = next.next;
                    // Don't advance - check if we can merge more
                    continue;
                }
            }

            current = region.next;
        }
    }

    /// Get allocation statistics
    ///
    /// Returns (allocated_bytes, free_bytes)
    pub fn stats(&self) -> (usize, usize) {
        let mut free = 0;
        let mut current = self.head;

        while let Some(region_ptr) = current {
            let region = unsafe { region_ptr.as_ref() };
            free += region.size;
            current = region.next;
        }

        (self.allocated, free)
    }
}

// Global program allocator instance with spinlock protection
use crate::allocator::Spinlock;

/// Global program memory allocator
static PROGRAM_ALLOCATOR: Spinlock<ProgramAllocator> = Spinlock::new(ProgramAllocator::new());

/// Initialize the program memory allocator
///
/// # Safety
/// Must be called exactly once during kernel initialization
pub unsafe fn init() {
    PROGRAM_ALLOCATOR.lock().init();
}

/// Allocate memory for a program
///
/// Returns the start address of the allocated region, or None if allocation fails.
pub fn allocate(size: usize) -> Option<usize> {
    PROGRAM_ALLOCATOR.lock().allocate(size)
}

/// Deallocate program memory
///
/// # Safety
/// - addr must have been returned by allocate()
/// - size must match the original allocation
pub unsafe fn deallocate(addr: usize, size: usize) {
    PROGRAM_ALLOCATOR.lock().deallocate(addr, size);
}

/// Get program memory statistics
///
/// Returns (allocated_bytes, free_bytes)
pub fn stats() -> (usize, usize) {
    PROGRAM_ALLOCATOR.lock().stats()
}
