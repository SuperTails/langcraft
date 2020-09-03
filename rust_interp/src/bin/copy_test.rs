#![no_std]
#![no_main]

use rust_interp::*;

use arrayvec::ArrayVec;

enum Stmt {
    Loop {
        body: usize
    }
}

impl Stmt {
    pub fn print_self(&self, blocks: &[Block]) {
        unsafe {
            match self {
                Stmt::Loop { body } => {
                    print_str!("loop");
                    for stmt in blocks[*body].iter() {
                        stmt.print_self(blocks);
                    }
                    print_str!("end loop");
                }
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Ident(ArrayVec::<[u8; 4]>);

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
}


type Block = ArrayVec<[Stmt; 2]>;

pub type Token = i32;

#[derive(Clone, PartialEq)]
struct FuncDecl {
    body: usize,
}

impl FuncDecl {
    pub fn print_self(&self, blocks: &[Block]) {
        unsafe {
            print_str!("function declaration with name:");
            print_str!("and body:");
            for stmt in blocks[self.body].iter() {
                stmt.print_self(blocks);
            }
            print_str!("end function");
        }
    } 
}

struct Parser {
    blocks: ArrayVec<[Block; 2]>,
}

struct Ast {
    blocks: ArrayVec<[Block; 2]>,
    root: FuncDecl,
}

fn parse_ast() {
    let mut parser = Parser { blocks: ArrayVec::new() };
    {
        let mut block = ArrayVec::new();
        block.push(Stmt::Loop { body: 1 });
        parser.blocks.push(block);
        parser.blocks.push(ArrayVec::new());
    }

    let root = FuncDecl {
        body: 0,
    };

    unsafe {
        print_str!("parser block count:");
        print(parser.blocks.len() as i32);
        root.print_self(&parser.blocks);
    };

    let result = Ast { blocks: parser.blocks, root };
    assert_eq!(result.blocks.len(), 2);
}

#[no_mangle]
pub fn main() {
    parse_ast();
}