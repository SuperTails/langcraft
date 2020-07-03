use std::path::Path;

fn main() {
    let funcs = langcraft::compile_bc(Path::new("mcfunction.bc")).unwrap();

    for func in funcs.iter() {
        let data = func
            .cmds
            .iter()
            .map(|cmd| cmd.to_string())
            .collect::<Vec<_>>();
        let data = data.join("\n");

        std::fs::write(
            Path::new(&format!("out/{}.mcfunction", func.name)),
            data.as_bytes(),
        )
        .unwrap();
    }

    println!(
        "Generated {} commands",
        funcs.iter().map(|f| f.cmds.len()).sum::<usize>()
    );
}
