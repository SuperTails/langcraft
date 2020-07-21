use langcraft::Interpreter;
use std::path::Path;

#[test]
#[ignore]
fn token_push() {
    let funcs = langcraft::compile_bc(Path::new("./tests/token_push.bc")).unwrap();

    let mut interp = Interpreter::new(funcs, "A");

    interp.run_to_end().unwrap();

    const EXPECTED: &[&str] = &[
        "%stackptr at start of main-blockstart is 56",
        "tokens3:",
        "%stackptr at start of _zn11rust_interp5token10print_self17h32b98aceab2bd1cee-blockstart is 488",
        "ident:",
        "65",
        "%stackptr at start of pop_and_branch is 488",
        "tokens:",
        "%stackptr at start of _zn11rust_interp5token10print_self17h32b98aceab2bd1cee-blockstart is 488",
        "ident:",
        "65",
        "%stackptr at start of pop_and_branch is 488",
        "%stackptr at start of pop_and_branch is 56",
    ];

    println!("==== Output ====");
    for (ex, ac) in EXPECTED.iter().zip(interp.output.iter()) {
        println!("Expected: {}", ex);
        println!("Actual:   {}", ac);
        println!();

        if ex != ac {
            panic!();
        }
    }
}
