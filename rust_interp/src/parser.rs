use crate::lexer::Token;
use crate::{print, Ident};
use arrayvec::ArrayVec;

pub struct Ast {
    pub stmts: ArrayVec<[Stmt; 8]>,
    pub root: FuncDecl,
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
pub struct FuncDecl {
    pub name: Ident,
    pub body: Block,
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

pub type Block = ArrayVec<[usize; 4]>;

#[derive(Clone, PartialEq)]
pub enum Expr {
    Literal(i32),
    Ident(Ident),
    Modulo(usize, usize),
}

#[derive(Clone, PartialEq)]
pub enum Stmt {
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
    },
    If {
        cond: (),
        true_body: Block,
        false_body: Block,
    },
}

impl Stmt {
    pub fn print_self(&self, stmts: &[Stmt]) {
        unsafe {
            match self {
                Stmt::While { cond: _, body } => {
                    print_str!("while");
                    print_str!("TODO: print cond");
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
                Stmt::If { cond: _, true_body, false_body } => {
                    print_str!("if");
                    print_str!("TODO: cond");
                    print_str!("then");
                    for stmt in true_body.iter().copied() {
                        stmts[stmt].print_self(stmts);
                    }
                    print_str!("else");
                    for stmt in false_body.iter().copied() {
                        stmts[stmt].print_self(stmts);
                    }
                    print_str!("end if");
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
pub enum ParseError {
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

pub fn parse_ast(tokens: &[Token]) -> Result<Ast, ParseError> {
    let mut parser = Parser { tokens, current_idx: 0, stmts: ArrayVec::new() };
    let root = parser.parse_func_decl()?;
    unsafe {
        print_str!("parser stmt count:");
        print(parser.stmts.len() as i32);
        root.print_self(&parser.stmts);
    };

    let result = Ast { stmts: parser.stmts, root };
    result.print_self();
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
            Token::Ident(i) if i.is_key_if() => {
                // TODO: Parse cond
                let cond = ();

                let true_body = self.parse_block()?;

                let false_body = match self.peek_token() {
                    Some(Token::Ident(i)) if i.is_key_else() => {
                        self.next_token()?;

                        self.parse_block()?
                    }
                    _ => Block::new(),
                };

                Stmt::If {
                    cond,
                    true_body,
                    false_body,
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
            _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
        };

        self.expect_token(&Token::OpenParen)?;

        // TODO: Parse args

        self.expect_token(&Token::CloseParen)?;

        // TODO: Parse return type

        let body = self.parse_block()?;

        Ok(FuncDecl { name, body })
    }
}

