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

            // Heap blocks
            if !task.heap_blocks.is_empty() {
                crate::println!("    Heap blocks: {}", task.heap_blocks.len());
                for (addr, size) in &task.heap_blocks {
                    crate::println!("      0x{:X} - 0x{:X} ({} bytes)",
                        addr, addr + size, size);
                }
            }
        }
    }
    crate::println!();
}

/// Run a BASIC program headlessly (for background tasks)
pub fn run_headless(source: &str) {
    let mut interp = Interpreter::new();
    interp.load_program(source);
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
    crate::println!();

    let mut interp = Interpreter::new();

    // Pre-load Fibonacci program
    let preload = r#"
10 REM Fibonacci sequence - first 10 numbers
20 LET A = 0
30 LET B = 1
40 LET N = 0
50 PRINT A
60 LET T = A + B
70 LET A = B
80 LET B = T
90 LET N = N + 1
100 IF N < 10 THEN 50
110 END
"#;
    interp.load_program(preload);

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
