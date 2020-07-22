use langcraft::Interpreter;
use langcraft::interpreter::{InterpError};
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
                        } else {
                            usize::from_str_radix(mem, 10)
                        };

                        match mem_idx {
                            Ok(mem_idx) => {
                                eprintln!(
                                    "Word at index {:#X} has value {}",
                                    mem_idx, interp.memory[mem_idx]
                                );
                            }
                            Err(err) => eprintln!("Invalid word index {:?}", err),
                        }
                    }
                    _ => eprintln!("Wrong number of arguments to `MEM`"),
                }
            } else {
                eprintln!("Invalid input {:?}", input);
            }
        } else {
            match interp.step() {
                Ok(()) => {},
                Err(InterpError::BreakpointHit) => {
                    hit_breakpoint = true;
                }
                Err(e) => return Err(e.into()),
            }

            /*if interp.next_command().map(|c| c.to_string().contains("block has this many")).unwrap_or(false) {
                hit_breakpoint = true;
            }*/


            /*if interp.next_command().map(|c| c.to_string().contains("tokens:")).unwrap_or(false) {
                hit_breakpoint = true;
            }

            if interp.next_command().map(|c| c.to_string().contains("intrinsic:bcmp")).unwrap_or(false) {
                hit_breakpoint = true;
            }*/

            /*if interp
                .next_command()
                .map(|c| c.to_string().contains("block count before printing:"))
                .unwrap_or(false)
            {
                hit_breakpoint = true;
            }*/

            if interp
                .next_command()
                .map(|c| c.to_string().contains("Panic"))
                .unwrap_or(false)
            {
                for o in interp.output.iter() {
                    println!("{:?}", o);
                }
                hit_breakpoint = true;
            }

            /*if interp
                .next_command()
                .map(|c| {
                    c.to_string()
                        == "scoreboard players operation %ptr rust = iter.sroa.0.0105%0 rust"
                })
                .unwrap_or(false)
            {
                hit_breakpoint = true;
            }

            if interp
                .next_command()
                .map(|c| c.to_string().contains("UNREACHABLE"))
                .unwrap_or(false)
            {
                hit_breakpoint = true;
            }*/
        }
    }

    Ok(())
}

pub fn compare_output(interp: &Interpreter) {
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

    let input =
"FN MAIN(){
LET FOO = 0
WHILE FOO < 20 {
FOO = FOO + 1
IF FOO%15 == 0 {
 PRINT(300)
} ELSE {
IF FOO%5 == 0 {
 PRINT(200)
} ELSE {
IF FOO%3 == 0 {
 PRINT(100)
} ELSE {
 PRINT(FOO)
} } } } } }";

    assert!(input.len() < 256);
    let mut interp = Interpreter::new(funcs, &input);

    //interp.set_mem_breakpoint(300, BreakKind::Access);

    match run_interpreter(&mut interp) {
        Ok(()) => {
            for i in interp.output.iter() {
                eprintln!("{}", i);
            }
            eprintln!("=== End output ===");
            eprintln!("Program finished normally");

            /*assert_eq!(
                interp
                    .get_rust_score(&langcraft::cir::ScoreHolder::new("%return%0".into()).unwrap())
                    .unwrap(),
                0
            );*/

            //compare_output(&interp);
        }
        Err(err) => {
            eprintln!("==========================================");
            eprintln!("Output:");
            for i in interp.output.iter() {
                eprintln!("{}", i);
            }
            eprintln!("=== End output ===");
            eprintln!("Encountered interpreter error: {}", err);
        }
    }
}
