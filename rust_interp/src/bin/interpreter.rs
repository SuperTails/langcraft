#![no_std]
#![no_main]

use rust_interp::{print, Ident, print_str};
use rust_interp::lexer::tokenize;
use rust_interp::parser::*;

use arrayvec::ArrayVec;

fn interpret(ast: &Ast) {
    let mut vars = ArrayVec::<[(Ident, i32); 2]>::new();
    for stmt in ast.root.body.iter().copied() {
        let stmt = &ast.stmts[stmt];
        match stmt {
            Stmt::Let { ident, value } => {
                if let Some((_, v)) = vars.iter_mut().find(|v| &v.0 == ident) {
                    *v = *value;
                } else {
                    vars.push((ident.clone(), *value));
                };
            }
            Stmt::Print { ident } => {
                if let Some((_, v)) = vars.iter().find(|v| &v.0 == ident) {
                    unsafe {
                        print_str!("printing value:");
                        print(*v);
                    }
                } else {
                    unsafe { print_str!("attempt to print undefined variable") };
                    return;
                }
            }
            Stmt::If { .. } => {
                unsafe { print_str!("TODO: IF") };
            }
            Stmt::Loop { .. } => {
                unsafe { print_str!("TODO: LOOP") };
            }
            Stmt::While { .. } => {
                unsafe { print_str!("TODO: WHILE") };
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

            interpret(&func);
        }
        Err(err) => {
            unsafe { print_str!(b"encountered error:") };
            err.print_self();
        }
    }
}