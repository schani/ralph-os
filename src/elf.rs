//! ELF64 Parser for Position-Independent Executables
//!
//! Parses ELF64 headers and program headers to load PIE executables.
//! Only supports x86_64 little-endian executables.

/// ELF magic bytes
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF class: 64-bit
const ELFCLASS64: u8 = 2;

/// ELF data encoding: little-endian
const ELFDATA2LSB: u8 = 1;

/// ELF type: executable
const ET_EXEC: u16 = 2;

/// ELF type: shared object (also used for PIE)
const ET_DYN: u16 = 3;

/// Machine type: x86_64
const EM_X86_64: u16 = 62;

/// Program header type: loadable segment
const PT_LOAD: u32 = 1;

/// ELF64 file header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    /// Magic number and other info
    pub e_ident: [u8; 16],
    /// Object file type
    pub e_type: u16,
    /// Architecture
    pub e_machine: u16,
    /// Object file version
    pub e_version: u32,
    /// Entry point virtual address
    pub e_entry: u64,
    /// Program header table file offset
    pub e_phoff: u64,
    /// Section header table file offset
    pub e_shoff: u64,
    /// Processor-specific flags
    pub e_flags: u32,
    /// ELF header size
    pub e_ehsize: u16,
    /// Program header table entry size
    pub e_phentsize: u16,
    /// Program header table entry count
    pub e_phnum: u16,
    /// Section header table entry size
    pub e_shentsize: u16,
    /// Section header table entry count
    pub e_shnum: u16,
    /// Section name string table index
    pub e_shstrndx: u16,
}

/// ELF64 program header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeader {
    /// Segment type
    pub p_type: u32,
    /// Segment flags
    pub p_flags: u32,
    /// Segment file offset
    pub p_offset: u64,
    /// Segment virtual address
    pub p_vaddr: u64,
    /// Segment physical address
    pub p_paddr: u64,
    /// Segment size in file
    pub p_filesz: u64,
    /// Segment size in memory
    pub p_memsz: u64,
    /// Segment alignment
    pub p_align: u64,
}

/// Errors that can occur during ELF parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    /// Data too small to contain ELF header
    TooSmall,
    /// Invalid ELF magic number
    InvalidMagic,
    /// Not a 64-bit ELF
    Not64Bit,
    /// Not little-endian
    NotLittleEndian,
    /// Not an executable or PIE
    NotExecutable,
    /// Not x86_64 architecture
    NotX86_64,
    /// Invalid program header
    InvalidProgramHeader,
    /// No loadable segments found
    NoLoadableSegments,
}

/// Parsed ELF file information
pub struct Elf<'a> {
    /// Raw ELF data
    data: &'a [u8],
    /// Parsed header
    header: &'a Elf64Header,
}

impl<'a> Elf<'a> {
    /// Parse an ELF file from raw bytes
    pub fn parse(data: &'a [u8]) -> Result<Self, ElfError> {
        // Check minimum size
        if data.len() < core::mem::size_of::<Elf64Header>() {
            return Err(ElfError::TooSmall);
        }

        // Get header reference
        let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

        // Validate magic
        if header.e_ident[0..4] != ELF_MAGIC {
            return Err(ElfError::InvalidMagic);
        }

        // Validate class (64-bit)
        if header.e_ident[4] != ELFCLASS64 {
            return Err(ElfError::Not64Bit);
        }

        // Validate endianness (little-endian)
        if header.e_ident[5] != ELFDATA2LSB {
            return Err(ElfError::NotLittleEndian);
        }

        // Validate type (executable or PIE)
        if header.e_type != ET_EXEC && header.e_type != ET_DYN {
            return Err(ElfError::NotExecutable);
        }

        // Validate architecture (x86_64)
        if header.e_machine != EM_X86_64 {
            return Err(ElfError::NotX86_64);
        }

        Ok(Elf { data, header })
    }

    /// Check if this is a PIE executable
    pub fn is_pie(&self) -> bool {
        self.header.e_type == ET_DYN
    }

    /// Get the entry point offset (relative to load base for PIE)
    pub fn entry_offset(&self) -> u64 {
        self.header.e_entry
    }

    /// Get the number of program headers
    pub fn program_header_count(&self) -> usize {
        self.header.e_phnum as usize
    }

    /// Get a program header by index
    pub fn program_header(&self, index: usize) -> Result<&'a Elf64ProgramHeader, ElfError> {
        if index >= self.header.e_phnum as usize {
            return Err(ElfError::InvalidProgramHeader);
        }

        let offset = self.header.e_phoff as usize + index * self.header.e_phentsize as usize;
        let end = offset + core::mem::size_of::<Elf64ProgramHeader>();

        if end > self.data.len() {
            return Err(ElfError::InvalidProgramHeader);
        }

        let phdr = unsafe { &*(self.data.as_ptr().add(offset) as *const Elf64ProgramHeader) };
        Ok(phdr)
    }

    /// Iterate over loadable (PT_LOAD) segments
    pub fn loadable_segments(&self) -> impl Iterator<Item = &'a Elf64ProgramHeader> + '_ {
        (0..self.program_header_count()).filter_map(move |i| {
            self.program_header(i).ok().filter(|ph| ph.p_type == PT_LOAD)
        })
    }

    /// Calculate total memory required to load all segments
    ///
    /// Returns (lowest_vaddr, total_size) where total_size is the span from
    /// the lowest to highest address of any loadable segment.
    pub fn memory_requirements(&self) -> Result<(u64, usize), ElfError> {
        let mut lowest: Option<u64> = None;
        let mut highest: u64 = 0;

        for phdr in self.loadable_segments() {
            let start = phdr.p_vaddr;
            let end = phdr.p_vaddr + phdr.p_memsz;

            match lowest {
                None => lowest = Some(start),
                Some(low) if start < low => lowest = Some(start),
                _ => {}
            }

            if end > highest {
                highest = end;
            }
        }

        match lowest {
            None => Err(ElfError::NoLoadableSegments),
            Some(low) => Ok((low, (highest - low) as usize)),
        }
    }

    /// Get the segment data for a program header
    pub fn segment_data(&self, phdr: &Elf64ProgramHeader) -> &'a [u8] {
        let offset = phdr.p_offset as usize;
        let size = phdr.p_filesz as usize;

        if offset + size <= self.data.len() {
            &self.data[offset..offset + size]
        } else {
            &[]
        }
    }
}

/// Load an ELF file into memory at a given base address
///
/// # Arguments
/// * `data` - Raw ELF file data
/// * `base_addr` - Address where the program should be loaded
///
/// # Returns
/// * Entry point address (absolute)
///
/// # Safety
/// * base_addr must point to a valid, writable memory region large enough
///   to hold the entire program
pub unsafe fn load_elf(data: &[u8], base_addr: usize) -> Result<usize, ElfError> {
    let elf = Elf::parse(data)?;
    let (lowest_vaddr, _) = elf.memory_requirements()?;

    // Load each segment
    for phdr in elf.loadable_segments() {
        // Calculate destination address
        let dest = base_addr + (phdr.p_vaddr - lowest_vaddr) as usize;
        let dest_ptr = dest as *mut u8;

        // Copy file contents
        let src = elf.segment_data(phdr);
        if !src.is_empty() {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dest_ptr, src.len());
        }

        // Zero BSS (memsz - filesz)
        let bss_size = (phdr.p_memsz - phdr.p_filesz) as usize;
        if bss_size > 0 {
            let bss_ptr = dest_ptr.add(phdr.p_filesz as usize);
            core::ptr::write_bytes(bss_ptr, 0, bss_size);
        }
    }

    // Calculate entry point
    let entry = base_addr + (elf.entry_offset() - lowest_vaddr) as usize;
    Ok(entry)
}
