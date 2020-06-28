use crate::cir::{McRange, Relation};
use std::convert::TryFrom;

#[derive(Debug, PartialEq, PartialOrd, Hash, Clone)]
pub enum Token {
    Ident(Ident),
    Literal(i32),
    BinaryOp(BinaryOp),
    GreaterThanEq,
    LessThanEq,
    Semicolon,
    If,
    OpenCurly,
    CloseCurly,
    OpenParen,
    CloseParen,
    And,
}

pub fn tokenize(s: &str) -> Result<Vec<Token>, String> {
    let mut current = None;
    let mut result = Vec::new();

    let flush = |result: &mut Vec<_>, current: &mut Option<Token>| {
        if let Some(current) = current.take() {
            if let Token::Ident(Ident(id)) = &current {
                if id == "if" {
                    result.push(Token::If);
                    return;
                }
            }

            result.push(current);
        }
    };

    let add_binary_op = |result: &mut Vec<_>, current: &mut Option<Token>, op: BinaryOp| {
        flush(result, current);
        result.push(Token::BinaryOp(op));
    };

    let mut iter = s.chars().peekable();
    while let Some(c) = iter.next() {
        match (c, iter.peek()) {
            ('+', Some('=')) => {
                add_binary_op(&mut result, &mut current, BinaryOp::AddAssign);
                iter.next();
            }
            ('-', Some('=')) => {
                add_binary_op(&mut result, &mut current, BinaryOp::SubAssign);
                iter.next();
            }
            ('*', Some('=')) => {
                add_binary_op(&mut result, &mut current, BinaryOp::MulAssign);
                iter.next();
            }
            ('/', Some('=')) => {
                add_binary_op(&mut result, &mut current, BinaryOp::DivAssign);
                iter.next();
            }
            ('%', Some('%')) => {
                add_binary_op(&mut result, &mut current, BinaryOp::ModAssign);
                iter.next();
            }
            ('>', Some('<')) => {
                add_binary_op(&mut result, &mut current, BinaryOp::Swap);
                iter.next();
            }
            ('>', Some('=')) => {
                flush(&mut result, &mut current);
                result.push(Token::GreaterThanEq);
                iter.next();
            }
            ('<', Some('=')) => {
                flush(&mut result, &mut current);
                result.push(Token::LessThanEq);
                iter.next();
            }
            ('&', Some('&')) => {
                flush(&mut result, &mut current);
                result.push(Token::And);
                iter.next();
            }
            ('>', _) => add_binary_op(&mut result, &mut current, BinaryOp::Max),
            ('<', _) => add_binary_op(&mut result, &mut current, BinaryOp::Min),
            ('=', _) => add_binary_op(&mut result, &mut current, BinaryOp::Assign),
            (';', _) => {
                flush(&mut result, &mut current);
                result.push(Token::Semicolon)
            }
            ('{', _) => {
                flush(&mut result, &mut current);
                result.push(Token::OpenCurly)
            }
            ('}', _) => {
                flush(&mut result, &mut current);
                result.push(Token::CloseCurly)
            }
            ('(', _) => {
                flush(&mut result, &mut current);
                result.push(Token::OpenParen)
            }
            (')', _) => {
                flush(&mut result, &mut current);
                result.push(Token::CloseParen)
            }
            (c, _) if c.is_alphabetic() || c == '_' => {
                if let Some(Token::Ident(ident)) = &mut current {
                    ident.0.push(c);
                } else {
                    flush(&mut result, &mut current);
                    current = Some(Token::Ident(Ident(c.to_string())));
                }
            }
            (c, _) if c.is_numeric() => {
                let digit = c.to_digit(10).unwrap() as i32;
                if let Some(Token::Literal(literal)) = &mut current {
                    *literal *= 10;
                    *literal += digit;
                } else {
                    flush(&mut result, &mut current);
                    current = Some(Token::Literal(digit));
                }
            }
            (c, _) if c.is_whitespace() => flush(&mut result, &mut current),
            (c, _) => return Err(format!("Unexpected character `{}`", c)),
        }
    }

    Ok(result)
}

pub fn parse(s: &str) -> Result<Unit, String> {
    let tokens = tokenize(s)?;
    let mut parser = Parser { tokens: &tokens };
    match parser.parse_unit() {
        Ok(result) => Ok(result),
        Err(err) => Err(err + &format!(" parser state: {:?}", parser.tokens)),
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
}

impl Parser<'_> {
    pub fn next_token(&mut self) -> Option<&Token> {
        if let Some((token, tail)) = self.tokens.split_first() {
            self.tokens = tail;
            Some(token)
        } else {
            None
        }
    }

    pub fn next_token_result(&mut self) -> Result<&Token, String> {
        self.next_token()
            .ok_or_else(|| "unexpected end of tokens".to_string())
    }

    pub fn next_ident(&mut self) -> Result<&Ident, String> {
        match self.next_token_result()? {
            Token::Ident(ident) => Ok(ident),
            token => Err(format!("expected ident, found {:?}", token)),
        }
    }

    pub fn parse_unit(&mut self) -> Result<Unit, String> {
        let mut decls = Vec::new();
        while !self.tokens.is_empty() {
            decls.push(self.parse_function()?);
        }
        Ok(Unit { decls })
    }

    pub fn parse_function(&mut self) -> Result<Function, String> {
        let name = self.next_ident()?.clone();

        match self.next_token_result()? {
            Token::OpenParen => {}
            token => return Err(format!("Expected `(`, got {:?}", token)),
        }

        match self.next_token_result()? {
            Token::CloseParen => {}
            token => return Err(format!("Expected `)`, got {:?}", token)),
        }

        match self.next_token_result()? {
            Token::OpenCurly => {}
            token => return Err(format!("Expected `{{`, got {:?}", token)),
        }

        let body = self.parse_block()?;

        match self.next_token_result()? {
            Token::CloseCurly => {}
            token => return Err(format!("Expected `}}`, got {:?}", token)),
        }

        Ok(Function { name, body })
    }

    pub fn parse_block(&mut self) -> Result<Block, String> {
        let mut stmts = Vec::new();

        loop {
            if let None | Some(Token::CloseCurly) = self.tokens.get(0) {
                break;
            }

            stmts.push(self.parse_stmt()?);

            if self.next_token() != Some(&Token::Semicolon) {
                return Err("expected semicolon".to_string());
            }
        }

        Ok(Block { stmts })
    }

    pub fn parse_conditions(&mut self) -> Result<Vec<Condition>, String> {
        let mut conds = Vec::new();

        loop {
            // TODO: Inverted conditions??
            // TODO: Matches??
            let lhs = self.parse_expr()?;
            let relation = Relation::try_from(self.next_token_result()?.clone())
                .map_err(|_| "expected relation")?;
            let rhs = self.parse_expr()?;

            conds.push(Condition {
                inverted: false,
                lhs,
                kind: ConditionKind::Relation { relation, rhs },
            });

            if self.tokens[0] != Token::And {
                break;
            }

            self.next_token();
        }

        Ok(conds)
    }

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        let token = self.next_token_result()?;

        if let Token::Literal(l) = token {
            Ok(Expr::Literal(*l))
        } else if let Token::Ident(i) = token {
            Ok(Expr::Ident(i.clone()))
        } else {
            Err(format!("invalid token {:?} for expr", token))
        }
    }

    pub fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let token = self.next_token_result()?;
        if token == &Token::If {
            let conds = self.parse_conditions()?;

            if self.next_token() != Some(&Token::OpenCurly) {
                return Err("expected `{`".to_string());
            }

            let body = Box::new(self.parse_block()?);

            if self.next_token() != Some(&Token::CloseCurly) {
                return Err("expected `}`".to_string());
            }

            Ok(Stmt::If(IfStmt { body, conds }))
        } else {
            let lhs = if let Token::Ident(ident) = token {
                ident.clone()
            } else {
                return Err(format!("expected ident or if, found {:?}", token));
            };

            let token = self.next_token_result()?;
            if token == &Token::OpenParen {
                if self.next_token_result()? != &Token::CloseParen {
                    return Err("expected `)`".to_string());
                }

                Ok(Stmt::FuncCall(FuncCall { name: lhs }))
            } else {
                let op = if let Token::BinaryOp(op) = token {
                    *op
                } else {
                    return Err("expected binary op for second token".to_string());
                };

                let rhs = self.parse_expr()?;

                Ok(Stmt::Binary(BinaryStmt { lhs, op, rhs }))
            }
        }
    }
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub struct Unit {
    pub decls: Vec<Function>,
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub struct Function {
    pub name: Ident,
    // pub params: Vec<Expr>,
    pub body: Block,
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub enum Stmt {
    If(IfStmt),
    Binary(BinaryStmt),
    FuncCall(FuncCall),
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub struct FuncCall {
    pub name: Ident,
    // pub params: Vec<Expr>,
}

#[derive(Debug, PartialEq, PartialOrd, Hash, Clone)]
pub enum Expr {
    Ident(Ident),
    Literal(i32),
}

#[derive(Debug, PartialEq, PartialOrd, Hash, Clone)]
pub struct Ident(pub String);

#[derive(Debug, PartialEq, PartialOrd, Hash, Clone)]
pub struct BinaryStmt {
    pub lhs: Ident,
    pub rhs: Expr,
    pub op: BinaryOp,
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub struct Condition {
    pub inverted: bool,
    pub lhs: Expr,
    pub kind: ConditionKind,
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub enum ConditionKind {
    Relation { relation: Relation, rhs: Expr },
    Matches(McRange),
}

#[derive(Debug, PartialEq, Hash, Clone)]
pub struct IfStmt {
    pub conds: Vec<Condition>,
    pub body: Box<Block>,
}

type BinaryOp = crate::cir::ScoreOpKind;
