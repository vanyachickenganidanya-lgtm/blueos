//! A tiny allocation-free Lua-style compiler and bytecode VM.
//!
//! This is intentionally a useful embedded subset rather than a claim of Lua 5.x
//! compatibility. It supports `local`, integer/string literals, variables,
//! arithmetic, parentheses, `print(...)`, comments, and `return`.

#[derive(Clone, Copy)]
enum Token {
    Eof,
    Newline,
    Number(i64),
    String(usize, usize),
    Identifier(usize, usize),
    Local,
    Print,
    Return,
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
    Equal,
    Semicolon,
}

struct Lexer<'a> {
    source: &'a [u8],
    cursor: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            cursor: 0,
        }
    }

    fn next(&mut self) -> Result<Token, &'static str> {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
                self.cursor += 1;
            }
            if self.peek() == Some(b'-') && self.peek_n(1) == Some(b'-') {
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.cursor += 1;
                }
                continue;
            }
            break;
        }

        let byte = match self.peek() {
            Some(byte) => byte,
            None => return Ok(Token::Eof),
        };
        self.cursor += 1;
        match byte {
            b'\n' => Ok(Token::Newline),
            b'+' => Ok(Token::Plus),
            b'-' => Ok(Token::Minus),
            b'*' => Ok(Token::Star),
            b'/' => Ok(Token::Slash),
            b'(' => Ok(Token::LeftParen),
            b')' => Ok(Token::RightParen),
            b'=' => Ok(Token::Equal),
            b';' => Ok(Token::Semicolon),
            b'"' | b'\'' => {
                let start = self.cursor;
                while let Some(current) = self.peek() {
                    if current == byte {
                        let length = self.cursor - start;
                        self.cursor += 1;
                        return Ok(Token::String(start, length));
                    }
                    if current == b'\n' {
                        return Err("unterminated string");
                    }
                    self.cursor += 1;
                }
                Err("unterminated string")
            }
            b'0'..=b'9' => {
                let mut number = (byte - b'0') as i64;
                while let Some(next @ b'0'..=b'9') = self.peek() {
                    number = number
                        .checked_mul(10)
                        .and_then(|value| value.checked_add((next - b'0') as i64))
                        .ok_or("integer overflow")?;
                    self.cursor += 1;
                }
                Ok(Token::Number(number))
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = self.cursor - 1;
                while matches!(self.peek(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')) {
                    self.cursor += 1;
                }
                let length = self.cursor - start;
                let word = &self.source[start..start + length];
                Ok(match word {
                    b"local" => Token::Local,
                    b"print" => Token::Print,
                    b"return" => Token::Return,
                    _ => Token::Identifier(start, length),
                })
            }
            _ => Err("unexpected character"),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.cursor).copied()
    }

    fn peek_n(&self, offset: usize) -> Option<u8> {
        self.source.get(self.cursor + offset).copied()
    }
}

#[derive(Clone, Copy)]
enum Op {
    PushInteger(i64),
    PushString(usize, usize),
    Load(u8),
    Store(u8),
    Add,
    Subtract,
    Multiply,
    Divide,
    Negate,
    Print,
    Return,
    Halt,
}

#[derive(Clone, Copy)]
struct Symbol {
    start: usize,
    length: usize,
    slot: u8,
}

pub struct Program<'a> {
    source: &'a str,
    code: [Op; 128],
    length: usize,
}

struct Compiler<'a> {
    source: &'a str,
    lexer: Lexer<'a>,
    current: Token,
    code: [Op; 128],
    length: usize,
    symbols: [Symbol; 16],
    symbol_count: usize,
}

impl<'a> Compiler<'a> {
    fn new(source: &'a str) -> Result<Self, &'static str> {
        let mut lexer = Lexer::new(source);
        let current = lexer.next()?;
        Ok(Self {
            source,
            lexer,
            current,
            code: [Op::Halt; 128],
            length: 0,
            symbols: [Symbol {
                start: 0,
                length: 0,
                slot: 0,
            }; 16],
            symbol_count: 0,
        })
    }

    fn compile(mut self) -> Result<Program<'a>, &'static str> {
        while !matches!(self.current, Token::Eof) {
            if matches!(self.current, Token::Newline | Token::Semicolon) {
                self.advance()?;
                continue;
            }
            self.statement()?;
            while matches!(self.current, Token::Newline | Token::Semicolon) {
                self.advance()?;
            }
        }
        self.emit(Op::Halt)?;
        Ok(Program {
            source: self.source,
            code: self.code,
            length: self.length,
        })
    }

    fn statement(&mut self) -> Result<(), &'static str> {
        match self.current {
            Token::Local => {
                self.advance()?;
                let (start, length) = match self.current {
                    Token::Identifier(start, length) => (start, length),
                    _ => return Err("expected a local variable name"),
                };
                self.advance()?;
                if !matches!(self.current, Token::Equal) {
                    return Err("expected '=' after local name");
                }
                self.advance()?;
                self.expression()?;
                let slot = self.add_symbol(start, length)?;
                self.emit(Op::Store(slot))
            }
            Token::Print => {
                self.advance()?;
                if !matches!(self.current, Token::LeftParen) {
                    return Err("expected '(' after print");
                }
                self.advance()?;
                self.expression()?;
                if !matches!(self.current, Token::RightParen) {
                    return Err("expected ')' after print value");
                }
                self.advance()?;
                self.emit(Op::Print)
            }
            Token::Return => {
                self.advance()?;
                self.expression()?;
                self.emit(Op::Return)
            }
            _ => Err("expected local, print, or return"),
        }
    }

    fn expression(&mut self) -> Result<(), &'static str> {
        self.term()?;
        loop {
            match self.current {
                Token::Plus => {
                    self.advance()?;
                    self.term()?;
                    self.emit(Op::Add)?;
                }
                Token::Minus => {
                    self.advance()?;
                    self.term()?;
                    self.emit(Op::Subtract)?;
                }
                _ => return Ok(()),
            }
        }
    }

    fn term(&mut self) -> Result<(), &'static str> {
        self.unary()?;
        loop {
            match self.current {
                Token::Star => {
                    self.advance()?;
                    self.unary()?;
                    self.emit(Op::Multiply)?;
                }
                Token::Slash => {
                    self.advance()?;
                    self.unary()?;
                    self.emit(Op::Divide)?;
                }
                _ => return Ok(()),
            }
        }
    }

    fn unary(&mut self) -> Result<(), &'static str> {
        if matches!(self.current, Token::Minus) {
            self.advance()?;
            self.unary()?;
            return self.emit(Op::Negate);
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<(), &'static str> {
        match self.current {
            Token::Number(value) => {
                self.emit(Op::PushInteger(value))?;
                self.advance()
            }
            Token::String(start, length) => {
                self.emit(Op::PushString(start, length))?;
                self.advance()
            }
            Token::Identifier(start, length) => {
                let slot = self.find_symbol(start, length).ok_or("unknown variable")?;
                self.emit(Op::Load(slot))?;
                self.advance()
            }
            Token::LeftParen => {
                self.advance()?;
                self.expression()?;
                if !matches!(self.current, Token::RightParen) {
                    return Err("expected ')'");
                }
                self.advance()
            }
            _ => Err("expected an expression"),
        }
    }

    fn add_symbol(&mut self, start: usize, length: usize) -> Result<u8, &'static str> {
        if self.symbol_count >= self.symbols.len() {
            return Err("too many local variables");
        }
        let slot = self.symbol_count as u8;
        self.symbols[self.symbol_count] = Symbol { start, length, slot };
        self.symbol_count += 1;
        Ok(slot)
    }

    fn find_symbol(&self, start: usize, length: usize) -> Option<u8> {
        let name = &self.source.as_bytes()[start..start + length];
        self.symbols[..self.symbol_count]
            .iter()
            .rev()
            .find(|symbol| {
                symbol.length == length
                    && &self.source.as_bytes()[symbol.start..symbol.start + symbol.length] == name
            })
            .map(|symbol| symbol.slot)
    }

    fn emit(&mut self, op: Op) -> Result<(), &'static str> {
        if self.length >= self.code.len() {
            return Err("bytecode program is too large");
        }
        self.code[self.length] = op;
        self.length += 1;
        Ok(())
    }

    fn advance(&mut self) -> Result<(), &'static str> {
        self.current = self.lexer.next()?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Value {
    Integer(i64),
    String(usize, usize),
    Nil,
}

pub enum Output<'a> {
    Integer(i64),
    String(&'a str),
}

pub fn compile(source: &str) -> Result<Program<'_>, &'static str> {
    Compiler::new(source)?.compile()
}

impl<'a> Program<'a> {
    pub fn run(&self, mut output: impl FnMut(Output<'a>)) -> Result<(), &'static str> {
        let mut stack = [Value::Nil; 32];
        let mut stack_length = 0usize;
        let mut locals = [Value::Nil; 16];
        let mut pc = 0usize;

        macro_rules! push {
            ($value:expr) => {{
                if stack_length >= stack.len() {
                    return Err("VM stack overflow");
                }
                stack[stack_length] = $value;
                stack_length += 1;
            }};
        }
        macro_rules! pop {
            () => {{
                if stack_length == 0 {
                    return Err("VM stack underflow");
                }
                stack_length -= 1;
                stack[stack_length]
            }};
        }

        while pc < self.length {
            let op = self.code[pc];
            pc += 1;
            match op {
                Op::PushInteger(value) => push!(Value::Integer(value)),
                Op::PushString(start, length) => push!(Value::String(start, length)),
                Op::Load(slot) => push!(locals[slot as usize]),
                Op::Store(slot) => locals[slot as usize] = pop!(),
                Op::Negate => match pop!() {
                    Value::Integer(value) => push!(Value::Integer(value.wrapping_neg())),
                    _ => return Err("unary minus expects an integer"),
                },
                Op::Add | Op::Subtract | Op::Multiply | Op::Divide => {
                    let right = match pop!() {
                        Value::Integer(value) => value,
                        _ => return Err("arithmetic expects integers"),
                    };
                    let left = match pop!() {
                        Value::Integer(value) => value,
                        _ => return Err("arithmetic expects integers"),
                    };
                    let value = match op {
                        Op::Add => left.wrapping_add(right),
                        Op::Subtract => left.wrapping_sub(right),
                        Op::Multiply => left.wrapping_mul(right),
                        Op::Divide if right != 0 => left / right,
                        Op::Divide => return Err("division by zero"),
                        _ => unreachable!(),
                    };
                    push!(Value::Integer(value));
                }
                Op::Print => match pop!() {
                    Value::Integer(value) => output(Output::Integer(value)),
                    Value::String(start, length) => output(Output::String(
                        &self.source[start..start + length],
                    )),
                    Value::Nil => return Err("cannot print nil"),
                },
                Op::Return => return Ok(()),
                Op::Halt => break,
            }
        }
        Ok(())
    }
}

pub const DEMO: &str = r#"-- compiled inside the kernel
local answer = 6 * 7
local blue = answer + 1
print("Lua bytecode VM online")
print(blue)
return 0
"#;
