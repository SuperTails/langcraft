use langcraft::{Datapack, Interpreter};
use std::path::Path;

pub fn compile_and_run(path: &Path) -> Interpreter {
    let datapack = Datapack::from_bc(path).unwrap();

    let idx = datapack.functions.iter().enumerate().find(|(_, f)| f.id.name == "run").unwrap().0;

    let mut interp = Interpreter::new(datapack, idx, "");

    if let Err(err) = interp.run_to_end() {
        eprintln!("Encountered error: {}", err);
        for f in interp.program() {
            eprintln!("======= {} =======", f.id);
            for c in f.cmds.iter() {
                eprintln!("{}", c);
            }
        }
        eprintln!("Stack:");
        for (f, c) in interp.call_stack() {
            eprintln!("{} line {}", f.id, c);
        }
        panic!();
    }

    interp
}

#[test]
pub fn fibonacci() {
    assert_eq!(
        compile_and_run(Path::new("./tests/fibonacci.bc")).output,
        vec!["0", "1", "1", "2", "3", "5", "8", "13", "21", "34"]
    );
}

#[test]
pub fn dyn_call() {
    assert_eq!(
        compile_and_run(Path::new("./tests/dyn_call.bc")).output,
        vec!["42"],
    )
}