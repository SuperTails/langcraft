#![no_std]
#![feature(rustc_attrs)]
#![no_main]

use arrayvec::ArrayVec;

#[repr(i32)]
#[derive(/*Debug,*/ PartialEq, PartialOrd, Clone, Copy)]
pub enum McBlock {
    Air,
    Cobblestone,
    Granite,
    Andesite,
    Diorite,
    LapisBlock,
    IronBlock,
    GoldBlock,
    DiamondBlock,
    RedstoneBlock,
}

static MC_BLOCKS: [McBlock; 10] = [
    McBlock::Air,
    McBlock::Cobblestone,
    McBlock::Granite,
    McBlock::Andesite,
    McBlock::Diorite,
    McBlock::LapisBlock,
    McBlock::IronBlock,
    McBlock::GoldBlock,
    McBlock::DiamondBlock,
    McBlock::RedstoneBlock,
];

/*impl core::fmt::Display for McBlock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "minecraft:")?;

        match self {
            McBlock::Air => write!(f, "air"),
            McBlock::Cobblestone => write!(f, "cobblestone"),
            McBlock::Granite => write!(f, "granite"),
            McBlock::Andesite => write!(f, "andesite"),
            McBlock::Diorite => write!(f, "diorite"),
            McBlock::LapisBlock => write!(f, "lapis_block"),
            McBlock::IronBlock => write!(f, "iron_block"),
            McBlock::GoldBlock => write!(f, "gold_block"),
            McBlock::DiamondBlock => write!(f, "diamond_block"),
            McBlock::RedstoneBlock => write!(f, "redstone_block"),
        }
    }
}*/

use core::panic::PanicInfo;

extern "C" {
    #[rustc_args_required_const(0, 1)]
    pub fn print_raw(data: *const u8, len: usize);
    pub fn print(value: i32);
    pub fn init();

    pub fn turtle_x(value: i32);
    pub fn turtle_y(value: i32);
    pub fn turtle_z(value: i32);

    /// Sets the block at the turtle's position
    pub fn turtle_set(block: McBlock);

    /// Returns 1 if the block at the turtle's position matches the argument
    pub fn turtle_check(block: McBlock) -> bool;

    /// Returns the block at the turtle's position
    pub fn turtle_get() -> McBlock;

    /// Returns the char at the turtle's position
    pub fn turtle_get_char() -> u8;
}

macro_rules! print_str {
    ($data:expr) => {
        print_raw($data.as_ptr(), $data.len())
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { print_str!(b"Panic") };
    loop {}
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Ident(u32);

const KEY_FN: Ident = Ident(u32::from_be_bytes(*b"\0\0FN"));
const KEY_LET: Ident = Ident(u32::from_be_bytes(*b"\0LET"));
const KEY_WHIL: Ident = Ident(u32::from_be_bytes(*b"WHIL"));

#[derive(Clone, Copy, PartialEq)]
enum Token {
    OpenSquare,
    CloseSquare,
    OpenCurly,
    CloseCurly,
    OpenParen,
    CloseParen,
    Ident(Ident),
}

impl Token {
    pub fn print_self(&self) {
        unsafe { 
            match self {
                Token::OpenSquare => print_str!("["),
                Token::CloseSquare => print_str!("]"),
                Token::OpenCurly => print_str!("{"),
                Token::CloseCurly => print_str!("}"),
                Token::OpenParen => print_str!("("),
                Token::CloseParen => print_str!(")"),
                Token::Ident(Ident(i)) => {
                    print_str!("ident:");
                    print(*i as i32)
                }
            }
        }
    }
}

unsafe fn tokenize() -> ArrayVec<[Token; 16]> {
    turtle_x(-16);
    turtle_y(16);

    let mut char_iter = (0..16)
        .map(|z| { turtle_z(-z); turtle_get_char() })
        .peekable();

    let mut current_token = None;
    let mut tokens = ArrayVec::new();

    loop {
        if let Some(c) = char_iter.next() {
            print_str!("len is: ");
            print(tokens.len() as i32);

            match c {
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
                        *i <<= 8;
                        *i += c as u32;
                    } else {
                        if let Some(t) = current_token.take() {
                            tokens.push(t);
                        }

                        current_token = Some(Token::Ident(Ident(c as u32)));
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

    tokens
}

struct Ast {
    blocks: ArrayVec<[Block; 8]>,
    root: FuncDecl,
}

impl Ast {
    pub fn print_self(&self) {
        self.root.print_self(&self.blocks);
    }
}

#[derive(Clone, PartialEq)]
struct FuncDecl {
    name: Ident,
    body: usize,
}

impl FuncDecl {
    pub fn print_self(&self, blocks: &[Block]) {
        unsafe {
            print_str!("function declaration with name:");
            print(self.name.0 as i32);
            print_str!("and body:");
            for stmt in blocks[self.body].iter() {
                stmt.print_self(blocks);
            }
            print_str!("end function");
        }
    } 
}

type Block = ArrayVec<[Stmt; 4]>;

#[derive(Clone, PartialEq)]
enum Stmt {
    While {
        cond: (),
        body: usize,
    }
}

impl Stmt {
    pub fn print_self(&self, blocks: &[Block]) {
        unsafe {
            match self {
                Stmt::While { cond, body } => {
                    print_str!("while");
                    // TODO: Print cond
                    print_str!("do");
                    for stmt in blocks[*body].iter() {
                        stmt.print_self(blocks);
                    }
                    print_str!("end while");
                }
            }
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    blocks: ArrayVec<[Block; 8]>,
}

#[derive(Clone, Copy, PartialEq)]
enum ParseError {
    Eof,
    UnexpectedToken,
}

impl ParseError {
    pub fn print_self(self) {
        unsafe {
            match self {
                ParseError::Eof => print_str!(b"eof"),
                ParseError::UnexpectedToken => print_str!("unexpected token"),
            }
        }
    }
}

fn parse_ast(tokens: &[Token]) -> Result<Ast, ParseError> {
    let mut parser = Parser { tokens, blocks: ArrayVec::new() };
    let root = parser.parse_func_decl()?;
    Ok(Ast {
        blocks: parser.blocks,
        root,
    })
}

impl Parser<'_> {
    pub fn peek_token(&self) -> Option<Token> {
        self.tokens.get(0).copied()
    }

    pub fn next_token(&mut self) -> Result<Token, ParseError> {
        if let Some((head, tail)) = self.tokens.split_first() {
            self.tokens = tail;
            Ok(*head)
        } else {
            Err(ParseError::Eof)
        }
    }

    pub fn expect_token(&mut self, expected: Token) -> Result<(), ParseError> {
        if self.next_token()? == expected {
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken)
        }
    }

    pub fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        let token = self.next_token()?;
        match token {
            Token::Ident(KEY_WHIL) => {
                // TODO: Parse condition
                let cond = ();

                let body = self.parse_block()?;
                Ok(Stmt::While {
                    cond,
                    body,
                })
            }
            _ => Err(ParseError::UnexpectedToken)
        }
    }

    pub fn parse_block(&mut self) -> Result<usize, ParseError> {
        self.expect_token(Token::OpenCurly)?;

        let mut block = Block::new();
        while self.peek_token() != Some(Token::CloseCurly) {
            block.push(self.parse_stmt()?);
        }

        self.expect_token(Token::CloseCurly)?;

        let result = self.blocks.len();
        self.blocks.push(block);
        Ok(result)
    }

    pub fn parse_func_decl(&mut self) -> Result<FuncDecl, ParseError> {
        if self.next_token()? != Token::Ident(KEY_FN) {
            return Err(ParseError::UnexpectedToken)
        }
        
        let name = match self.next_token()? {
            Token::Ident(i) => i,
            _token => return Err(ParseError::UnexpectedToken),
        };

        self.expect_token(Token::OpenParen)?;

        // TODO: Parse args

        self.expect_token(Token::CloseParen)?;

        // TODO: Parse return type

        let body = self.parse_block()?;

        Ok(FuncDecl { name, body })
    }
}

#[no_mangle]
pub fn main() {
    /*unsafe {
        turtle_x(-16);
        turtle_y(16);
        let mut result = ArrayVec::<[u8; 16]>::new();
        for idx in 0..16 {
            turtle_z(-idx);
            let got = turtle_get_char();
            if got != b' ' {
                result.push(got);
            }
        }

        for r in result.iter() {
            print(*r as i32);
        }
    }*/
    let tokens = unsafe { tokenize() };

    unsafe { print_str!(b"tokens:") };
    for token in tokens.iter() {
        token.print_self();
    }

    match parse_ast(&tokens) {
        Ok(func) => {
            func.print_self();
        }
        Err(err) => {
            unsafe { print_str!(b"encountered error:") };
            err.print_self();
        }
    }
}