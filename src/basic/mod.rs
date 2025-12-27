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
