#![no_std]
#![no_main]

use rust_interp::*;

use arrayvec::ArrayVec;

#[derive(Clone, PartialEq, Eq)]
struct Ident(ArrayVec::<[u8; 8]>);

impl Ident {
    pub fn new() -> Self {
        Ident(ArrayVec::new())
    }

    pub fn print_self(&self) {
        let mut word = [0, 0, 0, 0];
        for (idx, byte) in word.iter_mut().enumerate() {
            if let Some(b) = self.0.get(idx).copied() {
                *byte = b;
            }
        }
        unsafe { print(self.0.len() as i32) };
        unsafe { print(i32::from_ne_bytes(word)) };
    }

    pub fn is_key_print(&self) -> bool {
        &self.0[..] == &b"PRINT"[..]
    }

    pub fn is_key_fn(&self) -> bool {
        &self.0[..] == &b"FN"[..]
    }

    pub fn is_key_let(&self) -> bool {
        &self.0[..] == &b"LET"[..]
    }

    pub fn is_key_while(&self) -> bool {
        &self.0[..] == &b"WHILE"[..]
    }

    pub fn is_key_loop(&self) -> bool {
        &self.0[..] == &b"LOOP"[..]
    }
}

#[derive(Clone, PartialEq)]
enum Token {
    OpenSquare,
    CloseSquare,
    OpenCurly,
    CloseCurly,
    OpenParen,
    CloseParen,
    Equals,
    Ident(Ident),
    Literal(i32),
}

impl Token {
    pub fn is_key_fn(&self) -> bool {
        if let Token::Ident(i) = self {
            i.is_key_fn()
        } else {
            false
        }
    }

    pub fn print_self(&self) {
        unsafe { 
            match self {
                Token::OpenSquare => print_str!("["),
                Token::CloseSquare => print_str!("]"),
                Token::OpenCurly => print_str!("{"),
                Token::CloseCurly => print_str!("}"),
                Token::OpenParen => print_str!("("),
                Token::CloseParen => print_str!(")"),
                Token::Equals => print_str!("="),
                Token::Ident(i) => {
                    print_str!("ident:");
                    i.print_self();
                }
                Token::Literal(l) => {
                    print_str!("literal:");
                    print(*l);
                }
            }
        }
    }
}

unsafe fn tokenize() -> ArrayVec<[Token; 50]> {
    turtle_x(-16);
    turtle_y(16);

    let mut char_iter = (0..50)
        .map(|z| { turtle_z(-z); turtle_get_char() })
        .peekable();

    let mut current_token = None;
    let mut tokens = ArrayVec::<[Token; 50]>::new();

    loop {
        if let Some(c) = char_iter.next() {
            print_str!("len is: ");
            print(tokens.len() as i32);

            match c {
                b'0'..=b'9' => {
                    let val = (c - b'0') as i32;

                    if let Some(Token::Literal(l)) = current_token.as_mut() {
                        *l *= 10;
                        *l += val;
                    } else {
                        if let Some(c) = current_token.take() {
                            tokens.push(c);
                        }

                        current_token = Some(Token::Literal(val));
                    }
                }
                b'=' => {
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::Equals);
                }
                b'(' => {
                    print_str!(b"Open parenthesis token");
                    
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::OpenParen);
                }
                b')' => {
                    print_str!(b"Close parenthesis token");
                    
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::CloseParen);
                }
                b'{' => {
                    print_str!(b"Open curly token");
                    
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::OpenCurly);
                }
                b'}' => {
                    print_str!(b"Close curly token");
                    
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::CloseCurly);
                }
                b'[' => {
                    print_str!(b"Open square token");

                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::OpenSquare);
                }
                b']' => {
                    print_str!("Close square token");

                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::CloseSquare);
                }
                b'_' |
                b'A'..=b'Z' => {
                    print_str!("Part of an ident");

                    if let Some(Token::Ident(Ident(i))) = current_token.as_mut() {
                        i.push(c);
                    } else {
                        if let Some(t) = current_token.take() {
                            tokens.push(t);
                        }

                        let mut ident = Ident::new();
                        ident.0.push(c);
                        current_token = Some(Token::Ident(ident));
                    }
                }
                _ => {
                    if let Some(t) = current_token.take() {
                        tokens.push(t);
                    }

                    print_str!(b"other token");
                    print(c as i32);
                }
            }
        } else {
            break;
        }
    }

    unsafe { print_str!(b"tokens2:") };
    for token in tokens.iter() {
        token.print_self();
    }

    tokens
}

fn interpret(ast: &Ast) {
    let mut vars = ArrayVec::<[(Ident, i32); 2]>::new();
    for stmt in ast.root.body.iter().copied() {
        let stmt = &ast.stmts[stmt];
        match stmt {
            Stmt::Let { ident, value } => {
                /*let idx = if let Some((i, _)) = vars.iter().enumerate().find(|(i, v)| &v.0 == ident) {
                    i
                } else {
                    let idx = vars.len();
                    vars.push((ident.clone(), *value));
                    idx
                };*/
            }
            Stmt::Print { ident } => {
                /*if let Some((i, v)) = vars.iter().enumerate().find(|(i, v)| &v.0 == ident) {
                    print_str!("printing value:");
                    print(v.1);
                } else {
                    print_str!("attempt to print undefined variable");
                    return;
                }*/
            }
            Stmt::Loop { .. } => {
                print_str!("TODO: LOOP");
            }
            Stmt::While { .. } => {
                print_str!("TODO: WHILE");
            }
        }
    }
}

struct Ast {
    stmts: ArrayVec<[Stmt; 8]>,
    root: FuncDecl,
}

impl Ast {
    pub fn print_self(&self) {
        unsafe {
            print_str!("number of stmts:");
            print(self.stmts.len() as i32);
            print_str!("root:");
        }
        self.root.print_self(&self.stmts);
    }
}

#[derive(Clone, PartialEq)]
struct FuncDecl {
    name: Ident,
    body: Block,
}

impl FuncDecl {
    pub fn print_self(&self, stmts: &[Stmt]) {
        unsafe {
            print_str!("function declaration with name:");
            self.name.print_self();
            print_str!("and body (with X statements):");
            print(self.body.len() as i32);
            for stmt in self.body.iter().copied() {
                stmts[stmt].print_self(stmts);
            }
            print_str!("end function");
        }
    } 
}

type Block = ArrayVec<[usize; 4]>;

#[derive(Clone, PartialEq)]
enum Stmt {
    While {
        cond: (),
        body: Block,
    },
    Loop {
        body: Block,
    },
    Print {
        ident: Ident,
    },
    Let {
        ident: Ident,
        value: i32,
    }
}

impl Stmt {
    pub fn print_self(&self, stmts: &[Stmt]) {
        unsafe {
            match self {
                Stmt::While { cond, body } => {
                    print_str!("while");
                    // TODO: Print cond
                    print_str!("do");
                    for stmt in body.iter().copied() {
                        stmts[stmt].print_self(stmts);
                    }
                    print_str!("end while");
                }
                Stmt::Loop { body } => {
                    print_str!("loop");
                    for stmt in body.iter().copied() {
                        stmts[stmt].print_self(stmts);
                    }
                    print_str!("end loop");
                }
                Stmt::Print { ident } => {
                    print_str!("print");
                    ident.print_self();
                    print_str!("end print");
                }
                Stmt::Let { ident, value } => {
                    print_str!("let");
                    ident.print_self();
                    print_str!("=");
                    print(*value);
                    print_str!("end let");
                }
            }
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    current_idx: usize,
    stmts: ArrayVec<[Stmt; 8]>,
}

#[derive(Clone, PartialEq)]
enum ParseError {
    Eof,
    UnexpectedToken(usize),
}

impl ParseError {
    pub fn print_self(&self) {
        unsafe {
            match &self {
                ParseError::Eof => print_str!(b"eof"),
                ParseError::UnexpectedToken(t) => {
                    print_str!("unexpected token at index:");
                    print(*t as i32);
                }
            }
        }
    }
}

fn parse_ast(tokens: &[Token]) -> Result<Ast, ParseError> {
    let mut parser = Parser { tokens, current_idx: 0, stmts: ArrayVec::new() };
    let root = parser.parse_func_decl()?;
    unsafe { print_str!("parser stmt count:");
    print(parser.stmts.len() as i32); };
    unsafe { root.print_self(&parser.stmts) };

    let result = Ast { stmts: parser.stmts, root };
    unsafe { result.print_self() };
    Ok(result)
}

impl Parser<'_> {
    pub fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(0)
    }

    pub fn next_token(&mut self) -> Result<&Token, ParseError> {
        self.current_idx += 1;

        if let Some((head, tail)) = self.tokens.split_first() {
            self.tokens = tail;
            Ok(head)
        } else {
            Err(ParseError::Eof)
        }
    }

    pub fn expect_token(&mut self, expected: &Token) -> Result<(), ParseError> {
        let next = self.next_token()?;

        if next == expected {
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken(self.current_idx - 1))
        }
    }

    pub fn parse_stmt(&mut self) -> Result<usize, ParseError> {
        let idx = self.stmts.len();
        let token = self.next_token()?;
        let stmt = match token {
            Token::Ident(i) if i.is_key_while() => {
                // TODO: Parse condition
                let cond = ();

                let body = self.parse_block()?;
                Stmt::While {
                    cond,
                    body,
                }
            }
            Token::Ident(i) if i.is_key_loop() => {
                let body = self.parse_block()?;
                Stmt::Loop {
                    body,
                }
            }
            Token::Ident(i) if i.is_key_print() => {
                self.expect_token(&Token::OpenParen)?;
                let ident = match self.next_token()? {
                    Token::Ident(i) => i.clone(),
                    _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
                };
                self.expect_token(&Token::CloseParen)?;
                Stmt::Print { 
                    ident
                }
            }
            Token::Ident(i) if i.is_key_let() => {
                let ident = match self.next_token()? {
                    Token::Ident(i) => i.clone(),
                    _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
                };

                self.expect_token(&Token::Equals)?;

                let value = match self.next_token()? {
                    Token::Literal(l) => *l,
                    _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
                };

                Stmt::Let {
                    ident,
                    value,
                }
            }
            _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
        };

        self.stmts.push(stmt);
        Ok(idx)
    }

    pub fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect_token(&Token::OpenCurly)?;

        let mut block = Block::new();
        while self.peek_token() != Some(&Token::CloseCurly) {
            unsafe { print_str!("adding stmt") };
            block.push(self.parse_stmt()?);
        }

        self.expect_token(&Token::CloseCurly)?;

        unsafe {
            print_str!("block has this many statements");
            let i = block.len() as i32;
            assert_ne!(i, 255);
            print(i as i32);
        }

        Ok(block)
    }

    pub fn parse_func_decl(&mut self) -> Result<FuncDecl, ParseError> {
        if !self.next_token()?.is_key_fn() {
            return Err(ParseError::UnexpectedToken(self.current_idx - 1))
        }

        let name = match self.next_token()? {
            Token::Ident(i) => i.clone(),
            tok => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
        };

        self.expect_token(&Token::OpenParen)?;

        // TODO: Parse args

        self.expect_token(&Token::CloseParen)?;

        // TODO: Parse return type

        let body = self.parse_block()?;

        Ok(FuncDecl { name, body })
    }
}

#[no_mangle]
pub fn main() {
    /*unsafe {
        print_str!(b"Size and align of an ident:");
        print(core::mem::size_of::<Ident>() as i32);
        print(core::mem::align_of::<Ident>() as i32);
        print_str!("Size of a token:");
        print(core::mem::size_of::<Token>() as i32);
    }

    unsafe {
        turtle_x(-16);
        turtle_y(16);
        let mut result = ArrayVec::<[Token; 16]>::new();
        for idx in 0..16 {
            turtle_z(-idx);
            let got = turtle_get_char();
            if got != b' ' {
                let mut av = ArrayVec::new();
                av.push(got);
                result.push(Token::Ident(Ident(av)));
            }
        }

        for r in result.iter() {
            r.print_self();
        }
    }*/

    let tokens = unsafe { tokenize() };

    unsafe { print_str!(b"tokens:") };
    for token in tokens.iter() {
        token.print_self();
    }

    match parse_ast(&tokens) {
        Ok(func) => {
            unsafe { print_str!("Successfully parsed AST") };

            func.print_self();

            unsafe { print_str!("Interpreting") };

            interpret(&func);
        }
        Err(err) => {
            unsafe { print_str!(b"encountered error:") };
            err.print_self();
        }
    }
}