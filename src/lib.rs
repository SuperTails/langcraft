use cir::{Function, FunctionId};
use compile_ir::BuildOptions;
pub use interpreter::Interpreter;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;

pub mod cir;
pub mod compile_ir;
pub mod interpreter;
pub mod analysis;
mod intrinsics;

static SETUP_STR: &str = include_str!("setup.mcfunction");
static PUTC_STR: &str = include_str!("stdout/putc.mcfunction");
static FLUSH_STR: &str = include_str!("stdout/flush.mcfunction");

pub struct Datapack {
    pub functions: Vec<Function>,
    pub description: String,
}

impl Datapack {
    pub fn run_index(&self) -> Option<usize> {
        self.functions
            .iter()
            .enumerate()
            .find(|(_, f)| f.id == FunctionId::new("run"))
            .map(|(i, _)| i)
    }

    pub fn from_bc(path: &[PathBuf]) -> Result<Self, String> {
        let mut functions = compile_bc(path)?;

        functions.extend(intrinsics::INTRINSICS.iter().cloned());
        Ok(Datapack {
            functions,
            description: "Compiled datapack from Langcraft".into(),
        })
    }

    /// Creates a datapack with the given root directory, erasing the previous contents of the folder.
    pub fn save(&self, output_folder: &Path) -> Result<(), std::io::Error> {
        if output_folder.exists() {
            eprintln!("Removing previous contents of output directory");
            std::fs::remove_dir_all(output_folder)?;
        }

        std::fs::create_dir(&output_folder)?;

        let mcmeta_contents = json!({
            "pack": {
                "pack_format": 5,
                "description": self.description
            }
        });

        std::fs::write(
            output_folder.join("pack.mcmeta"),
            mcmeta_contents.to_string(),
        )?;

        std::fs::create_dir_all(output_folder.join(Path::new("data/setup/functions/")))?;
        std::fs::write(
            output_folder.join(Path::new("data/setup/functions/setup.mcfunction")),
            SETUP_STR,
        )?;

        std::fs::create_dir_all(output_folder.join(Path::new("data/stdout/functions/")))?;
        std::fs::write(
            output_folder.join(Path::new("data/stdout/functions/putc.mcfunction")),
            PUTC_STR,
        )?;
        std::fs::write(
            output_folder.join(Path::new("data/stdout/functions/flush.mcfunction")),
            FLUSH_STR,
        )?;

        for func in self.functions.iter() {
            let contents = func
                .cmds
                .iter()
                .map(|cmd| cmd.to_string())
                .collect::<Vec<_>>();

            let contents = contents.join("\n");

            let path = func.id.path();
            let path_folders = &path[..path.len() - 1];
            let file_name = &path[path.len() - 1];

            let mut full_path = output_folder
                .join(Path::new("data"))
                .join(Path::new(func.id.namespace()))
                .join(Path::new("functions"));

            for folder in path_folders {
                full_path = full_path.join(Path::new(folder));
            }

            std::fs::create_dir_all(&full_path)?;

            full_path = full_path.join(format!("{}.mcfunction", file_name));

            std::fs::write(full_path, contents.as_bytes())?
        }

        Ok(())
    }
}

pub fn compile_bc(paths: &[PathBuf]) -> Result<Vec<Function>, String> {
    let mut res = Vec::new();
    let mut modules = Vec::new();

    for path in paths {
        modules.push(llvm_ir::Module::from_bc_path(path)?);
    }

    res.extend(compile_ir::compile_module(
        modules,
        &BuildOptions { log_trace: false },
    ));

    Ok(res)
}
