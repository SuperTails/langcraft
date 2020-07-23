use crate::{print, turtle_x, turtle_y, turtle_z, turtle_get_char, Ident};
use arrayvec::ArrayVec;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    OpenSquare,
    CloseSquare,
    OpenCurly,
    CloseCurly,
    OpenParen,
    CloseParen,
    Equals,
    EqualsEquals,
    LessThan,
    Modulo,
    Plus,
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
                Token::EqualsEquals => print_str!("=="),
                Token::Modulo => print_str!("%"),
                Token::Plus => print_str!("+"),
                Token::LessThan => print_str!("<"),
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

pub unsafe fn tokenize() -> ArrayVec<[Token; 100]> {
    turtle_x(-16);

    let mut char_iter = (0..256)
        .map(|idx| {
            let z = -(idx % 16);
            turtle_z(z);
            turtle_y(32 - 2 * (idx / 16));
            turtle_get_char()
        })
        .peekable();

    let mut current_token = None;
    let mut tokens = ArrayVec::<[Token; 100]>::new();

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
                b'=' if char_iter.peek() == Some(&b'=') => {
                    char_iter.next();

                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::EqualsEquals);
                }
                b'=' => {
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::Equals);
                }
                b'<' => {
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::LessThan);
                }
                b'%' => {
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::Modulo);
                }
                b'+' => {
                    if let Some(c) = current_token.take() {
                        tokens.push(c);
                    }

                    tokens.push(Token::Plus);
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

    print_str!(b"tokens2:");
    for token in tokens.iter() {
        token.print_self();
    }

    tokens
}

