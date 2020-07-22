#![no_std]
#![no_main]

use rust_interp::{print, Ident, print_str};
use rust_interp::lexer::tokenize;
use rust_interp::parser::*;

use arrayvec::ArrayVec;

pub struct Interpreter {
    vars: ArrayVec<[(Ident, i32); 2]>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            vars: ArrayVec::new(),
        }
    }

    pub fn run(&mut self, ast: &Ast) {
        for stmt in ast.root.body.iter().copied() {
            self.run_stmt(&ast.stmts[stmt], &ast.stmts, &ast.exprs);
        }
    }

    pub fn eval_expr(&self, expr: &Expr, exprs: &[Expr]) -> i32 {
        let getter = |ident: &Ident| self.vars.iter().find(|v| &v.0 == ident).unwrap().1;
        expr.eval(exprs, &getter)
    }

    pub fn eval_cond(&self, cond: &Cond, exprs: &[Expr]) -> bool {
        let getter = |ident: &Ident| self.vars.iter().find(|v| &v.0 == ident).unwrap().1;
        cond.eval(exprs, &getter)
    }

    pub fn run_stmt(&mut self, stmt: &Stmt, stmts: &[Stmt], exprs: &[Expr]) {
        match stmt {
            Stmt::Let { ident, value } => {
                if let Some((_, v)) = self.vars.iter_mut().find(|v| &v.0 == ident) {
                    *v = *value;
                } else {
                    self.vars.push((ident.clone(), *value));
                };
            }
            Stmt::Print { arg } => {
                unsafe {
                    print_str!("printing value:");
                    print(self.eval_expr(arg, exprs));
                }
            }
            Stmt::If { cond, true_body, false_body } => {
                if self.eval_cond(cond, exprs) {
                    for s in true_body.iter().copied() {
                        self.run_stmt(&stmts[s], stmts, exprs);
                    }
                } else {
                    for s in false_body.iter().copied() {
                        self.run_stmt(&stmts[s], stmts, exprs);
                    }
                }
            }
            Stmt::Loop { .. } => {
                unsafe { print_str!("TODO: LOOP") };
            }
            Stmt::While { cond, body } => {
                while self.eval_cond(cond, exprs) {
                    for s in body.iter().copied() {
                        self.run_stmt(&stmts[s], stmts, exprs);
                    }
                }
            }
            Stmt::Assign { lhs, rhs } => {
                let value = self.eval_expr(rhs, exprs);
                self.vars.iter_mut().find(|v| &v.0 == lhs).unwrap().1 = value;
            }
        }
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

            let mut interp = Interpreter::new();
            interp.run(&func);
        }
        Err(err) => {
            unsafe { print_str!(b"encountered error:") };
            err.print_self();

            if let rust_interp::parser::ParseError::UnexpectedToken(idx) = err {
                unsafe { print_str!("token:") };
                tokens[idx].print_self();
            }
        }
    }
}