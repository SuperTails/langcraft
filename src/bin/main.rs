use langcraft::interpreter::InterpError;
use langcraft::{Datapack, Interpreter, BuildOptions};
use std::path::PathBuf;

// TODO: Allow specifying breakpoints somehow
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
                Ok(()) => {}
                Err(InterpError::BreakpointHit) => {
                    hit_breakpoint = true;
                }
                Err(e) => return Err(e.into()),
            }

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

pub struct Options {
    /// Run the generated code using the command interpreter
    pub interpret: bool,
    /// Compare output
    pub compare: bool,
    /// The path to the bitcode file to compile
    pub bc_path: PathBuf,
    pub output_folder: PathBuf,
    pub build_opts: BuildOptions,
}

fn parse_arguments() -> Result<Options, String> {
    let mut interpret = false;
    let mut compare = false;
    let mut trace_bbs = false;
    let mut force_input = false;
    let mut bc_path = None;
    let mut output_folder = None;

    let args = std::env::args().skip(1);

    for arg in args {
        if !force_input && arg.starts_with('-') {
            if arg == "--run" {
                interpret = true
            } else if arg == "--trace-bbs" {
                trace_bbs = true;
            } else if arg == "--compare" {
                compare = true;
            } else if arg.starts_with("--out=") {
                if output_folder.is_none() {
                    let tail = &arg["--out=".len()..];
                    output_folder = Some(PathBuf::from(tail));
                } else {
                    return Err(String::from("at most one `--out` argument may be specified"));
                }
            } else if arg == "--help" {
                // give help text then exit
                println!("Usage: langcraft [OPTION]... [FILE]...");
                println!("Convert an LLVM bitcode file to a Minecraft datapack");
                println!();
                println!("Options:");
                println!("\t--help          display this help message");
                println!("\t--out=PATH      specify the directory the datapack files should be placed in (default is `./out`)");
                println!("\t--run           run the command interpreter on the generated code");
                println!("\t--compare       compare the interpreter output to latest.log");
                println!("\t--trace-bbs     insert a print command at the beginning of each LLVM basic block");
                std::process::exit(0);
            } else if arg == "--" {
                // force potential options to be arguments
                force_input = true;
            } else {
                return Err(format!("invalid option `{}`",arg));
            }
        } else {
            // The non-option argument is a path
            if bc_path.is_none() {
                bc_path = Some(PathBuf::from(arg));
            } else {
                return Err("only one input file may be specified".into());
            }
        }
    }

    let output_folder = output_folder.unwrap_or_else(|| PathBuf::from("out/"));

    let bc_path = bc_path.ok_or("no input file was specified")?;

    if compare && !interpret {
        return Err(String::from("the `--compare` option requires `--run`"));
    }

    Ok(Options {
        interpret,
        compare,
        bc_path,
        output_folder,
        build_opts: BuildOptions {
            trace_bbs,
        }
    })
}

// TODO: Allow dynamically loading this, perhaps by reading a file?
const INPUT: &str = "FN MAIN() {    
LET FOO = 0    
WHILE FOO < 20{
FOO = FOO + 1 
IF FOO%15==0{ 
PRINT(300)   
} ELSE {        
IF FOO%5 == 0{
PRINT(200)   
} ELSE {        
IF FOO%3 == 0{
PRINT(100)   
} ELSE {        
PRINT(FOO)   
} } } } } }     ";

fn main() {
    let options = parse_arguments().unwrap_or_else(|err| {
        eprintln!("error when parsing arguments: {}", err);
        std::process::exit(1);
    });

    if options.output_folder.is_file() {
        eprintln!(
            "output path `{}` was a file",
            options.output_folder.display()
        );
        std::process::exit(1);
    }

    let datapack = Datapack::from_bc(&options.bc_path, &options.build_opts).unwrap_or_else(|err| {
        eprintln!("error when compiling: {}", err);
        std::process::exit(1);
    });

    println!(
        "Generated {} commands",
        datapack
            .functions
            .iter()
            .map(|f| f.cmds.len())
            .sum::<usize>()
    );

    datapack.save(&options.output_folder).unwrap_or_else(|err| {
        eprintln!("error when saving datapack: {}", err);
        std::process::exit(1);
    });

    if options.interpret {
        assert!(INPUT.len() < 256);
        let run_index = datapack.run_index().unwrap();
        let mut interp = Interpreter::new(datapack, run_index, &INPUT);

        match run_interpreter(&mut interp) {
            Ok(()) => {
                eprintln!("=== Begin output ===");
                for i in interp.output.iter() {
                    eprintln!("{}", i);
                }
                eprintln!("==== End output ====");
                eprintln!("Program finished normally in {} ticks", interp.tick);

                if options.compare {
                    compare_output(&interp);
                }
            }
            Err(err) => {
                eprintln!("==========================================");
                eprintln!("=== Output ===");
                for i in interp.output.iter() {
                    eprintln!("{}", i);
                }
                eprintln!("=== End output ===");
                eprintln!("=== Call stack ===");
                for (f, c) in interp.call_stack() {
                    eprintln!("{} line {}", f.id, f.get_line(c));
                }
                eprintln!("=== End call stack ===");
                eprintln!("Encountered interpreter error: {}", err);
            }
        }
    }
}
