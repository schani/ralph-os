//! BASIC interpreter module
//!
//! A simple BASIC interpreter for Ralph OS.
//! Supports: PRINT, LET, IF/THEN, GOTO, FOR/NEXT, SLEEP, MEM()

pub mod value;
pub mod lexer;
pub mod parser;
pub mod interpreter;

pub use value::Value;
pub use interpreter::{Interpreter, ExecutionStatus};
pub use parser::{Parser, Statement};
pub use lexer::Token;

use alloc::string::String;
use crate::scheduler;
use crate::serial;
use crate::meminfo;

/// Print detailed memory statistics using the unified meminfo API
fn print_memstats() {
    crate::println!("=== MEMORY MAP ===");
    crate::println!();

    // Print region statistics
    for region in meminfo::get_region_stats() {
        let total_kb = (region.end - region.start) / 1024;
        crate::println!("{}: 0x{:X} - 0x{:X} ({} KB)",
            region.name, region.start, region.end, total_kb);
        crate::println!("  Used: {} bytes", region.used);
        crate::println!("  Free: {} bytes", region.free);
        crate::println!();
    }

    // Per-task breakdown
    let tasks = meminfo::get_task_memory_info();

    if tasks.is_empty() {
        crate::println!("No tasks running.");
    } else {
        crate::println!("TASKS ({}):", tasks.len());
        for task in &tasks {
            let state_str = match task.state {
                crate::task::TaskState::Ready => "ready",
                crate::task::TaskState::Running => "running",
                crate::task::TaskState::Sleeping => "sleeping",
                crate::task::TaskState::Finished => "finished",
            };
            crate::println!();
            crate::println!("  [{}] {} ({})", task.id, task.name, state_str);

            // Stack
            if let Some((stack_base, stack_size)) = task.stack {
                crate::println!("    Stack: 0x{:X} - 0x{:X} ({} KB)",
                    stack_base, stack_base + stack_size, stack_size / 1024);
            }

            // Program code (if loaded ELF)
            if let Some((prog_base, prog_size, ref prog_name)) = task.program {
                crate::println!("    Code:  0x{:X} - 0x{:X} ({} KB) [{}]",
                    prog_base, prog_base + prog_size, prog_size / 1024, prog_name);
            }

            // Kernel heap allocations (0x200000-0x400000)
            if !task.kernel_heap.is_empty() {
                let total: usize = task.kernel_heap.iter().map(|(_, s)| *s).sum();
                crate::println!("    Kernel heap: {} allocs, {} bytes total",
                    task.kernel_heap.len(), total);
            }

            // Program heap blocks (0x400000-0x1000000)
            if !task.program_heap.is_empty() {
                crate::println!("    Program heap: {} blocks", task.program_heap.len());
                for (addr, size) in &task.program_heap {
                    crate::println!("      0x{:X} - 0x{:X} ({} bytes)",
                        addr, addr + size, size);
                }
            }
        }
    }

    // Show kernel/boot allocations
    let kernel_allocs = meminfo::get_kernel_heap_allocations();
    if !kernel_allocs.is_empty() {
        let total: usize = kernel_allocs.iter().map(|(_, s)| *s).sum();
        crate::println!();
        crate::println!("KERNEL (boot allocations):");
        crate::println!("  Heap: {} allocs, {} bytes total", kernel_allocs.len(), total);
    }

    crate::println!();
}

/// Run a BASIC program headlessly (for background tasks)
pub fn run_headless(source: &str) {
    let mut interp = Interpreter::new();
    if let Err(e) = interp.load_program(source) {
        crate::println!("BASIC load error: {}", e);
        return;
    }
    interp.run();

    while interp.is_running() {
        let status = interp.step();
        match status {
            ExecutionStatus::Sleeping(ms) => {
                scheduler::sleep_ms(ms);
            }
            ExecutionStatus::Ready => {
                scheduler::yield_now();
            }
            ExecutionStatus::Finished | ExecutionStatus::Error(_) => {
                break;
            }
            ExecutionStatus::WaitingForInput => {
                // Headless mode can't handle input
                break;
            }
        }
    }

    if let ExecutionStatus::Error(ref e) = *interp.status() {
        crate::println!("BASIC Error: {}", e);
    }
}

/// Read a line from serial input (with echo and editing)
fn read_line() -> String {
    let mut line = String::new();

    loop {
        // Yield while waiting for input
        if !serial::has_data() {
            scheduler::yield_now();
            continue;
        }

        let byte = serial::read_byte();

        match byte {
            b'\r' | b'\n' => {
                crate::println!(); // Echo newline
                break;
            }
            8 | 127 => {
                // Backspace or DEL
                if !line.is_empty() {
                    line.pop();
                    crate::print!("\x08 \x08"); // Erase character
                }
            }
            b if b >= 32 && b < 127 => {
                // Printable ASCII
                line.push(b as char);
                crate::print!("{}", b as char); // Echo
            }
            _ => {}
        }
    }

    line
}

/// Run the interactive BASIC REPL
pub fn run_repl() {
    crate::println!("Ralph BASIC v1.0");
    crate::println!("Type RUN to execute, LIST to show program, NEW to clear");
    crate::println!("Type LOAD \"name\" to load name.bas");
    crate::println!();

    let mut interp = Interpreter::new();

    loop {
        crate::print!("> ");
        let line = read_line();
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Parse the input
        let mut parser = Parser::new(line);

        // Check for commands
        match parser.current_token() {
            Token::Run => {
                interp.run();
                while interp.is_running() {
                    let status = interp.step();
                    match status {
                        ExecutionStatus::Sleeping(ms) => {
                            scheduler::sleep_ms(ms);
                        }
                        ExecutionStatus::Ready => {
                            scheduler::yield_now();
                        }
                        _ => break,
                    }
                }
                if let ExecutionStatus::Error(ref e) = *interp.status() {
                    crate::println!("Error: {}", e);
                }
                continue;
            }
            Token::List => {
                interp.list();
                continue;
            }
            Token::New => {
                interp.clear();
                crate::println!("Program cleared");
                continue;
            }
            Token::Load => {
                match load_bas_program(&mut interp, line) {
                    Ok(filename) => {
                        crate::println!("Loaded {}", filename);
                    }
                    Err(e) => {
                        crate::println!("Error: {}", e);
                    }
                }
                continue;
            }
            Token::Memstats => {
                print_memstats();
                continue;
            }
            _ => {}
        }

        // Try to parse as a line
        match parser.parse_line() {
            Ok(Some((line_num, stmt))) => {
                if let Some(num) = line_num {
                    // Line with number - add to program
                    interp.set_line(num, stmt);
                } else {
                    // Immediate mode - execute now
                    let status = interp.execute_immediate(&stmt);
                    match status {
                        ExecutionStatus::Sleeping(ms) => {
                            scheduler::sleep_ms(ms);
                        }
                        ExecutionStatus::Error(e) => {
                            crate::println!("Error: {}", e);
                        }
                        _ => {}
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                crate::println!("Syntax error: {}", e.0);
            }
        }
    }
}

fn load_bas_program(interp: &mut Interpreter, input: &str) -> Result<String, String> {
    // Expect: LOAD <name>  OR  LOAD "name"
    let mut parts = input.trim().splitn(2, char::is_whitespace);
    let _cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    if arg.is_empty() {
        return Err("Usage: LOAD \"name\"".into());
    }

    let name = if let Some(stripped) = arg.strip_prefix('"') {
        let Some(end_quote) = stripped.find('"') else {
            return Err("Unterminated string".into());
        };
        stripped[..end_quote].trim()
    } else {
        arg.split_whitespace().next().unwrap_or("")
    };

    if name.is_empty() {
        return Err("Usage: LOAD \"name\"".into());
    }

    let filename = if name.to_ascii_lowercase().ends_with(".bas") {
        String::from(name)
    } else {
        alloc::format!("{}.bas", name)
    };

    let bytes = crate::executable::read(&filename).map_err(|e| alloc::format!("{:?}", e))?;
    let src = core::str::from_utf8(bytes).map_err(|_| String::from("File is not valid UTF-8"))?;

    interp.clear();
    let loaded = interp.load_program(src)?;
    if loaded == 0 {
        return Err(String::from("Loaded 0 lines (file has no numbered program lines?)"));
    }
    Ok(filename)
}

/// Memory monitor task (headless BASIC program)
pub fn memstats_task() {
    let program = r#"
10 REM Memory monitor - runs in background
20 LET U = MEM(0)
30 LET F = MEM(1)
40 PRINT "Heap: "; U; " used, "; F; " free"
50 SLEEP 10000
60 GOTO 20
"#;
    run_headless(program);
}

/// Interactive BASIC REPL task
pub fn repl_task() {
    run_repl();
}
