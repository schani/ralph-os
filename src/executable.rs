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

/// Executable registry state
struct ExecRegistry {
    /// Address of the executable table header
    table_addr: usize,
    /// Number of executables available
    exec_count: usize,
    /// Mapping from task ID to loaded program info (for cleanup)
    loaded_programs: BTreeMap<TaskId, (usize, usize)>, // (addr, size)
}

impl ExecRegistry {
    const fn new() -> Self {
        ExecRegistry {
            table_addr: 0,
            exec_count: 0,
            loaded_programs: BTreeMap::new(),
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

/// Register a loaded program with a task ID for cleanup
pub fn register_task(task_id: TaskId, program: &LoadedProgram) {
    REGISTRY.with(|reg| {
        reg.loaded_programs
            .insert(task_id, (program.base_addr, program.size));
    });
}

/// Unload a program when its task exits
///
/// Called by the scheduler when a program task finishes.
pub fn unload_task(task_id: TaskId) {
    if !REGISTRY.is_initialized() {
        return;
    }

    REGISTRY.with(|reg| {
        if let Some((addr, size)) = reg.loaded_programs.remove(&task_id) {
            unsafe {
                program_alloc::deallocate(addr, size);
            }
            crate::println!(
                "Unloaded program at 0x{:X} (task {})",
                addr,
                task_id
            );
        }
    });
}

/// Get program memory statistics
pub fn memory_stats() -> (usize, usize) {
    program_alloc::stats()
}
