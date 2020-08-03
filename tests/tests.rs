use langcraft::{Datapack, Interpreter};
use langcraft::cir::ScoreHolder;
use std::path::Path;

pub fn compile_and_run(path: &Path) -> Interpreter {
    compile_and_run_multi(std::iter::once(path))
}

pub fn compile_and_run_multi<T, U>(paths: T) -> Interpreter
    where
        T: IntoIterator<Item=U>,
        U: Into<std::path::PathBuf>,
{
    let paths = paths.into_iter().map(|p| p.into()).collect();
    let datapack = Datapack::from_bc(&paths).unwrap();

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
pub fn func_ptr_cast() {
    let interp = compile_and_run(Path::new("./tests/func_ptr_cast.bc"));
    assert_eq!(interp.output, vec!["42", "42"]);
}

#[test]
pub fn static_link() {
    let interp = compile_and_run_multi(vec!["./tests/static_link_p1.bc", "./tests/static_link_p2.bc"]);
    assert_eq!(interp.output, vec!["42"]);
}