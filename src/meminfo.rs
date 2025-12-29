//! Unified Memory Information API
//!
//! Provides a single API for querying memory map information.
//! Used by both the memory visualizer tooltip and the BASIC MEMSTATS command.

use alloc::string::String;
use alloc::vec::Vec;

use crate::allocator;
use crate::executable;
use crate::program_alloc;
use crate::scheduler;
use crate::task::{TaskId, TaskState};

/// Memory region boundaries
pub const KERNEL_START: usize = 0x100000;
pub const KERNEL_END: usize = 0x200000;
pub const HEAP_START: usize = 0x200000;
pub const HEAP_END: usize = 0x400000;
pub const PROGRAM_START: usize = 0x400000;
pub const PROGRAM_END: usize = 0x1000000;

/// Information about a memory region at a specific address
#[derive(Debug, Clone)]
pub struct MemoryRegionInfo {
    /// Start address of the region
    pub start: usize,
    /// End address of the region (exclusive)
    pub end: usize,
    /// Human-readable region name
    pub region_name: &'static str,
    /// Whether this region is allocated (true) or free (false)
    pub is_allocated: bool,
}

/// Find the memory region that contains the given address
///
/// Queries actual allocator data structures to find the exact region boundaries.
pub fn find_region(addr: usize) -> MemoryRegionInfo {
    // Kernel region: 0x100000 - 0x200000 (always "allocated")
    if addr < KERNEL_END {
        return MemoryRegionInfo {
            start: KERNEL_START,
            end: KERNEL_END,
            region_name: "Kernel",
            is_allocated: true,
        };
    }

    // Heap region: 0x200000 - 0x400000
    if addr < HEAP_END {
        // Check if it's an allocation
        if let Some((start, end)) = allocator::find_allocation(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Heap",
                is_allocated: true,
            };
        }
        // Check if it's a free region
        if let Some((start, end)) = allocator::find_free_region(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Heap",
                is_allocated: false,
            };
        }
        // Fallback (shouldn't happen)
        return MemoryRegionInfo {
            start: HEAP_START,
            end: HEAP_END,
            region_name: "Heap",
            is_allocated: false,
        };
    }

    // Program region: 0x400000 - 0x1000000
    if addr < PROGRAM_END {
        // First check if it's a known program (has a name)
        if let Some((start, end, name)) = executable::find_program_by_addr(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: name,
                is_allocated: true,
            };
        }
        // Check if it's an allocation (stack or heap block without program name)
        if let Some((start, end)) = program_alloc::find_allocation(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Stack",
                is_allocated: true,
            };
        }
        // Check if it's a free region
        if let Some((start, end)) = program_alloc::find_free_region(addr) {
            return MemoryRegionInfo {
                start,
                end,
                region_name: "Program",
                is_allocated: false,
            };
        }
        // Fallback (shouldn't happen)
        return MemoryRegionInfo {
            start: PROGRAM_START,
            end: PROGRAM_END,
            region_name: "Program",
            is_allocated: false,
        };
    }

    // Beyond visualized region
    MemoryRegionInfo {
        start: addr,
        end: addr + 256, // One pixel worth
        region_name: "Unknown",
        is_allocated: false,
    }
}

/// Overall memory statistics for each major region
#[derive(Debug, Clone)]
pub struct RegionStats {
    /// Region name
    pub name: &'static str,
    /// Start address
    pub start: usize,
    /// End address
    pub end: usize,
    /// Bytes used/allocated
    pub used: usize,
    /// Bytes free
    pub free: usize,
}

/// Get statistics for all major memory regions
pub fn get_region_stats() -> Vec<RegionStats> {
    let mut stats = Vec::new();

    // Kernel (fixed, all "used")
    stats.push(RegionStats {
        name: "Kernel",
        start: KERNEL_START,
        end: KERNEL_END,
        used: KERNEL_END - KERNEL_START,
        free: 0,
    });

    // Heap
    let (heap_used, heap_free) = allocator::get_heap_stats();
    stats.push(RegionStats {
        name: "Heap",
        start: HEAP_START,
        end: HEAP_END,
        used: heap_used,
        free: heap_free,
    });

    // Program region
    let (prog_used, prog_free) = program_alloc::stats();
    stats.push(RegionStats {
        name: "Program",
        start: PROGRAM_START,
        end: PROGRAM_END,
        used: prog_used,
        free: prog_free,
    });

    stats
}

/// Information about a single task's memory usage
#[derive(Debug, Clone)]
pub struct TaskMemoryInfo {
    /// Task ID
    pub id: TaskId,
    /// Task name
    pub name: &'static str,
    /// Task state
    pub state: TaskState,
    /// Stack allocation (base, size)
    pub stack: Option<(usize, usize)>,
    /// Program code allocation (base, size, program_name)
    pub program: Option<(usize, usize, String)>,
    /// Heap blocks allocated by this task
    pub heap_blocks: Vec<(usize, usize)>,
}

/// Get memory information for all tasks
pub fn get_task_memory_info() -> Vec<TaskMemoryInfo> {
    let tasks = scheduler::get_all_tasks();
    let task_allocs = executable::get_all_task_memory();

    tasks
        .into_iter()
        .map(|task| {
            // Find memory allocations for this task
            let alloc = task_allocs.iter().find(|a| a.task_id == task.id);

            TaskMemoryInfo {
                id: task.id,
                name: task.name,
                state: task.state,
                stack: alloc.map(|a| a.stack),
                program: alloc.and_then(|a| a.program.clone()),
                heap_blocks: alloc.map(|a| a.heap_blocks.clone()).unwrap_or_default(),
            }
        })
        .collect()
}
