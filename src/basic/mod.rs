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

/// TODO web app server task (headless BASIC program)
pub fn http_server_task() {
    let program = r#"
10 REM === TODO Web App ===
20 DIM TODO$(20)
30 TODOCOUNT = 0
40 CR$ = CHR$(13)
50 LF$ = CHR$(10)
60 CRLF$ = CR$ + LF$

100 REM Create server
110 S = SOCKET()
120 IF S < 0 THEN 9000
130 L = LISTEN(S, 8080)
140 IF L = 0 THEN 9010
150 PRINT "TODO server on port 8080"

200 REM Accept connection
210 C = ACCEPT(S)
220 IF C >= 0 THEN 300
230 SLEEP 50
240 GOTO 200

300 REM Read request
305 WAIT = 0
310 R$ = RECV$(C)
320 IF R$ <> "" THEN 340
325 ST = SOCKSTATE(C)
326 IF ST <> 4 THEN 530
327 WAIT = WAIT + 1
328 IF WAIT > 200 THEN 530
330 SLEEP 10
335 GOTO 310
340 GOSUB 1000

400 REM Route
410 IF PATH$ = "/" THEN 420
415 GOTO 430
420 GOSUB 5000
425 GOTO 500
430 IF PATH$ = "/add" THEN 440
435 GOTO 450
440 GOSUB 6000
445 GOTO 500
450 IF PATH$ = "/delete" THEN 460
455 GOTO 470
460 GOSUB 7000
465 GOTO 500
470 STATUS = 404
475 BODY$ = "<h1>Not Found</h1>"
480 GOSUB 3000

500 REM Send response
510 SEND C, RESP$
520 SLEEP 100
530 CLOSE C
540 GOTO 200

1000 REM Parse request - Input: R$, Output: PATH$, QUERY$
1010 SP = INSTR(R$, " ")
1020 IF SP = 0 THEN 1025
1024 GOTO 1030
1025 PATH$ = "/"
1026 QUERY$ = ""
1027 RETURN
1030 REST$ = MID$(R$, SP + 1, 200)
1040 SP2 = INSTR(REST$, " ")
1050 IF SP2 = 0 THEN 1055
1054 GOTO 1060
1055 FULL$ = REST$
1056 GOTO 1070
1060 FULL$ = LEFT$(REST$, SP2 - 1)
1070 QM = INSTR(FULL$, "?")
1080 IF QM = 0 THEN 1085
1084 GOTO 1090
1085 PATH$ = FULL$
1086 QUERY$ = ""
1087 RETURN
1090 PATH$ = LEFT$(FULL$, QM - 1)
1100 QUERY$ = MID$(FULL$, QM + 1, 200)
1110 RETURN

2000 REM Get param - Input: QUERY$, PNAME$, Output: PVAL$
2010 SEARCH$ = PNAME$ + "="
2020 POS = INSTR(QUERY$, SEARCH$)
2030 IF POS = 0 THEN 2035
2034 GOTO 2040
2035 PVAL$ = ""
2036 RETURN
2040 START = POS + LEN(SEARCH$)
2050 REST$ = MID$(QUERY$, START, 200)
2060 AMP = INSTR(REST$, "&")
2070 IF AMP = 0 THEN 2075
2074 GOTO 2080
2075 PVAL$ = REST$
2076 RETURN
2080 PVAL$ = LEFT$(REST$, AMP - 1)
2090 RETURN

2100 REM URL decode - Input/Output: PVAL$
2110 RESULT$ = ""
2120 FOR I = 1 TO LEN(PVAL$)
2130 CH$ = MID$(PVAL$, I, 1)
2140 IF CH$ = "+" THEN 2145
2144 GOTO 2150
2145 CH$ = " "
2150 RESULT$ = RESULT$ + CH$
2160 NEXT I
2170 PVAL$ = RESULT$
2180 RETURN

3000 REM Build response - Input: STATUS, BODY$, Output: RESP$
3005 RESP$ = ""
3010 IF STATUS = 200 THEN 3015
3014 GOTO 3020
3015 RESP$ = "HTTP/1.0 200 OK" + CRLF$
3016 GOTO 3040
3020 IF STATUS = 302 THEN 3025
3024 GOTO 3030
3025 RESP$ = "HTTP/1.0 302 Found" + CRLF$ + "Location: /" + CRLF$
3026 GOTO 3040
3030 IF STATUS = 404 THEN 3035
3034 GOTO 3040
3035 RESP$ = "HTTP/1.0 404 Not Found" + CRLF$
3040 RESP$ = RESP$ + "Content-Type: text/html" + CRLF$
3050 RESP$ = RESP$ + CRLF$
3060 RESP$ = RESP$ + BODY$
3070 RETURN

5000 REM GET / - Render TODO list
5010 BODY$ = "<html><head><title>TODO</title></head><body>"
5020 BODY$ = BODY$ + "<h1>TODO List</h1>"
5030 BODY$ = BODY$ + "<form action=/add><input name=item size=30>"
5040 BODY$ = BODY$ + "<button>Add</button></form>"
5050 IF TODOCOUNT = 0 THEN 5055
5054 GOTO 5060
5055 BODY$ = BODY$ + "<p>No items yet.</p>"
5056 GOTO 5100
5060 BODY$ = BODY$ + "<ul>"
5070 FOR I = 1 TO TODOCOUNT
5080 BODY$ = BODY$ + "<li>" + TODO$(I) + " <a href=/delete?id=" + STR$(I) + ">[X]</a></li>"
5090 NEXT I
5095 BODY$ = BODY$ + "</ul>"
5100 BODY$ = BODY$ + "</body></html>"
5110 STATUS = 200
5120 GOSUB 3000
5130 RETURN

6000 REM GET /add - Add item
6010 PNAME$ = "item"
6020 GOSUB 2000
6030 GOSUB 2100
6040 IF PVAL$ = "" THEN 6100
6050 IF TODOCOUNT >= 20 THEN 6100
6060 TODOCOUNT = TODOCOUNT + 1
6070 TODO$(TODOCOUNT) = PVAL$
6080 PRINT "Added: "; PVAL$
6100 STATUS = 302
6110 BODY$ = "<a href=/>Back</a>"
6120 GOSUB 3000
6130 RETURN

7000 REM GET /delete - Delete item
7010 PNAME$ = "id"
7020 GOSUB 2000
7030 ID = VAL(PVAL$)
7040 IF ID < 1 THEN 7100
7050 IF ID > TODOCOUNT THEN 7100
7060 PRINT "Deleting item "; ID
7070 FOR I = ID TO TODOCOUNT - 1
7080 TODO$(I) = TODO$(I + 1)
7090 NEXT I
7095 TODOCOUNT = TODOCOUNT - 1
7100 STATUS = 302
7110 BODY$ = "<a href=/>Back</a>"
7120 GOSUB 3000
7130 RETURN

9000 PRINT "Socket failed"
9005 END
9010 PRINT "Listen failed"
9015 END
"#;
    run_headless(program);
}
