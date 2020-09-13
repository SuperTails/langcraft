use langcraft::{Datapack, Interpreter, BuildOptions};
use langcraft::cir::ScoreHolder;
use std::path::Path;

pub fn compile_and_run(path: &Path) -> Interpreter {
    let datapack = Datapack::from_bc(path, &BuildOptions::default()).unwrap();

    let idx = datapack.functions.iter().enumerate().find(|(_, f)| f.id.name == "run").unwrap().0;

    let mut interp = Interpreter::new(datapack, idx, "");

    if let Err(err) = interp.run_to_end() {
        for f in interp.program() {
            eprintln!("======= {} =======", f.id);
            for c in f.cmds.iter() {
                eprintln!("{}", c);
            }
        }
        eprintln!("==========================");
        eprintln!("Failure in test {:?}", path);
        eprintln!("Encountered error: {}", err);
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

pub fn do_c_test(path: &Path, output: Vec<&str>) {
    let interp = compile_and_run(path);
    assert_eq!(interp.get_rust_score(&ScoreHolder::new("%return%0".to_string()).unwrap()).unwrap(), 0);
    assert_eq!(interp.output, output);
}

// Compiled version of most tests from https://github.com/c-testsuite/c-testsuite
#[test]
pub fn run_c_tests() {
    for file in std::fs::read_dir("./tests/c_testsuite/").unwrap() {
        let file = file.unwrap();
        if file.file_name() == "00089.c.bc" {
            // no. just no.
            continue;
        }
        println!("Doing test {:?}", file.path());
        do_c_test(&file.path(), vec![]);
    }
}

#[test]
pub fn add_overflow() {
    let interp = compile_and_run(Path::new("./tests/add_overflow.bc"));
    assert_eq!(interp.output, vec!["42"]);
}

#[test]
pub fn func_ptr_cast() {
    let interp = compile_and_run(Path::new("./tests/func_ptr_cast.bc"));
    assert_eq!(interp.output, vec!["42", "42"]);
}

#[test]
pub fn func_ptr_direct_cast() {
    let interp = compile_and_run(Path::new("./tests/func_ptr_direct_cast.bc"));
    assert_eq!(interp.output, vec!["1"]);
}

#[test]
pub fn sext_8to64() {
    let interp = compile_and_run(Path::new("./tests/sext_8to64.bc"));
    assert_eq!(interp.output, vec!["127", "0", "-128", "-1"]);
}
