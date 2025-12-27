# Ralph BASIC Interpreter

A minimal BASIC interpreter for Ralph OS, designed for cooperative multitasking.

## Architecture Overview

The BASIC interpreter is structured in three main layers:

```
+-------------------+
|   Interpreter     |  Executes statements, manages variables and control flow
+-------------------+
         |
+-------------------+
|     Parser        |  Converts tokens into an Abstract Syntax Tree (AST)
+-------------------+
         |
+-------------------+
|     Lexer         |  Tokenizes source code into tokens
+-------------------+
```

## Module Structure

```
src/basic/
├── mod.rs          # Public API: run_headless(), run_repl(), tasks
├── lexer.rs        # Tokenizer - converts source to tokens
├── parser.rs       # Recursive descent parser - tokens to AST
├── interpreter.rs  # Step-based execution engine
└── value.rs        # Value type (Integer, String)
```

## Key Design Decisions

### Step-Based Execution

Unlike traditional interpreters that run to completion, Ralph BASIC executes one statement at a time via `step()`. This enables cooperative multitasking:

```rust
while interp.is_running() {
    let status = interp.step();
    match status {
        ExecutionStatus::Sleeping(ms) => scheduler::sleep_ms(ms),
        ExecutionStatus::Ready => scheduler::yield_now(),
        _ => break,
    }
}
```

### No Floating Point

All numeric values are 64-bit signed integers. This avoids the complexity of software floating-point emulation in a bare-metal environment without FPU support.

### Line-Number Based Program Storage

Programs are stored in a `BTreeMap<u32, Statement>` where line numbers are keys. This provides:
- O(log n) insertion and lookup
- Automatic ordering for execution
- Easy GOTO implementation

## Components

### Lexer (`lexer.rs`)

Converts source text into tokens. Key features:
- Case-insensitive keywords (PRINT, print, Print all work)
- Handles string literals with double quotes
- Produces `Token::Newline` to separate statements
- `skip_to_eol()` for REM comments

Token types:
```rust
enum Token {
    // Keywords
    Print, Let, If, Then, Goto, For, To, Step, Next, Sleep, Rem, End,
    Run, List, New, Mem,
    // Operators
    Plus, Minus, Star, Slash, Eq, Ne, Lt, Gt, Le, Ge,
    LParen, RParen, Semicolon, Comma,
    // Values
    Integer(i64), StringLit(String), Identifier(String),
    // Structure
    Newline, Eof,
}
```

### Parser (`parser.rs`)

Recursive descent parser that builds an AST. Key methods:
- `parse_line()` - Parses a single numbered or immediate line
- `parse_statement()` - Dispatches to statement-specific parsers
- `parse_expression()` - Handles operator precedence

Expression AST:
```rust
enum Expr {
    Integer(i64),
    StringLit(String),
    Variable(String),
    BinaryOp { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    Negate(Box<Expr>),
    Mem(Box<Expr>),  // MEM(0) or MEM(1) for heap stats
}
```

Statement AST:
```rust
enum Statement {
    Print(Vec<Expr>),
    Let { var: String, value: Expr },
    If { condition: Expr, then_line: u32 },
    Goto(u32),
    For { var: String, start: Expr, end: Expr, step: Expr },
    Next(String),
    Sleep(Expr),
    Rem,
    End,
}
```

### Interpreter (`interpreter.rs`)

The execution engine with these key structures:

```rust
struct Interpreter {
    program: BTreeMap<u32, Statement>,   // Line number -> Statement
    line_order: Vec<u32>,                 // Sorted line numbers
    current_idx: Option<usize>,           // Current position in line_order
    variables: BTreeMap<String, Value>,   // Variable storage
    for_stack: Vec<ForState>,             // FOR loop stack
    status: ExecutionStatus,              // Current execution state
    running: bool,                        // Is program running?
}
```

Execution flow:
1. `load_program(source)` - Parses and stores program lines
2. `run()` - Initializes execution state, sets `running = true`
3. `step()` - Executes one statement, returns status
4. Status determines what happens next:
   - `Ready` - Continue to next statement
   - `Sleeping(ms)` - Caller should sleep, then continue
   - `Finished` - Program ended normally
   - `Error(msg)` - Runtime error occurred

### Value (`value.rs`)

Simple enum for runtime values:
```rust
enum Value {
    Integer(i64),
    String(String),
}
```

## Supported Statements

| Statement | Syntax | Description |
|-----------|--------|-------------|
| PRINT | `PRINT expr [; expr]*` | Print expressions to serial |
| LET | `LET var = expr` | Assign value to variable |
| IF | `IF cond THEN linenum` | Conditional jump |
| GOTO | `GOTO linenum` | Unconditional jump |
| FOR | `FOR var = start TO end [STEP n]` | Begin counted loop |
| NEXT | `NEXT var` | End of FOR loop |
| SLEEP | `SLEEP milliseconds` | Pause execution |
| REM | `REM comment text` | Comment (ignored) |
| END | `END` | Terminate program |

## Built-in Functions

| Function | Description |
|----------|-------------|
| `MEM(0)` | Returns bytes of heap memory used |
| `MEM(1)` | Returns bytes of heap memory free |

## Operators

| Operator | Description |
|----------|-------------|
| `+` | Addition / String concatenation |
| `-` | Subtraction |
| `*` | Multiplication |
| `/` | Integer division |
| `=` | Equality comparison |
| `<>` | Not equal |
| `<` | Less than |
| `>` | Greater than |
| `<=` | Less or equal |
| `>=` | Greater or equal |

Comparisons return 1 (true) or 0 (false).

## REPL Commands

| Command | Description |
|---------|-------------|
| `RUN` | Execute the program |
| `LIST` | Display program listing |
| `NEW` | Clear the program |

## Example Programs

### Fibonacci Sequence (pre-loaded in REPL)
```basic
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
```

### Memory Monitor (background task)
```basic
10 REM Memory monitor - runs in background
20 LET U = MEM(0)
30 LET F = MEM(1)
40 PRINT "Heap: "; U; " used, "; F; " free"
50 SLEEP 10000
60 GOTO 20
```

## Integration with Scheduler

The BASIC interpreter integrates with Ralph OS's cooperative scheduler:

1. **Headless mode** (`run_headless`): For background tasks
   - Runs without user interaction
   - SLEEP triggers `scheduler::sleep_ms()`
   - Each step yields to other tasks

2. **Interactive mode** (`run_repl`): For user interaction
   - Reads input from serial port
   - Yields while waiting for input
   - Supports immediate mode commands

Both modes use `scheduler::yield_now()` or `scheduler::sleep_ms()` to cooperatively share CPU time with other tasks.

## Error Handling

Runtime errors are captured in `ExecutionStatus::Error(String)`:
- Division by zero
- Undefined variable
- Type mismatches
- GOTO to non-existent line
- NEXT without matching FOR

The interpreter stops on error and the error message is available via `status()`.
