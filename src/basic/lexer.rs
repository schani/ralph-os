//! BASIC tokenizer/lexer

use alloc::string::String;

/// Token types
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    // Keywords
    Print,
    Let,
    If,
    Then,
    Goto,
    For,
    To,
    Step,
    Next,
    Sleep,
    Rem,
    End,
    Run,
    List,
    New,
    Mem,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    LParen,
    RParen,
    Semicolon,
    Comma,

    // Literals and identifiers
    Integer(i64),
    StringLit(String),
    Identifier(String),

    // Structure
    Newline,
    Eof,
}

/// Tokenizer for BASIC source code
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer for the given input
    pub fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0 }
    }

    /// Peek at the current character without consuming it
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    /// Consume and return the current character
    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    /// Skip whitespace (but not newlines)
    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\t' {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Get the next token
    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        match self.peek() {
            None => Token::Eof,
            Some('\n') => {
                self.advance();
                Token::Newline
            }
            Some('\r') => {
                self.advance();
                if self.peek() == Some('\n') {
                    self.advance();
                }
                Token::Newline
            }
            Some('"') => self.read_string(),
            Some(ch) if ch.is_ascii_digit() => self.read_number(),
            Some(ch) if ch.is_ascii_alphabetic() => self.read_identifier_or_keyword(),
            Some('+') => {
                self.advance();
                Token::Plus
            }
            Some('-') => {
                self.advance();
                Token::Minus
            }
            Some('*') => {
                self.advance();
                Token::Star
            }
            Some('/') => {
                self.advance();
                Token::Slash
            }
            Some('(') => {
                self.advance();
                Token::LParen
            }
            Some(')') => {
                self.advance();
                Token::RParen
            }
            Some(';') => {
                self.advance();
                Token::Semicolon
            }
            Some(',') => {
                self.advance();
                Token::Comma
            }
            Some('<') => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Ne
                } else if self.peek() == Some('=') {
                    self.advance();
                    Token::Le
                } else {
                    Token::Lt
                }
            }
            Some('>') => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Ge
                } else {
                    Token::Gt
                }
            }
            Some('=') => {
                self.advance();
                Token::Eq
            }
            _ => {
                // Skip unknown character
                self.advance();
                self.next_token()
            }
        }
    }

    /// Read a string literal
    fn read_string(&mut self) -> Token {
        self.advance(); // consume opening "
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch == '"' {
                self.advance();
                break;
            }
            if ch == '\n' || ch == '\r' {
                break; // Unterminated string
            }
            s.push(ch);
            self.advance();
        }
        Token::StringLit(s)
    }

    /// Read a number
    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        let negative = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let n: i64 = s.parse().unwrap_or(0);
        Token::Integer(if negative { -n } else { n })
    }

    /// Read an identifier or keyword
    fn read_identifier_or_keyword(&mut self) -> Token {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Check for keywords (case-insensitive)
        match s.to_ascii_uppercase().as_str() {
            "PRINT" => Token::Print,
            "LET" => Token::Let,
            "IF" => Token::If,
            "THEN" => Token::Then,
            "GOTO" => Token::Goto,
            "FOR" => Token::For,
            "TO" => Token::To,
            "STEP" => Token::Step,
            "NEXT" => Token::Next,
            "SLEEP" => Token::Sleep,
            "REM" => Token::Rem,
            "END" => Token::End,
            "RUN" => Token::Run,
            "LIST" => Token::List,
            "NEW" => Token::New,
            "MEM" => Token::Mem,
            _ => Token::Identifier(s.to_ascii_uppercase()),
        }
    }

    /// Skip rest of line (for REM comments)
    pub fn skip_to_eol(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            self.advance();
        }
    }
}
