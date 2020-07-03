use cir::Function;
use std::path::Path;

pub mod cir;
pub mod compile_ir;

pub fn compile_bc(path: &Path) -> Result<Vec<Function>, String> {
    Ok(compile_ir::compile_module(
        &llvm_ir::Module::from_bc_path(path)?,
        &Default::default(),
    ))
}
