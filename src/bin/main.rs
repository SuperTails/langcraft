use langcraft::Interpreter;
use std::path::Path;

fn run_interpreter(interp: &mut Interpreter) -> Result<(), Box<dyn std::error::Error>> {
    let mut hit_breakpoint = false;

    let stdin = std::io::stdin();

    while !interp.halted() {
        if hit_breakpoint {
            let mut input = String::new();
            stdin.read_line(&mut input)?;
            input = input.trim().to_string();

            if input == "" || input == "STEP" {
                eprintln!("{}", interp.next_command().unwrap());
                interp.step()?;
            } else if input == "CONT" {
                eprintln!("Continuing");
                hit_breakpoint = false;
            } else if input.starts_with("REG") {
                let words = input.split_whitespace().collect::<Vec<_>>();
                match &words[..] {
                    ["REG", reg] => {
                        if let Ok(holder) = langcraft::cir::ScoreHolder::new(reg.to_string()) {
                            if let Some(score) = interp.rust_scores.get(&holder) {
                                eprintln!("Holder {} has score {}", holder, score);
                            } else {
                                eprintln!("Holder {} has no score", holder);
                            }
                        } else {
                            eprintln!("Invalid register name")
                        }
                    }
                    _ => eprintln!("Too many arguments to `REG`"),
                }
            } else if input.starts_with("MEM") {
                let words = input.split_whitespace().collect::<Vec<_>>();
                match &words[..] {
                    ["MEM", mem] => {
                        let mem_idx = if mem.starts_with("0x") {
                            usize::from_str_radix(&mem[2..], 16)
                        }  else {
                            usize::from_str_radix(mem, 10)
                        };

                        match mem_idx {
                            Ok(mem_idx) => {
                                eprintln!("Word at index {:#X} has value {}", mem_idx, interp.memory[mem_idx]);
                            }
                            Err(err) => {
                                eprintln!("Invalid word index {:?}", err)
                            }
                        }
                    }
                    _ => eprintln!("Wrong number of arguments to `MEM`"),
                }
                eprintln!("TODO: `MEM`");
            } else {
                eprintln!("Invalid input {:?}", input);
            }
        } else {
            if interp.next_command().map(|c| c.to_string().contains("block main-blockbb5_preheader_i_i_i_i_i_i")).unwrap_or(false) {
                hit_breakpoint = true;
            }
            /*if interp.next_command().map(|c| c.to_string()) == Some("execute at @e[tag=ptr] store result score %41%0 rust run data get block ~ ~ ~ RecordItem.tag.Memory 1".to_string()) {
                hit_breakpoint = true;
            }*/

            interp.step()?;
        }
    }

    Ok(())
}

fn compare_output(interp: &Interpreter) {
    let output = std::fs::read_to_string("latest.log")
        .unwrap()
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            l.contains("Render thread")
                && l.contains("[CHAT]")
                && !l.contains('@')
                && !l.contains("Executed") /* TODO: Show this in the interpreter output */
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
    println!();


    let mut match_success = true;
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
            (None, None) => {}
            (l, r) => todo!("{:?} {:?}", l, r),
        }
    }

    if match_success {
        println!("Successfully matched output!");
    }
}

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

    let mut interp = Interpreter::new(funcs, "FN ABC(){}");

    match run_interpreter(&mut interp) {
        Ok(()) => {
            eprintln!("Program finished normally");
            eprintln!("Output:");
            for i in interp.output.iter() {
                eprintln!("{}", i);
            }

            compare_output(&interp);
        }
        Err(err) => {
            eprintln!("Encountered interpreter error: {}", err);
            eprintln!("Output:");
            for i in interp.output.iter() {
                eprintln!("{}", i);
            }
        }
    }
}
