//! Executable Loading and Registry
//!
//! Manages embedded executables: discovering them in the disk image,
//! loading them into program memory, and cleaning up when they exit.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::elf;
use crate::program_alloc;
use crate::task::TaskId;

/// Magic bytes for the executable table header
const EXEC_TABLE_MAGIC: [u8; 4] = *b"REXE";

/// Maximum number of executables in the table
const MAX_EXECUTABLES: usize = 15;

/// Executable table header (matches disk format)
#[repr(C)]
struct ExecTableHeader {
    /// Magic bytes "REXE"
    magic: [u8; 4],
    /// Table format version
    version: u32,
    /// Number of executables in table
    exec_count: u32,
    /// Reserved for future use
    _reserved: u32,
    /// Executable entries
    entries: [ExecEntry; MAX_EXECUTABLES],
}

/// Single executable entry in the table
#[repr(C)]
#[derive(Clone, Copy)]
struct ExecEntry {
    /// Null-terminated name (max 15 chars + null)
    name: [u8; 16],
    /// Byte offset from header start
    offset: u32,
    /// Size in bytes
    size: u32,
    /// Reserved
    _reserved: [u32; 2],
}

/// Information about a loaded program
#[derive(Debug, Clone)]
pub struct LoadedProgram {
    /// Name of the program
    pub name: String,
    /// Base address where loaded
    pub base_addr: usize,
    /// Size of allocated region
    pub size: usize,
    /// Entry point address
    pub entry: usize,
}

/// Errors that can occur during executable operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    /// Executable subsystem not initialized
    NotInitialized,
    /// No executable table found in memory
    NoTableFound,
    /// Executable not found by name
    NotFound,
    /// Failed to parse ELF
    ElfError(elf::ElfError),
    /// Failed to allocate program memory
    AllocationFailed,
    /// Invalid executable table
    InvalidTable,
}

impl From<elf::ElfError> for ExecError {
    fn from(e: elf::ElfError) -> Self {
        ExecError::ElfError(e)
    }
}

/// All memory allocations belonging to a single task
struct TaskAllocations {
    /// Stack allocation (base, size) - always present
    stack: (usize, usize),
    /// Program code/data allocation - only for loaded ELF programs
    /// Tuple is (base_addr, size, program_name)
    program: Option<(usize, usize, String)>,
    /// User heap allocations via alloc() API - list of (addr, size)
    heap_blocks: Vec<(usize, usize)>,
}

impl TaskAllocations {
    fn new(stack_base: usize, stack_size: usize) -> Self {
        TaskAllocations {
            stack: (stack_base, stack_size),
            program: None,
            heap_blocks: Vec::new(),
        }
    }
}

/// Executable registry state
struct ExecRegistry {
    /// Address of the executable table header
    table_addr: usize,
    /// Number of executables available
    exec_count: usize,
    /// Per-task memory allocations (stack + program + heap)
    task_allocations: BTreeMap<TaskId, TaskAllocations>,
}

impl ExecRegistry {
    const fn new() -> Self {
        ExecRegistry {
            table_addr: 0,
            exec_count: 0,
            task_allocations: BTreeMap::new(),
        }
    }
}

/// Thread-safe registry cell
struct RegistryCell {
    inner: UnsafeCell<ExecRegistry>,
    initialized: AtomicBool,
}

// Safety: Single-threaded cooperative scheduling
unsafe impl Sync for RegistryCell {}

impl RegistryCell {
    const fn new() -> Self {
        RegistryCell {
            inner: UnsafeCell::new(ExecRegistry::new()),
            initialized: AtomicBool::new(false),
        }
    }

    fn init(&self, table_addr: usize, exec_count: usize) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            panic!("Executable registry already initialized");
        }
        unsafe {
            let reg = &mut *self.inner.get();
            reg.table_addr = table_addr;
            reg.exec_count = exec_count;
        }
    }

    fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ExecRegistry) -> R,
    {
        assert!(
            self.initialized.load(Ordering::SeqCst),
            "Executable registry not initialized"
        );
        unsafe { f(&mut *self.inner.get()) }
    }

    fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }
}

static REGISTRY: RegistryCell = RegistryCell::new();

// Symbol exported by linker script - end of kernel
// We declare this as a function to get its address without dereferencing
extern "C" {
    fn __kernel_end();
}

/// Initialize the executable subsystem
///
/// Searches for the executable table header after the kernel.
pub fn init() -> Result<usize, ExecError> {
    // Initialize program allocator first
    unsafe {
        program_alloc::init();
    }

    // Search for the executable table by looking for "REXE" magic
    // The table is somewhere after the kernel (0x100000) but before the heap (0x200000)
    // Search at 512-byte (sector) boundaries since that's how the disk is organized
    let search_start = 0x100000usize; // Kernel starts here
    let search_end = 0x200000usize;   // Heap starts here

    let mut table_addr = None;

    // Search for the magic header
    // The table could be at any address (not necessarily sector-aligned)
    // because the kernel binary size may not be a multiple of 512
    // Search in 4-byte increments (aligned for the u32 magic)
    let mut addr = search_start;
    while addr < search_end - 4 {
        let magic = unsafe { core::ptr::read(addr as *const [u8; 4]) };
        if magic == EXEC_TABLE_MAGIC {
            // Found potential table - validate it
            if validate_table(addr) {
                table_addr = Some(addr);
                break;
            }
        }
        addr += 4; // Search at 4-byte boundaries (u32 aligned)
    }

    match table_addr {
        Some(addr) => {
            let header = unsafe { &*(addr as *const ExecTableHeader) };
            let count = header.exec_count as usize;

            crate::println!(
                "Found executable table at 0x{:X} with {} executables",
                addr,
                count
            );

            REGISTRY.init(addr, count);
            Ok(count)
        }
        None => {
            crate::println!("No executable table found (searched 0x{:X}-0x{:X})", search_start, search_end);
            REGISTRY.init(0, 0);
            Ok(0)
        }
    }
}

/// Validate an executable table at the given address
fn validate_table(addr: usize) -> bool {
    let header = unsafe { &*(addr as *const ExecTableHeader) };

    // Check magic (already checked before calling this, but be safe)
    if header.magic != EXEC_TABLE_MAGIC {
        return false;
    }

    // Check version
    if header.version != 1 {
        return false;
    }

    // Check count is reasonable
    if header.exec_count as usize > MAX_EXECUTABLES {
        return false;
    }

    // Check that each entry has reasonable values
    for i in 0..header.exec_count as usize {
        let entry = &header.entries[i];
        // Offset should be positive and size should be non-zero
        if entry.offset == 0 || entry.size == 0 {
            return false;
        }
    }

    true
}

/// List all available executables
pub fn list() -> Vec<String> {
    if !REGISTRY.is_initialized() {
        return Vec::new();
    }

    REGISTRY.with(|reg| {
        if reg.table_addr == 0 {
            return Vec::new();
        }

        let header = unsafe { &*(reg.table_addr as *const ExecTableHeader) };
        let mut names = Vec::new();

        for i in 0..reg.exec_count {
            let entry = &header.entries[i];
            let name = entry_name(entry);
            names.push(name);
        }

        names
    })
}

/// Get the name from an executable entry
fn entry_name(entry: &ExecEntry) -> String {
    let len = entry
        .name
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(entry.name.len());
    String::from_utf8_lossy(&entry.name[..len]).into_owned()
}

/// Find an executable by name
fn find_executable(name: &str) -> Result<(usize, usize), ExecError> {
    REGISTRY.with(|reg| {
        if reg.table_addr == 0 {
            return Err(ExecError::NoTableFound);
        }

        let header = unsafe { &*(reg.table_addr as *const ExecTableHeader) };

        for i in 0..reg.exec_count {
            let entry = &header.entries[i];
            let entry_name = entry_name(entry);

            if entry_name == name {
                // Calculate ELF data address
                let elf_addr = reg.table_addr + entry.offset as usize;
                let elf_size = entry.size as usize;
                return Ok((elf_addr, elf_size));
            }
        }

        Err(ExecError::NotFound)
    })
}

/// Load an executable into memory
///
/// Returns information about the loaded program including the entry point.
pub fn load(name: &str) -> Result<LoadedProgram, ExecError> {
    if !REGISTRY.is_initialized() {
        return Err(ExecError::NotInitialized);
    }

    // Find the executable in the table
    let (elf_addr, elf_size) = find_executable(name)?;

    // Get ELF data
    let elf_data = unsafe { core::slice::from_raw_parts(elf_addr as *const u8, elf_size) };

    // Parse ELF to get memory requirements
    let elf = elf::Elf::parse(elf_data)?;
    let (_, mem_size) = elf.memory_requirements()?;

    // Allocate program memory
    let base_addr = program_alloc::allocate(mem_size).ok_or(ExecError::AllocationFailed)?;

    // Load ELF into allocated memory
    let entry = unsafe { elf::load_elf(elf_data, base_addr)? };

    crate::println!(
        "Loaded '{}' at 0x{:X} (size: {} bytes, entry: 0x{:X})",
        name,
        base_addr,
        mem_size,
        entry
    );

    Ok(LoadedProgram {
        name: String::from(name),
        base_addr,
        size: mem_size,
        entry,
    })
}

/// Read an embedded file's raw bytes by name.
///
/// The executable table is also used as a simple "faux filesystem" for
/// non-ELF assets (e.g. BASIC `.bas` source). The caller is responsible for
/// interpreting the returned bytes.
pub fn read(name: &str) -> Result<&'static [u8], ExecError> {
    if !REGISTRY.is_initialized() {
        return Err(ExecError::NotInitialized);
    }

    let (addr, size) = find_executable(name)?;
    Ok(unsafe { core::slice::from_raw_parts(addr as *const u8, size) })
}

/// Register a task's stack allocation
///
/// Called when a task is created. Must be called before the task runs.
pub fn register_task_stack(task_id: TaskId, stack_base: usize, stack_size: usize) {
    REGISTRY.with(|reg| {
        reg.task_allocations
            .insert(task_id, TaskAllocations::new(stack_base, stack_size));
    });
}

/// Register a task's program memory (for loaded ELF programs)
///
/// Called after loading an ELF executable for a task.
pub fn register_task_program(task_id: TaskId, base_addr: usize, size: usize, name: &str) {
    REGISTRY.with(|reg| {
        if let Some(allocs) = reg.task_allocations.get_mut(&task_id) {
            allocs.program = Some((base_addr, size, String::from(name)));
        }
    });
}

/// Allocate heap memory for a task
///
/// Allocations are rounded up to 4KB multiples.
/// Returns the allocation address, or None if allocation fails.
pub fn task_alloc(task_id: TaskId, size: usize) -> Option<usize> {
    if size == 0 {
        return None;
    }

    // Round up to 4KB multiple
    let aligned_size = (size + 0xFFF) & !0xFFF;

    // Allocate from program region
    let addr = program_alloc::allocate(aligned_size)?;

    // Track the allocation
    REGISTRY.with(|reg| {
        if let Some(allocs) = reg.task_allocations.get_mut(&task_id) {
            allocs.heap_blocks.push((addr, aligned_size));
        }
    });

    Some(addr)
}

/// Free heap memory for a task
///
/// Verifies the pointer belongs to this task before freeing.
/// Returns true if freed, false if pointer not found or wrong task.
pub fn task_free(task_id: TaskId, ptr: usize) -> bool {
    REGISTRY.with(|reg| {
        if let Some(allocs) = reg.task_allocations.get_mut(&task_id) {
            // Find the allocation in this task's heap_blocks
            if let Some(idx) = allocs.heap_blocks.iter().position(|(addr, _)| *addr == ptr) {
                let (addr, size) = allocs.heap_blocks.remove(idx);
                unsafe {
                    program_alloc::deallocate(addr, size);
                }
                return true;
            }
        }
        false
    })
}

/// Unload all memory for a task when it exits
///
/// Called by the scheduler when a task finishes.
/// Frees: heap blocks, program memory, and stack.
pub fn unload_task(task_id: TaskId) {
    if !REGISTRY.is_initialized() {
        return;
    }

    REGISTRY.with(|reg| {
        if let Some(allocs) = reg.task_allocations.remove(&task_id) {
            // Free all heap blocks first
            for (addr, size) in allocs.heap_blocks {
                unsafe {
                    program_alloc::deallocate(addr, size);
                }
            }

            // Free program memory if any
            if let Some((addr, size, ref name)) = allocs.program {
                unsafe {
                    program_alloc::deallocate(addr, size);
                }
                crate::println!(
                    "Unloaded '{}' at 0x{:X} (task {})",
                    name,
                    addr,
                    task_id
                );
            }

            // Free stack
            let (stack_addr, stack_size) = allocs.stack;
            unsafe {
                program_alloc::deallocate(stack_addr, stack_size);
            }
        }
    });
}

/// Get program memory statistics
pub fn memory_stats() -> (usize, usize) {
    program_alloc::stats()
}

/// Information about a task's memory allocations (for MEMSTATS)
#[derive(Debug)]
pub struct TaskMemoryInfo {
    /// Task ID
    pub task_id: TaskId,
    /// Stack allocation (base, size)
    pub stack: (usize, usize),
    /// Program allocation if any (base, size, name)
    pub program: Option<(usize, usize, String)>,
    /// Heap blocks (list of (addr, size))
    pub heap_blocks: Vec<(usize, usize)>,
}

/// Get memory allocations for all tasks
///
/// Returns a vector of TaskMemoryInfo for all registered tasks.
pub fn get_all_task_memory() -> Vec<TaskMemoryInfo> {
    if !REGISTRY.is_initialized() {
        return Vec::new();
    }

    REGISTRY.with(|reg| {
        reg.task_allocations
            .iter()
            .map(|(&task_id, allocs)| TaskMemoryInfo {
                task_id,
                stack: allocs.stack,
                program: allocs.program.clone(),
                heap_blocks: allocs.heap_blocks.clone(),
            })
            .collect()
    })
}

/// Find the program that owns a given address
///
/// Returns Some((start, end, name)) if the address is in a loaded program's memory,
/// or None if not found.
pub fn find_program_by_addr(addr: usize) -> Option<(usize, usize, &'static str)> {
    if !REGISTRY.is_initialized() {
        return None;
    }

    REGISTRY.with(|reg| {
        for allocs in reg.task_allocations.values() {
            if let Some((base, size, ref name)) = allocs.program {
                if addr >= base && addr < base + size {
                    // Safety: We leak the string to get a 'static lifetime
                    // This is acceptable because program names are small and
                    // programs don't unload frequently
                    let name_static: &'static str = unsafe {
                        &*(name.as_str() as *const str)
                    };
                    return Some((base, base + size, name_static));
                }
            }
        }
        None
    })
}

/// Find which task owns a given address in the program region
///
/// Checks stack, program code, and heap blocks for all tasks.
/// Returns Some(task_id) if found, None otherwise.
pub fn find_task_by_program_addr(addr: usize) -> Option<TaskId> {
    if !REGISTRY.is_initialized() {
        return None;
    }

    REGISTRY.with(|reg| {
        for (&task_id, allocs) in reg.task_allocations.iter() {
            // Check stack
            let (stack_base, stack_size) = allocs.stack;
            if addr >= stack_base && addr < stack_base + stack_size {
                return Some(task_id);
            }

            // Check program code
            if let Some((prog_base, prog_size, _)) = &allocs.program {
                if addr >= *prog_base && addr < *prog_base + *prog_size {
                    return Some(task_id);
                }
            }

            // Check heap blocks
            for &(block_base, block_size) in &allocs.heap_blocks {
                if addr >= block_base && addr < block_base + block_size {
                    return Some(task_id);
                }
            }
        }
        None
    })
}
