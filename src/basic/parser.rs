//! BASIC parser
//!
//! Parses BASIC statements into executable form.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use super::lexer::{Lexer, Token};

/// A BASIC expression
#[derive(Clone, Debug)]
pub enum Expr {
    /// Integer literal
    Integer(i64),
    /// String literal
    StringLit(String),
    /// Variable reference
    Variable(String),
    /// Binary operation
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    /// Unary negation
    Negate(Box<Expr>),
    /// MEM(n) function call
    Mem(Box<Expr>),
    // String functions
    /// CHR$(n) - character from ASCII code
    Chr(Box<Expr>),
    /// ASC(s$) - ASCII code of first character
    Asc(Box<Expr>),
    /// LEN(s$) - string length
    Len(Box<Expr>),
    /// MID$(s$, start, len) - substring
    Mid(Box<Expr>, Box<Expr>, Box<Expr>),
    /// LEFT$(s$, n) - first n characters
    Left(Box<Expr>, Box<Expr>),
    /// INSTR(haystack$, needle$) - find substring
    Instr(Box<Expr>, Box<Expr>),
    // Network functions
    /// SOCKET() - create socket
    Socket,
    /// LISTEN(sock, port) - listen on port
    Listen(Box<Expr>, Box<Expr>),
    /// ACCEPT(sock) - accept connection
    Accept(Box<Expr>),
    /// RECV$(sock) - receive data
    Recv(Box<Expr>),
    /// SOCKSTATE(sock) - get socket state
    Sockstate(Box<Expr>),
    // Array access
    /// Array element access: ARR(index)
    ArrayAccess { name: String, index: Box<Expr> },
}

/// Binary operators
#[derive(Clone, Debug, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

/// FOR loop state
#[derive(Clone, Debug)]
pub struct ForState {
    pub var: String,
    pub end_value: i64,
    pub step: i64,
    pub body_line: u32,
}

/// A parsed BASIC statement
#[derive(Clone, Debug)]
pub enum Statement {
    /// PRINT expr [; expr]*
    Print(Vec<Expr>),
    /// LET var = expr
    Let { var: String, value: Expr },
    /// IF cond THEN linenum
    If { condition: Expr, then_line: u32 },
    /// GOTO linenum
    Goto(u32),
    /// FOR var = start TO end [STEP step]
    For {
        var: String,
        start: Expr,
        end: Expr,
        step: Expr,
    },
    /// NEXT var
    Next(String),
    /// SLEEP milliseconds
    Sleep(Expr),
    /// REM (comment - no-op)
    Rem,
    /// END
    End,
    /// SPAWN "program_name" [, "arg1", "arg2", ...]
    Spawn(String, Vec<String>),
    /// GOSUB linenum
    Gosub(u32),
    /// RETURN
    Return,
    /// DIM name(size)
    Dim { name: String, size: Expr },
    /// Array assignment: ARR(index) = value
    ArrayAssign { name: String, index: Expr, value: Expr },
    /// SEND sock, data$
    Send { sock: Expr, data: Expr },
    /// CLOSE sock
    NetClose(Expr),
}

/// Parse error
#[derive(Debug)]
pub struct ParseError(pub String);

/// Parser for BASIC
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
}

impl<'a> Parser<'a> {
    /// Create a new parser
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token();
        Parser { lexer, current }
    }

    /// Advance to the next token
    fn advance(&mut self) {
        self.current = self.lexer.next_token();
    }

    /// Parse a single line (may have line number or be immediate)
    /// Returns (optional line number, statement)
    pub fn parse_line(&mut self) -> Result<Option<(Option<u32>, Statement)>, ParseError> {
        // Skip any leading newlines
        while self.current == Token::Newline {
            self.advance();
        }

        if self.current == Token::Eof {
            return Ok(None);
        }

        // Check for line number
        let line_num = if let Token::Integer(n) = &self.current {
            let num = *n as u32;
            self.advance();
            Some(num)
        } else {
            None
        };

        // Parse statement
        let stmt = self.parse_statement()?;

        // Consume newline if present
        if self.current == Token::Newline {
            self.advance();
        }

        Ok(Some((line_num, stmt)))
    }

    /// Parse a statement
    pub fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        match &self.current {
            Token::Print => self.parse_print(),
            Token::Let => self.parse_let(),
            Token::If => self.parse_if(),
            Token::Goto => self.parse_goto(),
            Token::For => self.parse_for(),
            Token::Next => self.parse_next(),
            Token::Sleep => self.parse_sleep(),
            Token::Spawn => self.parse_spawn(),
            Token::Gosub => self.parse_gosub(),
            Token::Return => {
                self.advance();
                Ok(Statement::Return)
            }
            Token::Dim => self.parse_dim(),
            Token::Send => self.parse_send(),
            Token::Close => self.parse_close(),
            Token::Rem => {
                self.advance();
                self.lexer.skip_to_eol();
                // Fetch the next token (newline or EOF) after skipping to EOL
                self.current = self.lexer.next_token();
                Ok(Statement::Rem)
            }
            Token::End => {
                self.advance();
                Ok(Statement::End)
            }
            Token::Identifier(name) => {
                // Could be implicit LET (X = 5) or array assignment (ARR(I) = 5)
                let var = name.clone();
                self.advance();

                // Check for array assignment: ARR(index) = value
                if self.current == Token::LParen {
                    self.advance();
                    let index = self.parse_expression()?;
                    if self.current != Token::RParen {
                        return Err(ParseError("Expected ')' after array index".into()));
                    }
                    self.advance();
                    if self.current != Token::Eq {
                        return Err(ParseError("Expected '=' after array element".into()));
                    }
                    self.advance();
                    let value = self.parse_expression()?;
                    return Ok(Statement::ArrayAssign { name: var, index, value });
                }

                // Simple variable assignment
                if self.current == Token::Eq {
                    self.advance();
                    let value = self.parse_expression()?;
                    Ok(Statement::Let { var, value })
                } else {
                    Err(ParseError("Expected '='".into()))
                }
            }
            _ => Err(ParseError(alloc::format!(
                "Unexpected token: {:?}",
                self.current
            ))),
        }
    }

    fn parse_print(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume PRINT
        let mut exprs = Vec::new();

        loop {
            // Check for end of statement
            if matches!(self.current, Token::Newline | Token::Eof) {
                break;
            }

            let expr = self.parse_expression()?;
            exprs.push(expr);

            // Check for semicolon (more items) or end
            if self.current == Token::Semicolon {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Statement::Print(exprs))
    }

    fn parse_let(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume LET

        let var = match &self.current {
            Token::Identifier(name) => name.clone(),
            _ => return Err(ParseError("Expected variable name".into())),
        };
        self.advance();

        if self.current != Token::Eq {
            return Err(ParseError("Expected '='".into()));
        }
        self.advance();

        let value = self.parse_expression()?;
        Ok(Statement::Let { var, value })
    }

    fn parse_if(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume IF

        let condition = self.parse_expression()?;

        if self.current != Token::Then {
            return Err(ParseError("Expected THEN".into()));
        }
        self.advance();

        let then_line = match &self.current {
            Token::Integer(n) => *n as u32,
            _ => return Err(ParseError("Expected line number after THEN".into())),
        };
        self.advance();

        Ok(Statement::If {
            condition,
            then_line,
        })
    }

    fn parse_goto(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume GOTO

        let line = match &self.current {
            Token::Integer(n) => *n as u32,
            _ => return Err(ParseError("Expected line number".into())),
        };
        self.advance();

        Ok(Statement::Goto(line))
    }

    fn parse_for(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume FOR

        let var = match &self.current {
            Token::Identifier(name) => name.clone(),
            _ => return Err(ParseError("Expected variable name".into())),
        };
        self.advance();

        if self.current != Token::Eq {
            return Err(ParseError("Expected '='".into()));
        }
        self.advance();

        let start = self.parse_expression()?;

        if self.current != Token::To {
            return Err(ParseError("Expected TO".into()));
        }
        self.advance();

        let end = self.parse_expression()?;

        // Optional STEP
        let step = if self.current == Token::Step {
            self.advance();
            self.parse_expression()?
        } else {
            Expr::Integer(1)
        };

        Ok(Statement::For {
            var,
            start,
            end,
            step,
        })
    }

    fn parse_next(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume NEXT

        let var = match &self.current {
            Token::Identifier(name) => {
                let v = name.clone();
                self.advance();
                v
            }
            _ => return Err(ParseError("Expected variable name".into())),
        };

        Ok(Statement::Next(var))
    }

    fn parse_sleep(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume SLEEP
        let ms = self.parse_expression()?;
        Ok(Statement::Sleep(ms))
    }

    fn parse_spawn(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume SPAWN

        // Expect a string literal for the program name
        let name = match &self.current {
            Token::StringLit(s) => s.clone(),
            _ => return Err(ParseError("SPAWN requires a program name string".into())),
        };
        self.advance();

        // Parse optional comma-separated string arguments
        let mut args = Vec::new();
        while self.current == Token::Comma {
            self.advance(); // consume comma

            let arg = match &self.current {
                Token::StringLit(s) => s.clone(),
                _ => return Err(ParseError("SPAWN arguments must be strings".into())),
            };
            self.advance();
            args.push(arg);
        }

        Ok(Statement::Spawn(name, args))
    }

    fn parse_gosub(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume GOSUB

        let line = match &self.current {
            Token::Integer(n) => *n as u32,
            _ => return Err(ParseError("Expected line number after GOSUB".into())),
        };
        self.advance();

        Ok(Statement::Gosub(line))
    }

    fn parse_dim(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume DIM

        let name = match &self.current {
            Token::Identifier(n) => n.clone(),
            _ => return Err(ParseError("Expected array name after DIM".into())),
        };
        self.advance();

        if self.current != Token::LParen {
            return Err(ParseError("Expected '(' after array name".into()));
        }
        self.advance();

        let size = self.parse_expression()?;

        if self.current != Token::RParen {
            return Err(ParseError("Expected ')' after array size".into()));
        }
        self.advance();

        Ok(Statement::Dim { name, size })
    }

    fn parse_send(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume SEND

        let sock = self.parse_expression()?;

        if self.current != Token::Comma {
            return Err(ParseError("Expected ',' after socket in SEND".into()));
        }
        self.advance();

        let data = self.parse_expression()?;

        Ok(Statement::Send { sock, data })
    }

    fn parse_close(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume CLOSE
        let sock = self.parse_expression()?;
        Ok(Statement::NetClose(sock))
    }

    /// Parse expression with operator precedence
    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;

        loop {
            let op = match &self.current {
                Token::Eq => BinaryOp::Eq,
                Token::Ne => BinaryOp::Ne,
                Token::Lt => BinaryOp::Lt,
                Token::Gt => BinaryOp::Gt,
                Token::Le => BinaryOp::Le,
                Token::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;

        loop {
            let op = match &self.current {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;

        loop {
            let op = match &self.current {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.current == Token::Minus {
            self.advance();
            let expr = self.parse_primary()?;
            return Ok(Expr::Negate(Box::new(expr)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match &self.current {
            Token::Integer(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Integer(n))
            }
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::StringLit(s))
            }
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                // Check for array access: name(index)
                if self.current == Token::LParen {
                    self.advance();
                    let index = self.parse_expression()?;
                    if self.current != Token::RParen {
                        return Err(ParseError("Expected ')' after array index".into()));
                    }
                    self.advance();
                    return Ok(Expr::ArrayAccess { name, index: Box::new(index) });
                }
                Ok(Expr::Variable(name))
            }
            Token::Mem => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after MEM".into()));
                }
                self.advance();
                let arg = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')'".into()));
                }
                self.advance();
                Ok(Expr::Mem(Box::new(arg)))
            }
            // String functions
            Token::Chr => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after CHR$".into()));
                }
                self.advance();
                let arg = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')'".into()));
                }
                self.advance();
                Ok(Expr::Chr(Box::new(arg)))
            }
            Token::Asc => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after ASC".into()));
                }
                self.advance();
                let arg = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')'".into()));
                }
                self.advance();
                Ok(Expr::Asc(Box::new(arg)))
            }
            Token::Len => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after LEN".into()));
                }
                self.advance();
                let arg = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')'".into()));
                }
                self.advance();
                Ok(Expr::Len(Box::new(arg)))
            }
            Token::Mid => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after MID$".into()));
                }
                self.advance();
                let s = self.parse_expression()?;
                if self.current != Token::Comma {
                    return Err(ParseError("Expected ',' in MID$".into()));
                }
                self.advance();
                let start = self.parse_expression()?;
                if self.current != Token::Comma {
                    return Err(ParseError("Expected ',' in MID$".into()));
                }
                self.advance();
                let len = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after MID$".into()));
                }
                self.advance();
                Ok(Expr::Mid(Box::new(s), Box::new(start), Box::new(len)))
            }
            Token::Left => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after LEFT$".into()));
                }
                self.advance();
                let s = self.parse_expression()?;
                if self.current != Token::Comma {
                    return Err(ParseError("Expected ',' in LEFT$".into()));
                }
                self.advance();
                let n = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after LEFT$".into()));
                }
                self.advance();
                Ok(Expr::Left(Box::new(s), Box::new(n)))
            }
            Token::Instr => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after INSTR".into()));
                }
                self.advance();
                let haystack = self.parse_expression()?;
                if self.current != Token::Comma {
                    return Err(ParseError("Expected ',' in INSTR".into()));
                }
                self.advance();
                let needle = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after INSTR".into()));
                }
                self.advance();
                Ok(Expr::Instr(Box::new(haystack), Box::new(needle)))
            }
            // Network functions
            Token::Socket => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after SOCKET".into()));
                }
                self.advance();
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after SOCKET".into()));
                }
                self.advance();
                Ok(Expr::Socket)
            }
            Token::NetListen => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after LISTEN".into()));
                }
                self.advance();
                let sock = self.parse_expression()?;
                if self.current != Token::Comma {
                    return Err(ParseError("Expected ',' in LISTEN".into()));
                }
                self.advance();
                let port = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after LISTEN".into()));
                }
                self.advance();
                Ok(Expr::Listen(Box::new(sock), Box::new(port)))
            }
            Token::Accept => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after ACCEPT".into()));
                }
                self.advance();
                let sock = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after ACCEPT".into()));
                }
                self.advance();
                Ok(Expr::Accept(Box::new(sock)))
            }
            Token::Recv => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after RECV$".into()));
                }
                self.advance();
                let sock = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after RECV$".into()));
                }
                self.advance();
                Ok(Expr::Recv(Box::new(sock)))
            }
            Token::Sockstate => {
                self.advance();
                if self.current != Token::LParen {
                    return Err(ParseError("Expected '(' after SOCKSTATE".into()));
                }
                self.advance();
                let sock = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')' after SOCKSTATE".into()));
                }
                self.advance();
                Ok(Expr::Sockstate(Box::new(sock)))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expression()?;
                if self.current != Token::RParen {
                    return Err(ParseError("Expected ')'".into()));
                }
                self.advance();
                Ok(expr)
            }
            _ => Err(ParseError(alloc::format!(
                "Expected value, got {:?}",
                self.current
            ))),
        }
    }

    /// Check if current token is end of input
    pub fn is_eof(&self) -> bool {
        self.current == Token::Eof
    }

    /// Get current token (for checking commands like RUN, LIST)
    pub fn current_token(&self) -> &Token {
        &self.current
    }
}
