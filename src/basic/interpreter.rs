//! BASIC interpreter
//!
//! Executes BASIC programs with step-by-step execution for cooperative scheduling.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use super::value::Value;
use super::parser::{Statement, Expr, BinaryOp, ForState, Parser};
use crate::allocator;

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
            Some(s) => s.clone(),
            None => {
                self.running = false;
                self.status = ExecutionStatus::Error("Line not found".into());
                return self.status.clone();
            }
        };

        // Execute the statement
        match self.execute_statement(&stmt, line_num) {
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

    fn execute_statement(
        &mut self,
        stmt: &Statement,
        current_line: u32,
    ) -> Result<NextAction, String> {
        match stmt {
            Statement::Print(exprs) => {
                for (i, expr) in exprs.iter().enumerate() {
                    let value = self.eval_expr(expr)?;
                    if i > 0 {
                        crate::print!(" ");
                    }
                    crate::print!("{}", value);
                }
                crate::println!();
                Ok(NextAction::Continue)
            }

            Statement::Let { var, value } => {
                let val = self.eval_expr(value)?;
                self.variables.insert(var.clone(), val);
                Ok(NextAction::Continue)
            }

            Statement::If {
                condition,
                then_line,
            } => {
                let cond_val = self.eval_expr(condition)?;
                if cond_val.is_truthy() {
                    Ok(NextAction::Jump(*then_line))
                } else {
                    Ok(NextAction::Continue)
                }
            }

            Statement::Goto(target) => Ok(NextAction::Jump(*target)),

            Statement::For {
                var,
                start,
                end,
                step,
            } => {
                let start_val = self
                    .eval_expr(start)?
                    .as_integer()
                    .ok_or("FOR start must be numeric")?;
                let end_val = self
                    .eval_expr(end)?
                    .as_integer()
                    .ok_or("FOR end must be numeric")?;
                let step_val = self
                    .eval_expr(step)?
                    .as_integer()
                    .ok_or("FOR step must be numeric")?;

                // Set loop variable
                self.variables.insert(var.clone(), Value::Integer(start_val));

                // Find line after FOR (the body)
                let body_line = self
                    .line_order
                    .iter()
                    .find(|&&n| n > current_line)
                    .copied()
                    .unwrap_or(current_line);

                // Push loop state
                self.for_stack.push(ForState {
                    var: var.clone(),
                    end_value: end_val,
                    step: step_val,
                    body_line,
                });

                Ok(NextAction::Continue)
            }

            Statement::Next(var) => {
                // Find matching FOR
                let loop_idx = self
                    .for_stack
                    .iter()
                    .rposition(|f| f.var == *var)
                    .ok_or_else(|| alloc::format!("NEXT without FOR: {}", var))?;

                let loop_state = self.for_stack[loop_idx].clone();
                let current_val = self
                    .variables
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
                    self.variables.insert(var.clone(), Value::Integer(next_val));
                    Ok(NextAction::Jump(loop_state.body_line))
                } else {
                    // Loop finished - pop and continue
                    self.for_stack.remove(loop_idx);
                    Ok(NextAction::Continue)
                }
            }

            Statement::Sleep(expr) => {
                let val = self.eval_expr(expr)?;
                let ms = val.as_integer().ok_or("SLEEP requires numeric value")? as u64;
                Ok(NextAction::Sleep(ms))
            }

            Statement::Rem => Ok(NextAction::Continue),

            Statement::End => Ok(NextAction::End),
        }
    }

    /// Evaluate an expression
    fn eval_expr(&self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Integer(n) => Ok(Value::Integer(*n)),
            Expr::StringLit(s) => Ok(Value::String(s.clone())),
            Expr::Variable(name) => self
                .variables
                .get(name)
                .cloned()
                .ok_or_else(|| alloc::format!("Undefined variable: {}", name)),
            Expr::Negate(inner) => {
                let val = self.eval_expr(inner)?;
                match val {
                    Value::Integer(n) => Ok(Value::Integer(-n)),
                    Value::String(_) => Err("Cannot negate string".into()),
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.eval_binary_op(&l, op, &r)
            }
            Expr::Mem(arg) => {
                let idx = self.eval_expr(arg)?.as_integer().ok_or("MEM requires numeric argument")?;
                let (used, free) = allocator::get_heap_stats();
                match idx {
                    0 => Ok(Value::Integer(used as i64)),
                    1 => Ok(Value::Integer(free as i64)),
                    _ => Err("MEM: invalid argument (use 0 for used, 1 for free)".into()),
                }
            }
        }
    }

    fn eval_binary_op(&self, l: &Value, op: &BinaryOp, r: &Value) -> Result<Value, String> {
        // Handle string concatenation
        if let (Value::String(ls), BinaryOp::Add, Value::String(rs)) = (l, op, r) {
            let mut result = ls.clone();
            result.push_str(rs);
            return Ok(Value::String(result));
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
        match self.execute_statement(stmt, 0) {
            Ok(NextAction::Continue) | Ok(NextAction::End) => ExecutionStatus::Ready,
            Ok(NextAction::Jump(_)) => ExecutionStatus::Error("Cannot GOTO in immediate mode".into()),
            Ok(NextAction::Sleep(ms)) => ExecutionStatus::Sleeping(ms),
            Err(e) => ExecutionStatus::Error(e),
        }
    }
}

/// What to do after executing a statement
enum NextAction {
    Continue,
    Jump(u32),
    Sleep(u64),
    End,
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
        Statement::For { var, start, end, step } => {
            alloc::format!("FOR {} = {} TO {} STEP {}", var, format_expr(start), format_expr(end), format_expr(step))
        }
        Statement::Next(var) => alloc::format!("NEXT {}", var),
        Statement::Sleep(expr) => alloc::format!("SLEEP {}", format_expr(expr)),
        Statement::Rem => String::from("REM"),
        Statement::End => String::from("END"),
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
    }
}
