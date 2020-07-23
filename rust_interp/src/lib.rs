#![feature(rustc_attrs)]
#![no_std]

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        write!($crate::Stdout, $($arg)*).unwrap()
    };
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        writeln!($crate::Stdout, $($arg)*).unwrap()
    }
}

#[macro_export]
macro_rules! print_str {
    ($data:expr) => {
        $crate::print_raw($data.as_ptr(), $data.len())
    }
}

use arrayvec::ArrayVec;

pub mod lexer;
pub mod parser;

#[derive(Clone, PartialEq, Eq)]
pub struct Ident(ArrayVec::<[u8; 8]>);

impl core::fmt::Debug for Ident {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Ident(")?;
        for c in self.0.iter().copied() {
            write!(f, "{}", char::from(c))?;
        }
        write!(f, ")")
    }
}

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

    pub fn is_key_if(&self) -> bool {
        &self.0[..] == &b"IF"[..]
    }
    
    pub fn is_key_else(&self) -> bool {
        &self.0[..] == &b"ELSE"[..]
    }
}

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

pub struct Stdout;

impl core::fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            unsafe { putc(b) };
        }
        Ok(())
    }
}

impl core::fmt::Display for McBlock {
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
}

use core::panic::PanicInfo;

extern "C" {
    //#[rustc_args_required_const(0, 1)]
    pub fn print_raw(data: *const u8, len: usize);
    pub fn print(value: i32);
    pub fn init();

    pub fn putc(c: u8);

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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { print_str!(b"Panic") };
    loop {}
}