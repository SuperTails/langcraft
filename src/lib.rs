use cir::Function;
use compile_ir::BuildOptions;
pub use interpreter::Interpreter;
use std::path::Path;

pub mod cir;
pub mod compile_ir;
pub mod interpreter;
mod intrinsics;

pub fn compile_bc(path: &Path) -> Result<Vec<Function>, String> {
    Ok(compile_ir::compile_module(
        &llvm_ir::Module::from_bc_path(path)?,
        &BuildOptions { log_trace: false },
    ))
}
