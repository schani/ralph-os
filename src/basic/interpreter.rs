//! BASIC interpreter
//!
//! Executes BASIC programs with step-by-step execution for cooperative scheduling.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use super::value::Value;
use super::parser::{Statement, Expr, BinaryOp, ForState, Parser};
use crate::allocator;
use crate::api;

/// Execution status after running a statement
#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionStatus {
    /// Ready to run next statement
    Ready,
    /// Sleeping for the specified milliseconds
    Sleeping(u64),
    /// Program has ended normally
    Finished,
    /// Waiting for input (interactive mode)
    WaitingForInput,
    /// Runtime error occurred
    Error(String),
}

/// BASIC interpreter
pub struct Interpreter {
    /// Program lines (line number -> statement)
    program: BTreeMap<u32, Statement>,
    /// Sorted line numbers for execution order
    line_order: Vec<u32>,
    /// Current line index in line_order (None = not running)
    current_idx: Option<usize>,
    /// Variable storage
    variables: BTreeMap<String, Value>,
    /// FOR loop stack
    for_stack: Vec<ForState>,
    /// GOSUB return stack
    return_stack: Vec<usize>,
    /// Current execution status
    status: ExecutionStatus,
    /// Whether program is running
    running: bool,
}

impl Interpreter {
    /// Create a new interpreter
    pub fn new() -> Self {
        Interpreter {
            program: BTreeMap::new(),
            line_order: Vec::new(),
            current_idx: None,
            variables: BTreeMap::new(),
            for_stack: Vec::new(),
            return_stack: Vec::new(),
            status: ExecutionStatus::Ready,
            running: false,
        }
    }

    /// Load a program from source
    pub fn load_program(&mut self, source: &str) {
        let mut parser = Parser::new(source);
        while let Ok(Some((line_num, stmt))) = parser.parse_line() {
            if let Some(num) = line_num {
                self.program.insert(num, stmt);
            }
        }
        self.rebuild_line_order();
    }

    /// Add or replace a line in the program
    pub fn set_line(&mut self, line_num: u32, stmt: Statement) {
        self.program.insert(line_num, stmt);
        self.rebuild_line_order();
    }

    /// Delete a line from the program
    pub fn delete_line(&mut self, line_num: u32) {
        self.program.remove(&line_num);
        self.rebuild_line_order();
    }

    /// Clear the program
    pub fn clear(&mut self) {
        self.program.clear();
        self.line_order.clear();
        self.variables.clear();
        self.for_stack.clear();
        self.current_idx = None;
        self.running = false;
    }

    fn rebuild_line_order(&mut self) {
        self.line_order = self.program.keys().copied().collect();
    }

    /// Start program execution
    pub fn run(&mut self) {
        if self.line_order.is_empty() {
            self.status = ExecutionStatus::Error("No program".into());
            return;
        }
        self.current_idx = Some(0);
        self.variables.clear();
        self.for_stack.clear();
        self.return_stack.clear();
        self.running = true;
        self.status = ExecutionStatus::Ready;
    }

    /// Check if program is currently running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get number of lines in the program
    pub fn line_count(&self) -> usize {
        self.line_order.len()
    }

    /// Get current execution status
    pub fn status(&self) -> &ExecutionStatus {
        &self.status
    }

    /// Execute one statement (for cooperative scheduling)
    pub fn step(&mut self) -> ExecutionStatus {
        if !self.running {
            return ExecutionStatus::Finished;
        }

        let idx = match self.current_idx {
            Some(i) => i,
            None => {
                self.running = false;
                return ExecutionStatus::Finished;
            }
        };

        if idx >= self.line_order.len() {
            self.running = false;
            self.status = ExecutionStatus::Finished;
            return self.status.clone();
        }

        let line_num = self.line_order[idx];
        let stmt = match self.program.get(&line_num) {
            Some(s) => s,
            None => {
                self.running = false;
                self.status = ExecutionStatus::Error("Line not found".into());
                return self.status.clone();
            }
        };

        // Execute the statement (split borrow: stmt from program, mutable state separate)
        match execute_statement(
            &mut self.variables,
            &mut self.for_stack,
            &mut self.return_stack,
            &self.line_order,
            stmt,
            line_num,
            idx,
        ) {
            Ok(action) => {
                match action {
                    NextAction::Continue => {
                        self.current_idx = Some(idx + 1);
                        if idx + 1 >= self.line_order.len() {
                            self.running = false;
                            self.status = ExecutionStatus::Finished;
                        } else {
                            self.status = ExecutionStatus::Ready;
                        }
                    }
                    NextAction::Jump(target) => {
                        // Find index of target line
                        if let Some(new_idx) = self.line_order.iter().position(|&n| n == target) {
                            self.current_idx = Some(new_idx);
                            self.status = ExecutionStatus::Ready;
                        } else {
                            self.running = false;
                            self.status =
                                ExecutionStatus::Error(alloc::format!("Line {} not found", target));
                        }
                    }
                    NextAction::JumpToIndex(new_idx) => {
                        self.current_idx = Some(new_idx);
                        self.status = ExecutionStatus::Ready;
                    }
                    NextAction::Sleep(ms) => {
                        self.current_idx = Some(idx + 1);
                        self.status = ExecutionStatus::Sleeping(ms);
                    }
                    NextAction::End => {
                        self.running = false;
                        self.status = ExecutionStatus::Finished;
                    }
                }
            }
            Err(e) => {
                self.running = false;
                self.status = ExecutionStatus::Error(e);
            }
        }

        self.status.clone()
    }

    /// List the program
    pub fn list(&self) {
        for &line_num in &self.line_order {
            if let Some(stmt) = self.program.get(&line_num) {
                crate::println!("{} {}", line_num, format_statement(stmt));
            }
        }
    }

    /// Execute an immediate command (for REPL)
    pub fn execute_immediate(&mut self, stmt: &Statement) -> ExecutionStatus {
        match execute_statement(
            &mut self.variables,
            &mut self.for_stack,
            &mut self.return_stack,
            &self.line_order,
            stmt,
            0,
            0,
        ) {
            Ok(NextAction::Continue) | Ok(NextAction::End) => ExecutionStatus::Ready,
            Ok(NextAction::Jump(_)) | Ok(NextAction::JumpToIndex(_)) => {
                ExecutionStatus::Error("Cannot GOTO/GOSUB in immediate mode".into())
            }
            Ok(NextAction::Sleep(ms)) => ExecutionStatus::Sleeping(ms),
            Err(e) => ExecutionStatus::Error(e),
        }
    }
}

/// What to do after executing a statement
enum NextAction {
    Continue,
    Jump(u32),
    JumpToIndex(usize),  // For RETURN - jump to specific index
    Sleep(u64),
    End,
}

/// Execute a BASIC statement
///
/// Takes split borrows to avoid cloning the statement:
/// - variables, for_stack, return_stack are mutable state
/// - line_order is needed for FOR loop body lookup
/// - stmt is borrowed from the program BTreeMap
fn execute_statement(
    variables: &mut BTreeMap<String, Value>,
    for_stack: &mut Vec<ForState>,
    return_stack: &mut Vec<usize>,
    line_order: &[u32],
    stmt: &Statement,
    current_line: u32,
    current_idx: usize,
) -> Result<NextAction, String> {
    match stmt {
        Statement::Print(exprs) => {
            for (i, expr) in exprs.iter().enumerate() {
                let value = eval_expr(variables, expr)?;
                if i > 0 {
                    crate::print!(" ");
                }
                crate::print!("{}", value);
            }
            crate::println!();
            Ok(NextAction::Continue)
        }

        Statement::Let { var, value } => {
            let val = eval_expr(variables, value)?;
            variables.insert(var.clone(), val);
            Ok(NextAction::Continue)
        }

        Statement::If {
            condition,
            then_line,
        } => {
            let cond_val = eval_expr(variables, condition)?;
            if cond_val.is_truthy() {
                Ok(NextAction::Jump(*then_line))
            } else {
                Ok(NextAction::Continue)
            }
        }

        Statement::Goto(target) => Ok(NextAction::Jump(*target)),

        Statement::Gosub(target) => {
            // Push return address (next line index) onto stack
            return_stack.push(current_idx + 1);
            Ok(NextAction::Jump(*target))
        }

        Statement::Return => {
            match return_stack.pop() {
                Some(idx) => Ok(NextAction::JumpToIndex(idx)),
                None => Err("RETURN without GOSUB".into()),
            }
        }

        Statement::For {
            var,
            start,
            end,
            step,
        } => {
            let start_val = eval_expr(variables, start)?
                .as_integer()
                .ok_or("FOR start must be numeric")?;
            let end_val = eval_expr(variables, end)?
                .as_integer()
                .ok_or("FOR end must be numeric")?;
            let step_val = eval_expr(variables, step)?
                .as_integer()
                .ok_or("FOR step must be numeric")?;

            // Set loop variable
            variables.insert(var.clone(), Value::Integer(start_val));

            // Find line after FOR (the body)
            let body_line = line_order
                .iter()
                .find(|&&n| n > current_line)
                .copied()
                .unwrap_or(current_line);

            // Push loop state
            for_stack.push(ForState {
                var: var.clone(),
                end_value: end_val,
                step: step_val,
                body_line,
            });

            Ok(NextAction::Continue)
        }

        Statement::Next(var) => {
            // Find matching FOR
            let loop_idx = for_stack
                .iter()
                .rposition(|f| f.var == *var)
                .ok_or_else(|| alloc::format!("NEXT without FOR: {}", var))?;

            let loop_state = for_stack[loop_idx].clone();
            let current_val = variables
                .get(var)
                .and_then(|v| v.as_integer())
                .ok_or("Loop variable missing")?;

            let next_val = current_val + loop_state.step;

            // Check if loop should continue
            let continue_loop = if loop_state.step > 0 {
                next_val <= loop_state.end_value
            } else {
                next_val >= loop_state.end_value
            };

            if continue_loop {
                // Update variable and jump back to body
                variables.insert(var.clone(), Value::Integer(next_val));
                Ok(NextAction::Jump(loop_state.body_line))
            } else {
                // Loop finished - pop and continue
                for_stack.remove(loop_idx);
                Ok(NextAction::Continue)
            }
        }

        Statement::Sleep(expr) => {
            let val = eval_expr(variables, expr)?;
            let ms = val.as_integer().ok_or("SLEEP requires numeric value")? as u64;
            Ok(NextAction::Sleep(ms))
        }

        Statement::Rem => Ok(NextAction::Continue),

        Statement::End => Ok(NextAction::End),

        Statement::Spawn(name, args) => {
            // Convert Vec<String> to Vec<&str> for the API
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            match api::spawn_program_dynamic(name, &arg_refs) {
                Ok(task_id) => {
                    crate::println!("Spawned '{}' as task {}", name, task_id);
                    Ok(NextAction::Continue)
                }
                Err(e) => Err(alloc::format!("SPAWN failed: {:?}", e)),
            }
        }

        Statement::Dim { name, size } => {
            let size = eval_expr(variables, size)?
                .as_integer()
                .ok_or("DIM size must be numeric")? as usize;
            // Create array based on name suffix ($ = string, otherwise integer)
            if name.ends_with('$') {
                variables.insert(name.clone(), Value::StringArray(vec![String::new(); size + 1]));
            } else {
                variables.insert(name.clone(), Value::IntArray(vec![0; size + 1]));
            }
            Ok(NextAction::Continue)
        }

        Statement::ArrayAssign { name, index, value } => {
            let idx = eval_expr(variables, index)?
                .as_integer()
                .ok_or("Array index must be numeric")? as usize;
            let val = eval_expr(variables, value)?;

            match variables.get_mut(name) {
                Some(Value::StringArray(arr)) => {
                    if idx < arr.len() {
                        arr[idx] = val.as_string().unwrap_or_default();
                    } else {
                        return Err(alloc::format!("Array index {} out of bounds", idx));
                    }
                }
                Some(Value::IntArray(arr)) => {
                    if idx < arr.len() {
                        arr[idx] = val.as_integer().unwrap_or(0);
                    } else {
                        return Err(alloc::format!("Array index {} out of bounds", idx));
                    }
                }
                _ => return Err(alloc::format!("Array {} not found", name)),
            }
            Ok(NextAction::Continue)
        }

        Statement::Send { sock, data } => {
            let sock_val = eval_expr(variables, sock)?
                .as_integer()
                .ok_or("SEND socket must be numeric")? as usize;
            let data_val = eval_expr(variables, data)?
                .as_string()
                .ok_or("SEND data must be string")?;
            crate::net::tcp::send(sock_val, data_val.as_bytes());
            Ok(NextAction::Continue)
        }

        Statement::NetClose(sock) => {
            let sock_val = eval_expr(variables, sock)?
                .as_integer()
                .ok_or("CLOSE socket must be numeric")? as usize;
            crate::net::tcp::close(sock_val);
            Ok(NextAction::Continue)
        }
    }
}

/// Evaluate a BASIC expression
fn eval_expr(variables: &BTreeMap<String, Value>, expr: &Expr) -> Result<Value, String> {
    use crate::net::tcp;

    match expr {
        Expr::Integer(n) => Ok(Value::Integer(*n)),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Variable(name) => variables
            .get(name)
            .cloned()
            .ok_or_else(|| alloc::format!("Undefined variable: {}", name)),
        Expr::Negate(inner) => {
            let val = eval_expr(variables, inner)?;
            match val {
                Value::Integer(n) => Ok(Value::Integer(-n)),
                _ => Err("Cannot negate non-integer".into()),
            }
        }
        Expr::BinaryOp { left, op, right } => {
            let l = eval_expr(variables, left)?;
            let r = eval_expr(variables, right)?;
            eval_binary_op(&l, op, &r)
        }
        Expr::Mem(arg) => {
            let idx = eval_expr(variables, arg)?
                .as_integer()
                .ok_or("MEM requires numeric argument")?;
            let (used, free) = allocator::get_heap_stats();
            match idx {
                0 => Ok(Value::Integer(used as i64)),
                1 => Ok(Value::Integer(free as i64)),
                _ => Err("MEM: invalid argument (use 0 for used, 1 for free)".into()),
            }
        }

        // String functions
        Expr::Chr(arg) => {
            let n = eval_expr(variables, arg)?
                .as_integer()
                .ok_or("CHR$ requires numeric argument")?;
            let ch = (n as u8) as char;
            Ok(Value::String(alloc::format!("{}", ch)))
        }
        Expr::Asc(arg) => {
            let s = eval_expr(variables, arg)?
                .as_string()
                .ok_or("ASC requires string argument")?;
            let n = s.bytes().next().unwrap_or(0) as i64;
            Ok(Value::Integer(n))
        }
        Expr::Len(arg) => {
            let s = eval_expr(variables, arg)?
                .as_string()
                .ok_or("LEN requires string argument")?;
            Ok(Value::Integer(s.len() as i64))
        }
        Expr::Mid(s_expr, start_expr, len_expr) => {
            let s = eval_expr(variables, s_expr)?
                .as_string()
                .ok_or("MID$ requires string argument")?;
            let start = eval_expr(variables, start_expr)?
                .as_integer()
                .ok_or("MID$ start must be numeric")? as usize;
            let len = eval_expr(variables, len_expr)?
                .as_integer()
                .ok_or("MID$ length must be numeric")? as usize;
            // BASIC uses 1-based indexing
            let result: String = s.chars().skip(start.saturating_sub(1)).take(len).collect();
            Ok(Value::String(result))
        }
        Expr::Left(s_expr, n_expr) => {
            let s = eval_expr(variables, s_expr)?
                .as_string()
                .ok_or("LEFT$ requires string argument")?;
            let n = eval_expr(variables, n_expr)?
                .as_integer()
                .ok_or("LEFT$ count must be numeric")? as usize;
            let result: String = s.chars().take(n).collect();
            Ok(Value::String(result))
        }
        Expr::Instr(haystack_expr, needle_expr) => {
            let haystack = eval_expr(variables, haystack_expr)?
                .as_string()
                .ok_or("INSTR requires string arguments")?;
            let needle = eval_expr(variables, needle_expr)?
                .as_string()
                .ok_or("INSTR requires string arguments")?;
            // Return 1-based position, or 0 if not found
            let pos = haystack.find(&needle).map(|p| p + 1).unwrap_or(0);
            Ok(Value::Integer(pos as i64))
        }

        // Array access
        Expr::ArrayAccess { name, index } => {
            let idx = eval_expr(variables, index)?
                .as_integer()
                .ok_or("Array index must be numeric")? as usize;
            match variables.get(name) {
                Some(Value::StringArray(arr)) => {
                    Ok(Value::String(arr.get(idx).cloned().unwrap_or_default()))
                }
                Some(Value::IntArray(arr)) => {
                    Ok(Value::Integer(*arr.get(idx).unwrap_or(&0)))
                }
                _ => Err(alloc::format!("Array {} not found", name)),
            }
        }

        // Network functions
        Expr::Socket => {
            match tcp::socket() {
                Some(h) => Ok(Value::Integer(h as i64)),
                None => Ok(Value::Integer(-1)),
            }
        }
        Expr::Listen(sock_expr, port_expr) => {
            let sock = eval_expr(variables, sock_expr)?
                .as_integer()
                .ok_or("LISTEN socket must be numeric")? as usize;
            let port = eval_expr(variables, port_expr)?
                .as_integer()
                .ok_or("LISTEN port must be numeric")? as u16;
            let ok = tcp::listen(sock, port);
            Ok(Value::Integer(if ok { 1 } else { 0 }))
        }
        Expr::Accept(sock_expr) => {
            let sock = eval_expr(variables, sock_expr)?
                .as_integer()
                .ok_or("ACCEPT socket must be numeric")? as usize;
            match tcp::accept(sock) {
                Some(h) => Ok(Value::Integer(h as i64)),
                None => Ok(Value::Integer(-1)),
            }
        }
        Expr::Recv(sock_expr) => {
            let sock = eval_expr(variables, sock_expr)?
                .as_integer()
                .ok_or("RECV$ socket must be numeric")? as usize;
            let mut buf = [0u8; 1024];
            match tcp::recv(sock, &mut buf) {
                n if n > 0 => {
                    let s = String::from_utf8_lossy(&buf[..n as usize]).into_owned();
                    Ok(Value::String(s))
                }
                _ => Ok(Value::String(String::new())),
            }
        }
        Expr::Sockstate(sock_expr) => {
            let sock = eval_expr(variables, sock_expr)?
                .as_integer()
                .ok_or("SOCKSTATE socket must be numeric")? as usize;
            let code = match tcp::get_state(sock) {
                tcp::TcpState::Closed => 0,
                tcp::TcpState::Listen => 1,
                tcp::TcpState::SynSent => 2,
                tcp::TcpState::SynReceived => 3,
                tcp::TcpState::Established => 4,
                tcp::TcpState::FinWait1 => 5,
                tcp::TcpState::FinWait2 => 6,
                tcp::TcpState::CloseWait => 7,
                tcp::TcpState::Closing => 8,
                tcp::TcpState::LastAck => 9,
                tcp::TcpState::TimeWait => 10,
            };
            Ok(Value::Integer(code))
        }
    }
}

/// Evaluate a binary operation
fn eval_binary_op(l: &Value, op: &BinaryOp, r: &Value) -> Result<Value, String> {
    // Handle string concatenation
    if let (Value::String(ls), BinaryOp::Add, Value::String(rs)) = (l, op, r) {
        let mut result = ls.clone();
        result.push_str(rs);
        return Ok(Value::String(result));
    }

    // Handle string comparison
    if let (Value::String(ls), Value::String(rs)) = (l, r) {
        return match op {
            BinaryOp::Eq => Ok(Value::Integer(if ls == rs { 1 } else { 0 })),
            BinaryOp::Ne => Ok(Value::Integer(if ls != rs { 1 } else { 0 })),
            _ => Err("Invalid string operation".into()),
        };
    }

    // Numeric operations
    let lv = l.as_integer().ok_or("Type error in left operand")?;
    let rv = r.as_integer().ok_or("Type error in right operand")?;

    let result = match op {
        BinaryOp::Add => Value::Integer(lv + rv),
        BinaryOp::Sub => Value::Integer(lv - rv),
        BinaryOp::Mul => Value::Integer(lv * rv),
        BinaryOp::Div => {
            if rv == 0 {
                return Err("Division by zero".into());
            }
            Value::Integer(lv / rv)
        }
        // Comparisons return 1 (true) or 0 (false)
        BinaryOp::Eq => Value::Integer(if lv == rv { 1 } else { 0 }),
        BinaryOp::Ne => Value::Integer(if lv != rv { 1 } else { 0 }),
        BinaryOp::Lt => Value::Integer(if lv < rv { 1 } else { 0 }),
        BinaryOp::Gt => Value::Integer(if lv > rv { 1 } else { 0 }),
        BinaryOp::Le => Value::Integer(if lv <= rv { 1 } else { 0 }),
        BinaryOp::Ge => Value::Integer(if lv >= rv { 1 } else { 0 }),
    };

    Ok(result)
}

/// Format a statement for LIST output
fn format_statement(stmt: &Statement) -> String {
    match stmt {
        Statement::Print(exprs) => {
            let mut s = String::from("PRINT ");
            for (i, expr) in exprs.iter().enumerate() {
                if i > 0 {
                    s.push_str("; ");
                }
                s.push_str(&format_expr(expr));
            }
            s
        }
        Statement::Let { var, value } => {
            alloc::format!("LET {} = {}", var, format_expr(value))
        }
        Statement::If { condition, then_line } => {
            alloc::format!("IF {} THEN {}", format_expr(condition), then_line)
        }
        Statement::Goto(line) => alloc::format!("GOTO {}", line),
        Statement::Gosub(line) => alloc::format!("GOSUB {}", line),
        Statement::Return => String::from("RETURN"),
        Statement::For { var, start, end, step } => {
            alloc::format!("FOR {} = {} TO {} STEP {}", var, format_expr(start), format_expr(end), format_expr(step))
        }
        Statement::Next(var) => alloc::format!("NEXT {}", var),
        Statement::Sleep(expr) => alloc::format!("SLEEP {}", format_expr(expr)),
        Statement::Rem => String::from("REM"),
        Statement::End => String::from("END"),
        Statement::Spawn(name, args) => {
            let mut s = alloc::format!("SPAWN \"{}\"", name);
            for arg in args {
                s.push_str(&alloc::format!(", \"{}\"", arg));
            }
            s
        }
        Statement::Dim { name, size } => {
            alloc::format!("DIM {}({})", name, format_expr(size))
        }
        Statement::ArrayAssign { name, index, value } => {
            alloc::format!("{}({}) = {}", name, format_expr(index), format_expr(value))
        }
        Statement::Send { sock, data } => {
            alloc::format!("SEND {}, {}", format_expr(sock), format_expr(data))
        }
        Statement::NetClose(sock) => {
            alloc::format!("CLOSE {}", format_expr(sock))
        }
    }
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Integer(n) => alloc::format!("{}", n),
        Expr::StringLit(s) => alloc::format!("\"{}\"", s),
        Expr::Variable(name) => name.clone(),
        Expr::BinaryOp { left, op, right } => {
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Eq => "=",
                BinaryOp::Ne => "<>",
                BinaryOp::Lt => "<",
                BinaryOp::Gt => ">",
                BinaryOp::Le => "<=",
                BinaryOp::Ge => ">=",
            };
            alloc::format!("{} {} {}", format_expr(left), op_str, format_expr(right))
        }
        Expr::Negate(inner) => alloc::format!("-{}", format_expr(inner)),
        Expr::Mem(arg) => alloc::format!("MEM({})", format_expr(arg)),
        // String functions
        Expr::Chr(arg) => alloc::format!("CHR$({})", format_expr(arg)),
        Expr::Asc(arg) => alloc::format!("ASC({})", format_expr(arg)),
        Expr::Len(arg) => alloc::format!("LEN({})", format_expr(arg)),
        Expr::Mid(s, start, len) => {
            alloc::format!("MID$({}, {}, {})", format_expr(s), format_expr(start), format_expr(len))
        }
        Expr::Left(s, n) => alloc::format!("LEFT$({}, {})", format_expr(s), format_expr(n)),
        Expr::Instr(h, n) => alloc::format!("INSTR({}, {})", format_expr(h), format_expr(n)),
        // Array access
        Expr::ArrayAccess { name, index } => alloc::format!("{}({})", name, format_expr(index)),
        // Network functions
        Expr::Socket => String::from("SOCKET()"),
        Expr::Listen(sock, port) => {
            alloc::format!("LISTEN({}, {})", format_expr(sock), format_expr(port))
        }
        Expr::Accept(sock) => alloc::format!("ACCEPT({})", format_expr(sock)),
        Expr::Recv(sock) => alloc::format!("RECV$({})", format_expr(sock)),
        Expr::Sockstate(sock) => alloc::format!("SOCKSTATE({})", format_expr(sock)),
    }
}
