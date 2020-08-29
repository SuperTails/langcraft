use crate::lexer::Token;
use crate::{print, Ident};
use arrayvec::ArrayVec;
use langcraft_api::print_str;

pub struct Ast {
    pub exprs: ArrayVec<[Expr; 8]>,
    pub stmts: ArrayVec<[Stmt; 16]>,
    pub root: FuncDecl,
}

impl Ast {
    pub fn print_self(&self) {
        unsafe {
            print_str!("number of stmts:");
            print(self.stmts.len() as i32);
            print_str!("root:");
        }
        self.root.print_self(&self.stmts, &self.exprs);
    }
}

#[derive(Clone, PartialEq)]
pub struct FuncDecl {
    pub name: Ident,
    pub body: Block,
}

impl FuncDecl {
    pub fn print_self(&self, stmts: &[Stmt], exprs: &[Expr]) {
        unsafe {
            print_str!("function declaration with name:");
            self.name.print_self();
            print_str!("and body (with X statements):");
            print(self.body.len() as i32);
            for stmt in self.body.iter().copied() {
                stmts[stmt].print_self(stmts, exprs);
            }
            print_str!("end function");
        }
    } 
}

pub type Block = ArrayVec<[usize; 4]>;

#[derive(Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Modulo
}

#[derive(Clone, PartialEq)]
pub enum Expr {
    Literal(i32),
    Ident(Ident),
    BinOp(usize, BinOp, usize),
}

impl Expr {
    pub fn print_self(&self, exprs: &[Expr]) {
        unsafe {
            match self {
                Expr::Literal(l) => print(*l),
                Expr::Ident(i) => i.print_self(),
                Expr::BinOp(lhs, op, rhs) => {
                    print_str!("expr:");
                    exprs[*lhs].print_self(exprs);
                    match op {
                        BinOp::Add => print_str!("plus"),
                        BinOp::Modulo => print_str!("mod"),
                    }
                    exprs[*rhs].print_self(exprs);
                    print_str!("end expr");
                }
            }
        }
    }

    pub fn eval<F: Fn(&Ident) -> i32>(&self, exprs: &[Expr], f: &F) -> i32 {
        match self {
            Expr::Literal(l) => *l,
            Expr::Ident(i) => f(i),
            Expr::BinOp(l, op, r) => {
                let l = exprs[*l].eval(exprs, f);
                let r = exprs[*r].eval(exprs, f);
                match op {
                    BinOp::Add => l + r,
                    BinOp::Modulo => l % r,
                }
            }
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum Relation {
    Equal,
    LessThan,
}

#[derive(Clone, PartialEq)]
pub struct Cond {
    lhs: Expr,
    rel: Relation,
    rhs: Expr,
}

impl Cond {
    pub fn print_self(&self, exprs: &[Expr]) {
        unsafe {
            print_str!("condition:");
            self.lhs.print_self(exprs);
            match &self.rel {
                Relation::Equal => print_str!("=="),
                Relation::LessThan => print_str!("<"),
            }
            self.rhs.print_self(exprs);
            print_str!("end condition");
        }
    }

    pub fn eval<F: Fn(&Ident) -> i32>(&self, exprs: &[Expr], f: &F) -> bool {
        let l = self.lhs.eval(exprs, f);
        let r = self.rhs.eval(exprs, f);
        match self.rel {
            Relation::Equal => l == r,
            Relation::LessThan => l < r,
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum Stmt {
    While {
        cond: Cond,
        body: Block,
    },
    Loop {
        body: Block,
    },
    Print {
        arg: Expr,
    },
    Let {
        ident: Ident,
        value: i32,
    },
    If {
        cond: Cond,
        true_body: Block,
        false_body: Block,
    },
    Assign {
        lhs: Ident,
        rhs: Expr,
    }
}

impl Stmt {
    pub fn print_self(&self, stmts: &[Stmt], exprs: &[Expr]) {
        unsafe {
            match self {
                Stmt::While { cond, body } => {
                    print_str!("while");
                    cond.print_self(exprs);
                    print_str!("do");
                    for stmt in body.iter().copied() {
                        stmts[stmt].print_self(stmts, exprs);
                    }
                    print_str!("end while");
                }
                Stmt::Loop { body } => {
                    print_str!("loop");
                    for stmt in body.iter().copied() {
                        stmts[stmt].print_self(stmts, exprs);
                    }
                    print_str!("end loop");
                }
                Stmt::Print { arg } => {
                    print_str!("print");
                    arg.print_self(exprs);
                    print_str!("end print");
                }
                Stmt::Let { ident, value } => {
                    print_str!("let");
                    ident.print_self();
                    print_str!("=");
                    print(*value);
                    print_str!("end let");
                }
                Stmt::If { cond, true_body, false_body } => {
                    print_str!("if");
                    cond.print_self(exprs);
                    print_str!("then");
                    for stmt in true_body.iter().copied() {
                        stmts[stmt].print_self(stmts, exprs);
                    }
                    print_str!("else");
                    for stmt in false_body.iter().copied() {
                        stmts[stmt].print_self(stmts, exprs);
                    }
                    print_str!("end if");
                }
                Stmt::Assign { lhs, rhs } => {
                    print_str!("assign");
                    lhs.print_self();
                    print_str!("=");
                    rhs.print_self(exprs);
                    print_str!("end assign");
                }
            }
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    current_idx: usize,
    stmts: ArrayVec<[Stmt; 16]>,
    exprs: ArrayVec<[Expr; 8]>,
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
    let mut parser = Parser { tokens, current_idx: 0, stmts: ArrayVec::new(), exprs: ArrayVec::new() };
    let root = parser.parse_func_decl()?;
    unsafe {
        print_str!("parser stmt count:");
        print(parser.stmts.len() as i32);
        root.print_self(&parser.stmts, &parser.exprs);
    };

    let result = Ast { stmts: parser.stmts, exprs: parser.exprs, root };
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

    pub fn add_expr(&mut self, ex: Expr) -> usize {
        let result = self.exprs.len();
        self.exprs.push(ex);
        result
    }

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let lhs = match self.next_token()? {
            Token::Literal(l) => Expr::Literal(*l),
            Token::Ident(i) => Expr::Ident(i.clone()),
            _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
        };

        let op = match self.peek_token() {
            Some(&Token::Modulo) => Some(BinOp::Modulo),
            Some(&Token::Plus) => Some(BinOp::Add),
            _ => None,
        };

        if let Some(op) = op {
            self.next_token()?;

            let rhs = match self.next_token()? {
                Token::Literal(l) => Expr::Literal(*l),
                Token::Ident(i) => Expr::Ident(i.clone()),
                _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
            };

            let lhs = self.add_expr(lhs);
            let rhs = self.add_expr(rhs);

            Ok(Expr::BinOp(lhs, op, rhs))
        } else {
            Ok(lhs)
        }
    }

    pub fn parse_cond(&mut self) -> Result<Cond, ParseError> {
        let lhs = self.parse_expr()?;
        let rel = match self.next_token()? {
            Token::EqualsEquals => Relation::Equal,
            Token::LessThan => Relation::LessThan,
            _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
        };
        let rhs = self.parse_expr()?;
        Ok(Cond { lhs, rel, rhs })
    }

    pub fn parse_stmt(&mut self) -> Result<usize, ParseError> {
        let token = self.next_token()?;
        let stmt = match token {
            Token::Ident(i) if i.is_key_while() => {
                let cond = self.parse_cond()?;

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
                let arg = self.parse_expr()?;
                self.expect_token(&Token::CloseParen)?;
                Stmt::Print { 
                    arg
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
                unsafe { print_str!("parsing if statement") };

                let cond = self.parse_cond()?;

                cond.print_self(&self.exprs);

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
            Token::Ident(i) => {
                let lhs = i.clone();

                self.expect_token(&Token::Equals)?;

                let rhs = self.parse_expr()?;

                Stmt::Assign {
                    lhs,
                    rhs,
                }
            }
            _ => return Err(ParseError::UnexpectedToken(self.current_idx - 1)),
        };

        let idx = self.stmts.len();
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

