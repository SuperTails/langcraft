use std::path::Path;
use langcraft::Interpreter;

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

    let mut interp = Interpreter::new(funcs);
    
    let mut hit_breakpoint = false;

    /*let output = std::fs::read_to_string("latest.log")
        .unwrap()
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            l.contains("Render thread") && l.contains("[CHAT]") && !l.contains('@') && !l.contains("Executed") /* TODO: Show this in the interpreter output */
        })
        .map(|(i, l)| {
            let idx = l.find("[CHAT] ").unwrap() + "[CHAT] ".len();
            (i, l[idx..].to_owned())
        })
        .collect::<Vec<(usize, String)>>();
    
    println!("Output:");
    for out in output.iter() {
        println!("[{:>3}] {}", out.0, out.1);
    }
    println!();*/

    let stdin = std::io::stdin();

    while !interp.halted() {
        if hit_breakpoint {
            let mut input = String::new();
            stdin.read_line(&mut input).unwrap();
            input = input.trim().to_string();

            if input == "" || input == "STEP" {
                println!("{}", interp.next_command().unwrap());
                interp.step();
            } else if input == "CONT" {
                println!("Continuing");
                hit_breakpoint = false;
            } else if input.starts_with("REG") {
                let words = input.split_whitespace().collect::<Vec<_>>();
                match &words[..] {
                    ["REG", reg] => {
                        let holder = langcraft::cir::ScoreHolder::new(reg.to_string()).unwrap();
                        if let Some(score) = interp.rust_scores.get(&holder) {
                            println!("Holder {} has score {}", holder, score);
                        } else{ 
                            println!("Holder {} has no score", holder);
                        }
                    }
                    _ => eprintln!("Too many arguments to `REG`"),
                }
            } else if input.starts_with("MEM") {
                eprintln!("TODO: `MEM`");
            } else {
                eprintln!("Invalid input {:?}", input);
            }
        } else {
            /*if interp.next_command().map(|c| c.to_string()) == Some("execute at @e[tag=ptr] store result score %41%0 rust run data get block ~ ~ ~ RecordItem.tag.Memory 1".to_string()) {
                hit_breakpoint = true;
            }*/

            interp.step();
        }
    }

    /*let mut match_success = true;
    for idx in 0..output.len().max(interp.output.len()) {
        let game = output.get(idx);
        let interp = interp.output.get(idx);

        match (game, interp) {
            (Some((game_line, game)), Some(interp)) => {
                if game != interp {
                    eprintln!("Output difference at index {}:", idx);
                    eprintln!("MC ({:>4}):   {:?}", game_line, game);
                    eprintln!("Interpreted: {:?}", interp);
                    match_success = false;
                    break;
                }
            }
            (None, None) => {},
            (l, r) => todo!("{:?} {:?}", l, r),
        }
    }

    if match_success {
        println!("Successfully matched output!");
    }*/

    /*let ptr_read_small = langcraft::compile_ir::read_ptr_small(
        langcraft::cir::ScoreHolder::new("%temp1000".to_string()).unwrap(),
        false,
    );

    for c in ptr_read_small {
        println!("{}", c);
    }*/
}
