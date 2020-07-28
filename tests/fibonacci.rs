use langcraft::{Datapack, Interpreter};
use std::path::Path;

#[test]
pub fn fibonacci() {
    let datapack = Datapack::from_bc(Path::new("./tests/fibonacci.bc")).unwrap();

    let idx = datapack.functions.iter().enumerate().find(|(_, f)| f.id.name == "run").unwrap().0;

    let mut interp = Interpreter::new(datapack, idx, "");
    interp.run_to_end().unwrap();

    assert_eq!(interp.output, vec!["0", "1", "1", "2", "3", "5", "8", "13", "21", "34"]);
}