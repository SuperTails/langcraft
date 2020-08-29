#![feature(rustc_attrs)]
#![no_std]

use arrayvec::ArrayVec;
use langcraft_api::{print, print_str};

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

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { print_str!(b"Panic") };
    loop {}
}