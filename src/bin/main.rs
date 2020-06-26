static PROGRAM: &str = "
main() {
    foo += 1;
    if foo < 10 {
        main();
    };
}
";

fn main() {
    let fns = langcraft::compile(PROGRAM).unwrap();
    for decl in fns.iter() {
        println!("Function `{}`", decl.name);
        for r in decl.cmds.iter() {
            println!("{}", r);
        }
        println!();
    }
}
