use std::path::Path;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    assert_eq!(args.len(), 2);

    let funcs = langcraft::compile_bc(Path::new(&args[1])).unwrap();

    for file in std::fs::read_dir("out/").unwrap() {
        let file = file.unwrap();
        std::fs::remove_file(file.path()).unwrap();
    }

    for func in funcs.iter() {
        let data = func
            .cmds
            .iter()
            .map(|cmd| cmd.to_string())
            .collect::<Vec<_>>();
        let data = data.join("\n");

        std::fs::write(
            Path::new(&format!("out/{}.mcfunction", func.id)),
            data.as_bytes(),
        )
        .unwrap();
    }

    println!(
        "Generated {} commands",
        funcs.iter().map(|f| f.cmds.len()).sum::<usize>()
    );
}
