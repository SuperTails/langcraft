use cir::Function;

pub mod cir;
pub mod compile;
pub mod parse;

pub fn compile(program: &str) -> Result<Vec<Function>, String> {
    Ok(compile::compile_unit(&parse::parse(program)?))
}
