use crate::cir::*;
use crate::compile_ir::{get_index, pos_to_func_idx, func_idx_to_pos, OBJECTIVE};
use crate::Datapack;
use std::collections::HashMap;
use std::str::FromStr;
use std::convert::TryFrom;

// FIXME: Multiple conditions and a `store success` does not work like I think it does!!!

#[derive(Debug, Clone, PartialEq)]
pub enum InterpError {
    OutOfBoundsAccess(i32, i32, i32),
    MaxCommandsRun,
    EnteredUnreachable,
    EnteredTodo,
    AssertionFailed,
    BreakpointHit,
    InvalidBranch(usize),
    MultiBranch(FunctionId, Option<FunctionId>),
}

impl std::fmt::Display for InterpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpError::OutOfBoundsAccess(x, y, z) => {
                write!(f, "out of bounds access at x={}, y={}, z={}", x, y, z)
            }
            InterpError::MaxCommandsRun => write!(f, "ran too many commands at once"),
            InterpError::EnteredUnreachable => write!(f, "entered unreachable code"),
            InterpError::EnteredTodo => write!(f, "entered code not yet implemented"),
            InterpError::AssertionFailed => write!(f, "assertion failed"),
            InterpError::BreakpointHit => write!(f, "breakpoint hit"),
            InterpError::InvalidBranch(b) => write!(f, "invalid branch to {}", b),
            InterpError::MultiBranch(prev, att) => {
                write!(f, "branch to more than one block (previous was {}, attempted was ", prev)?;
                if let Some(att) = att {
                    write!(f, "{}", att)?;
                } else {
                    write!(f, "invalid")?;
                }
                write!(f, ")")
            }
        }
    }
}

type RelPos = ((i32, bool), (i32, bool), (i32, bool));

fn add_rel_pos(base: (i32, i32, i32), rel: RelPos) -> (i32, i32, i32) {
    let do_coord = |lhs: i32, (rhs, rel)| -> i32 {
        if rel {
            lhs + rhs
        } else {
            rhs
        }
    };

    (do_coord(base.0, rel.0), do_coord(base.1, rel.1), do_coord(base.2, rel.2))
}

fn parse_rel_coords(pos: &str) -> Result<RelPos, String> {
    let mut coords = pos
        .split_whitespace()
        .map(|s| -> Result<_, String> {
            let (st, relative) = if s.starts_with('~') {
                if s == "~" {
                    return Ok((0, true))
                } else {
                    (&s[1..], true)
                }
            } else {
                (s, false)
            };

            let st_parsed = st.parse::<i32>().map_err(|_| format!("invalid {}", s))?;

            Ok((st_parsed, relative))
        });

    let x = coords.next().ok_or("expected x")??;
    let y = coords.next().ok_or("expected y")??;
    let z = coords.next().ok_or("expected z")??;
    
    if let Some(c) = coords.next() {
        return Err(format!("trailing data `{:?}`", c))
    }

    Ok((x, y, z))
}

impl std::error::Error for InterpError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BreakKind {
    Read,
    Write,
    Access,
}

pub enum RunState {
    Grid {
        next_pos: Option<(usize, usize)>,
    },
    Chain {
        resume_cmd: Option<Command>,
        cmd_1: Option<Command>,
        cmd_2: Option<Command>,
        /// The index of the currently executing command block in the chain, i.e. the actual top of the call stack
        idx: Option<usize>,
        tick_queued: bool,
        next_pos: (i32, i32, i32),
    },
}

pub struct Interpreter {
    pub rust_scores: HashMap<ScoreHolder, i32>,
    pub(crate) call_stack: Vec<(usize, usize, (i32, i32, i32))>,
    program: Vec<Function>,
    pub memory: [i32; 128 * 16 * 16],
    ptr_pos: (i32, i32, i32),
    turtle_pos: (i32, i32, i32),
    run_state: RunState,
    letters: HashMap<(i32, i32, i32), char>,
    pub output: Vec<String>,
    pub tick: usize,
    commands_run: usize,
    memory_points: HashMap<usize, BreakKind>,
    /// FIXME: Add support for the *real* commands
    stdout_buffer: String,
}

impl Interpreter {
    /// Does not include any intrinsics
    pub fn new_raw(program: Vec<Function>, input: &str) -> Self {
        let func_idx = program.len() - 1;

        let mut letters = HashMap::new();
        for (z, letter) in input.chars().enumerate() {
            if letter != ' ' {
                letters.insert((-16, 16, -(z as i32)), letter);
            }
        }

        Interpreter {
            program,
            call_stack: vec![(func_idx, 0, (0, 0, 0))],
            memory: [0; 128 * 16 * 16],
            rust_scores: HashMap::new(),
            ptr_pos: (0, 0, 0),
            turtle_pos: (0, 0, 0),
            run_state: RunState::Chain { resume_cmd: None, cmd_1: None, cmd_2: None, idx: None, tick_queued: false, next_pos: (0, 0, 0) },
            commands_run: 0,
            tick: 0,
            output: Vec::new(),
            memory_points: HashMap::new(),
            letters,
            stdout_buffer: String::new(),
        }
    }

    pub fn new(datapack: Datapack, start_idx: usize, input: &str) -> Self {
        let mut letters = HashMap::new();
        let mut z = 0;
        let mut y = 32;
        for letter in input.chars() {
            match letter {
                '\n' => {
                    z = 0;
                    y -= 2;
                }
                ' ' => {
                    z -= 1;
                }
                _ => {
                    letters.insert((-16, y, z), letter);
                    z -= 1;
                }
            }
        }

        // TODO: Remove
        let mut rust_scores = HashMap::new();
        rust_scores.insert(ScoreHolder::new("%%CMD_LIMIT".into()).unwrap(), 10_000);
        

        let is_chain = datapack.functions[start_idx].cmds.last().unwrap().to_string() == "setblock -2 1 0 minecraft:redstone_block replace";

        let run_state = if is_chain {
            RunState::Chain { resume_cmd: None, cmd_1: None, cmd_2: None, idx: None, tick_queued: false, next_pos: (0, 0, 0) }
        } else {
            RunState::Grid { next_pos: Some((start_idx, 0)) }
        };

        Interpreter {
            program: datapack.functions,
            call_stack: vec![(start_idx, 0, (0, 0, 0))],
            memory: [0x55_55_55_55; 128 * 16 * 16],
            rust_scores,
            ptr_pos: (0, 0, 0),
            turtle_pos: (0, 0, 0),
            run_state,
            tick: 0,
            commands_run: 0,
            output: Vec::new(),
            memory_points: HashMap::new(),
            letters,
            stdout_buffer: String::new(),
        }
    }

    pub fn program(&self) -> &[Function] {
        &self.program
    }

    pub fn call_stack(&self) -> Vec<(&Function, usize)> {
        self.call_stack
            .iter()
            .copied()
            .map(|(f, c, _)| (&self.program[f], c))
            .collect()
    }

    /// `word_start` is in bytes, must be aligned to a multiple of 4
    pub fn set_mem_breakpoint(&mut self, word_start: usize, kind: BreakKind) {
        assert_eq!(word_start % 4, 0);

        self.memory_points.insert(word_start / 4, kind);
    }

    pub fn take_next_pos(&mut self) -> Option<(usize, usize)> {
        match &mut self.run_state {
            RunState::Grid { next_pos } => {
                std::mem::take(next_pos)
            }
            _ => panic!(),
        }
    }

    pub fn set_next_pos(&mut self, func_idx: usize) -> Result<(), InterpError> {
        match &mut self.run_state {
            RunState::Grid { next_pos } => {
                if let Some((f, _)) = next_pos {
                    let att = self.program.get(func_idx).map(|f| f.id.clone());
                    Err(InterpError::MultiBranch(self.program[*f].id.clone(), att))
                } else if func_idx >= self.program.len() {
                    Err(InterpError::InvalidBranch(func_idx))
                } else {
                    *next_pos = Some((func_idx, 0));
                    Ok(())
                }
            }
            _ => panic!(),
        }
    }

    pub fn get_word(&self, addr: usize) -> Result<i32, InterpError> {
        assert_eq!(addr % 4, 0);

        match self.memory_points.get(&(addr / 4)) {
            Some(BreakKind::Access) | Some(BreakKind::Read) => {
                return Err(InterpError::BreakpointHit)
            }
            _ => {}
        };

        Ok(self.memory[addr / 4])
    }

    pub fn get_byte(&self, addr: usize) -> Result<u8, InterpError> {
        let word_addr = (addr / 4) * 4;

        Ok(self.get_word(word_addr)?.to_le_bytes()[addr % 4])
    }

    pub fn set_word(&mut self, value: i32, addr: usize) -> Result<(), InterpError> {
        assert_eq!(addr % 4, 0);

        match self.memory_points.get(&(addr / 4)) {
            Some(BreakKind::Access) | Some(BreakKind::Write) => {
                return Err(InterpError::BreakpointHit)
            }
            _ => {}
        };

        self.memory[addr / 4] = value;

        Ok(())
    }

    pub fn set_byte(&mut self, value: u8, addr: usize) -> Result<(), InterpError> {
        match self.memory_points.get(&(addr / 4)) {
            Some(BreakKind::Access) | Some(BreakKind::Write) => {
                return Err(InterpError::BreakpointHit)
            }
            _ => {}
        };

        let mut bytes = self.memory[addr / 4].to_le_bytes();
        bytes[addr % 4] = value;
        self.memory[addr / 4] = i32::from_le_bytes(bytes);

        Ok(())
    }

    /// Runs until the program halts
    pub fn run_to_end(&mut self) -> Result<(), InterpError> {
        while !self.halted() {
            self.step()?
        }
        Ok(())
    }

    pub fn next_command(&self) -> Option<&Command> {
        if !self.halted() {
            let (func_idx, cmd_idx, _) = self.call_stack.last().unwrap();
            Some(&self.program[*func_idx].cmds[*cmd_idx])
        } else {
            None
        }
    }

    pub fn eval_message(&self, msg: &[TextComponent]) -> String {
        let mut result = String::new();
        let score_getter = |name: &ScoreHolder, obj: &Objective| -> Option<i32> {
            if obj != OBJECTIVE {
                None
            } else {
                self.rust_scores.get(name).copied()
            }
        };

        for s in msg.iter().map(|m| m.as_string(&score_getter).unwrap()) {
            result.push_str(&s);
        }
        result
    }

    fn turtle_pos_set(&mut self, coord: usize, cmd: &Command) {
        let exec = if let Command::Execute(Execute {
            run: Some(exec), ..
        }) = cmd
        {
            &**exec
        } else {
            unreachable!()
        };

        if let Command::ScoreGet(ScoreGet {
            target: Target::Uuid(target),
            target_obj,
        }) = exec
        {
            if target_obj != OBJECTIVE {
                todo!("{:?}", target_obj)
            }

            let value = self.get_rust_score(target).unwrap();

            match coord {
                0 => self.turtle_pos.0 = value,
                1 => self.turtle_pos.1 = value,
                2 => self.turtle_pos.2 = value,
                _ => panic!("{}", coord),
            }
        } else {
            unreachable!()
        }
    }

    fn ptr_pos_set(&mut self, coord: usize, cmd: &Command) {
        let exec = if let Command::Execute(Execute {
            run: Some(exec), ..
        }) = cmd
        {
            &**exec
        } else {
            unreachable!()
        };

        if let Command::ScoreGet(ScoreGet {
            target: Target::Uuid(target),
            target_obj,
        }) = exec
        {
            if target_obj != OBJECTIVE {
                todo!("{:?}", target_obj)
            }

            let value = self.get_rust_score(target).unwrap();

            match coord {
                0 => self.ptr_pos.0 = value,
                1 => self.ptr_pos.1 = value,
                2 => self.ptr_pos.2 = value,
                _ => panic!("{}", coord),
            }
        } else {
            unreachable!()
        }
    }

    pub fn get_rust_score(&self, holder: &ScoreHolder) -> Result<i32, String> {
        self.rust_scores
            .get(&holder)
            .copied()
            .ok_or_else(|| format!("read from uninitialized variable {}", holder))
    }

    pub fn check_cond(&self, is_unless: bool, cond: &ExecuteCondition) -> bool {
        let result = match cond {
            ExecuteCondition::Score {
                target: Target::Uuid(target),
                target_obj,
                kind,
            } => {
                if target_obj != OBJECTIVE {
                    todo!("{:?}", target_obj)
                }

                let target = self.get_rust_score(target).unwrap();

                match kind {
                    ExecuteCondKind::Relation {
                        relation,
                        source: Target::Uuid(source),
                        source_obj,
                    } => {
                        if source_obj != OBJECTIVE {
                            todo!("{:?}", source_obj)
                        }

                        let source = self.get_rust_score(source).unwrap();

                        match relation {
                            Relation::LessThan => target < source,
                            Relation::LessThanEq => target <= source,
                            Relation::Eq => target == source,
                            Relation::GreaterThan => target > source,
                            Relation::GreaterThanEq => target >= source,
                        }
                    }
                    ExecuteCondKind::Matches(m) => m.contains(target),
                    _ => todo!("{:?}", kind),
                }
            }
            _ => todo!("{:?}", cond),
        };

        if is_unless {
            !result
        } else {
            result
        }
    }

    fn read_mem(&self) -> Result<i32, InterpError> {
        let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2)?;
        self.get_word(index as usize)
    }

    fn execute_setblock(&mut self, pos: (i32, i32, i32), block: &str, _kind: crate::cir::SetBlockKind) {
        match &mut self.run_state {
            RunState::Grid { .. } => {
                /*if block.starts_with("minecraft:command_block") {
                    // Command block placement
                    println!("Command block placement at {} {} {} block {}", x, y, z, block);
                } else if x == 0 && x_rel && y == 1 && y_rel && z == 0 && z_rel {
                    if block == "minecraft:redstone_block" {
                        println!("Branching to self");

                        if let [(func_idx, _)] = &self.call_stack[..] {
                            let func_idx = *func_idx;
                            self.set_next_pos(func_idx)?;
                        } else {
                            todo!()
                        }
                    } else if block == "minecraft:air" {
                        // Do nothing
                    } else {
                        todo!("{:?}", cmd)
                    }
                } else {
                    if x_rel {
                        todo!()
                    }

                    if z_rel {
                        todo!()
                    }

                    if y != 1 {
                        todo!("{} {}", pos, block);
                    } else if block == "minecraft:redstone_block" {
                        let idx = pos_to_func_idx(x, z);

                        println!("Branching to {}", self.program[idx as usize].id);

                        self.set_next_pos(idx as usize)?;
                    }
                }*/
            }
            RunState::Chain { resume_cmd, cmd_1, cmd_2, tick_queued, .. } => {
                let extract_cmd = |block: &str| {
                    assert!(block.contains("command_block"), "{}", block);

                    let idx = block.find("Command:\"")?;
                    let cmd = &block[idx + "Command:\"".len()..block.len() - 2];
                    if cmd.is_empty() {
                        None
                    } else {
                        Some(cmd.parse().unwrap())
                    }
                };

                if pos == (-2, 0, 0) && block == "minecraft:command_block[facing=south]{Command:\"function rust:__langcraft_on_tick\"}" {
                    println!("Placed on-tick command block");
                } else if pos == (-2, 1, 1) {
                    *resume_cmd = extract_cmd(block);
                } else if pos == (-2, 0, 2) {
                    if block == "minecraft:air" {
                        *cmd_2 = None;
                    } else {
                        *cmd_2 = extract_cmd(block);
                    }
                } else if pos == (-2, 1, 0) {
                    if block == "minecraft:redstone_block" {
                        assert_eq!(*tick_queued, false);
                        *tick_queued = true;
                    } else if block == "minecraft:air" {
                        // Removing the block
                    } else {
                        todo!()
                    }
                } else if pos == (-2, 0, 1) && block == "minecraft:air" {
                    *cmd_1 = None;
                } else {
                    todo!("{:?}: {}", pos, block)
                }
            }
        }
    }

    fn execute_cmd(&mut self, pos: (i32, i32, i32), cmd: &Command) -> Result<(), InterpError> {
        //eprintln!("{}", cmd);
        /*if !self
            .call_stack
            .iter()
            .any(|(i, _)| self.program[*i].id.name.contains("intrinsic"))
        {
            eprintln!("{}", cmd);
        }*/

        match cmd {
            Command::ScoreAdd(ScoreAdd { target: Target::Uuid(target), target_obj, score }) => {
                if target_obj != OBJECTIVE {
                    todo!("{:?}", target_obj)
                }

                let mut lhs = self.get_rust_score(target).unwrap();
                lhs = lhs.wrapping_add(*score);
                self.rust_scores.insert(target.clone(), lhs);
            }
            Command::ScoreOp(ScoreOp { target, target_obj, kind, source, source_obj }) => {
                if target_obj != OBJECTIVE {
                    todo!("{}", target_obj);
                }

                if source_obj != OBJECTIVE {
                    todo!("{}", source_obj);
                }

                if let Target::Uuid(target) = target {
                    if let Target::Uuid(source) = source {
                        let rhs = self.get_rust_score(source).unwrap();

                        match kind {
                            ScoreOpKind::Assign => {
                                self.rust_scores.insert(target.clone(), rhs);
                            }
                            ScoreOpKind::AddAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                val = val.wrapping_add(rhs);
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::SubAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                val -= rhs;
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::MulAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                val = val.wrapping_mul(rhs);
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::DivAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                val /= rhs;
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::ModAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                if rhs < 0 {
                                    todo!("DETERMINE BEHAVIOR")
                                } else {
                                    val = val.rem_euclid(rhs);
                                }
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::Min => {
                                let mut val = self.get_rust_score(target).unwrap();
                                val = val.min(rhs);
                                self.rust_scores.insert(target.clone(), val);
                            }
                            _ => todo!("{}", kind)
                        }
                    } else {
                        todo!("{}", source)
                    }
                } else {
                    todo!("{}", target)
                }
            }
            Command::FuncCall(FuncCall { id }) => {
                if id.name == "stdout:putc" {
                    let c = self.get_rust_score(&ScoreHolder::new("%%temp0_putc".into()).unwrap()).unwrap();
                    let c = char::from(u8::try_from(c).unwrap_or_else(|_| panic!("invalid argument to stdout:putc {}", c)));

                    if c == '\n' {
                        let out = std::mem::take(&mut self.stdout_buffer);
                        self.output.push(out);
                    } else if c.is_ascii() && !c.is_control() {
                        self.stdout_buffer.push(c);
                    } else {
                        panic!("invalid argument to stdout:putc `{}`", c)
                    }
                } else {
                    let called_idx = self.program.iter().enumerate().find(|(_, f)| &f.id == id).unwrap_or_else(|| todo!("{:?}", id)).0;
                    self.call_stack.push((called_idx, 0, pos));
                }
            }
            Command::Fill(Fill { start: _, end: _, block }) => {
                if block != "minecraft:air" {
                    todo!()
                }
            }
            Command::Data(Data { target, kind }) => {
                match (target, kind) {
                    (DataTarget::Block(block), DataKind::Modify { path, kind: DataModifyKind::Set, source }) => {
                        let target_pos = add_rel_pos(pos, parse_rel_coords(block).unwrap());

                        if path == "RecordItem.tag.Memory" {
                            if let DataModifySource::Value(score) = source {
                                self.set_word(*score, get_index(target_pos.0, target_pos.1, target_pos.2)? as usize)?;
                            } else {
                                todo!()
                            }
                        } else if path == "Command" {
                            if let DataModifySource::ValueString(s) = source {
                                let new_cmd = if s.is_empty() {
                                    None
                                } else {
                                    Some(s.parse().unwrap())
                                };

                                if let RunState::Chain { resume_cmd, cmd_1, cmd_2, .. } = &mut self.run_state {
                                    match target_pos {
                                        (-2, 1, 1) => *resume_cmd = new_cmd,
                                        (-2, 0, 1) => *cmd_1 = new_cmd,
                                        (-2, 0, 2) => *cmd_2 = new_cmd,
                                        _ => todo!(),
                                    }
                                } else {
                                    todo!()
                                }
                            } else {
                                panic!("command must be string")
                            }
                        } else {
                            todo!("{}", path)
                        }
                    }
                    _ => todo!("{:?} {:?}", target, kind),
                }
            }
            Command::ScoreSet(ScoreSet { target, target_obj, score }) => {
                if target_obj != OBJECTIVE {
                    todo!("{:?}", target_obj)
                }

                match target {
                    Target::Uuid(target) => {
                        self.rust_scores.insert(target.clone(), *score);
                    }
                    _ => todo!("{:?}", target)
                }
            }
            Command::Tellraw(b) => {
                let Tellraw { message, target: _target } = &**b;
                let msg = self.eval_message(&message);
                println!("\n{}\n", msg);
                self.output.push(msg);
            }
            Command::SetBlock(SetBlock { pos: block_pos, block, kind }) => {
                let rel_pos = parse_rel_coords(block_pos).unwrap();

                let block_pos = add_rel_pos(pos, rel_pos);

                self.execute_setblock(block_pos, block, *kind);
            }
            cmd if cmd.to_string().starts_with("execute as @e[tag=turtle] store result entity @s Pos[0] double 1 run") => {
                self.turtle_pos_set(0, cmd)
            }
            cmd if cmd.to_string().starts_with("execute as @e[tag=turtle] store result entity @s Pos[1] double 1 run") => {
                self.turtle_pos_set(1, cmd)
            }
            cmd if cmd.to_string().starts_with("execute as @e[tag=turtle] store result entity @s Pos[2] double 1 run") => {
                self.turtle_pos_set(2, cmd)
            }
            cmd if cmd.to_string().starts_with("execute at @e[tag=turtle] if block ~ ~ ~ minecraft:") => {
                let (run, subcmds) = if let Command::Execute(Execute { run: Some(run), subcommands }) = cmd {
                    (&**run, subcommands)
                } else {
                    unreachable!()
                };

                if let [_at, ExecuteSubCmd::Condition { is_unless: false, cond: ExecuteCondition::Block { block, .. } }] = &subcmds[..] {
                    let len = block.len();
                    let letter = block.chars().nth(len - 6).unwrap();
                    if self.letters.get(&self.turtle_pos) == Some(&letter) {
                        self.execute_cmd(self.turtle_pos, run)?;
                    }
                } else {
                    unreachable!()
                }
            }
            cmd if cmd.to_string().starts_with("execute at @e[tag=ptr] run setblock") => {
                let (block, pos) = if let Command::Execute(Execute { run: Some(run), .. }) = cmd {
                    if let Command::SetBlock(SetBlock { block, pos, kind: _ }) = &**run {
                        (block, pos)
                    } else {
                        unreachable!()
                    }
                } else {
                    unreachable!()
                };

                let mut coords = pos
                    .split_whitespace()
                    .map(|s| {
                        let (st, relative) = if s.starts_with('~') {
                            if s == "~" {
                                return (0, true)
                            } else {
                                (&s[1..], true)
                            }
                        } else {
                            (s, false)
                        };
                        (st.parse::<i32>().unwrap_or_else(|_| panic!("invalid {}", s)), relative)
                    });

                let (x, x_rel) = coords.next().unwrap();
                let (y, y_rel) = coords.next().unwrap();
                let (z, z_rel) = coords.next().unwrap();

                if block == "minecraft:redstone_block" {
                    let x = if x_rel {
                        x + self.ptr_pos.0
                    } else {
                        x
                    };
                    let y = if y_rel {
                        y + self.ptr_pos.1
                    } else {
                        y
                    };
                    let z = if z_rel {
                        z + self.ptr_pos.2
                    } else {
                        z
                    };

                    if y == 1 {
                        let idx = pos_to_func_idx(x, z);
                        self.set_next_pos(idx as usize)?;
                        println!("Dynamic branch to {}", self.program[idx].id);
                    } else {
                        panic!("attempt to branch improperly")
                    }
                }
            }
            cmd if cmd.to_string().starts_with("execute at @e[tag=ptr] run") => {
                let run = if let Command::Execute(Execute { run: Some(d), .. }) = cmd {
                    &**d
                } else {
                    unreachable!()
                };

                if let Command::Data(Data { target: DataTarget::Block(block), kind: DataKind::Modify { path, kind: DataModifyKind::Set, source: DataModifySource::Value(v) } }) = run {
                    let index = match block.as_str() {
                        "~ ~ ~" => {
                            get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2)
                        }
                        "~-2 1 ~" => {
                            get_index(self.ptr_pos.0 - 2, 1, self.ptr_pos.2)
                        }
                        _ => todo!("{:?}", block)
                    };

                    if path != "RecordItem.tag.Memory" {
                        todo!("{:?}", path);
                    }

                    self.set_word(*v, index.unwrap() as usize)?;
                } else {
                    todo!("{:?}", cmd)
                }
            }
            cmd if cmd.to_string().starts_with("execute at @e[tag=ptr]") => {
                let (subcmds, run) = if let Command::Execute(Execute { run: Some(run), subcommands }) = cmd {
                    (subcommands, run)
                } else {
                    todo!("{:?}", cmd)
                };

                if subcmds.len() == 1 {
                    todo!("{}", cmd)
                }

                if let ExecuteSubCmd::Store { is_success: false, kind: ExecuteStoreKind::Score { target: Target::Uuid(target), objective } } = &subcmds[1] {
                    if objective != OBJECTIVE {
                        todo!("{:?}", objective)
                    }

                    if run.to_string() == "data get block ~ ~ ~ RecordItem.tag.Memory 1" {
                        let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2)?;
                        let word = self.get_word(index as usize)?;
                        self.rust_scores.insert(target.clone(), word);
                    } else {
                        todo!()
                    }
                } else if subcmds[1].to_string() == "store result block ~ ~ ~ RecordItem.tag.Memory int 1" {
                    if let Command::ScoreGet(ScoreGet { target: Target::Uuid(target), target_obj }) = &**run {
                        if target_obj != OBJECTIVE {
                            todo!("{:?}", target_obj)
                        }

                        let val = *self.rust_scores.get(target).unwrap_or_else(|| panic!("read from uninitialized variable {}", target));
                        let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2)?;
                        self.set_word(val, index as usize)?;
                    }
                } else {
                    todo!("{:?} {}", subcmds[1].to_string(), cmd)
                }
            }
            cmd if cmd.to_string().starts_with("execute as @e[tag=ptr] store result entity @s Pos[0] double 1 run") => {
                self.ptr_pos_set(0, cmd);
            }
            cmd if cmd.to_string().starts_with("execute as @e[tag=ptr] store result entity @s Pos[1] double 1 run") => {
                self.ptr_pos_set(1, cmd);
            }
            cmd if cmd.to_string().starts_with("execute as @e[tag=ptr] store result entity @s Pos[2] double 1 run") => {
                self.ptr_pos_set(2, cmd);
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] at @s store result entity @s Pos[2] double 1 run data get block ~ ~ ~ RecordItem.tag.Memory 1" => {
                self.ptr_pos.2 = self.read_mem()?;
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] at @s run tp @s ~-2 1 ~" => {
                self.ptr_pos.0 += -2;
                self.ptr_pos.1 = 1;
            }
            cmd if cmd.to_string().starts_with("execute at @e[tag=ptr] store result block ~ ~ ~ RecordItem.tag.Memory int 1 run") => {
                let sg = if let Command::Execute(Execute { run: Some(sg), .. }) = cmd {
                    &**sg
                } else {
                    unreachable!()
                };

                if let Command::ScoreGet(ScoreGet { target: Target::Uuid(target), target_obj }) = sg {
                    if target_obj != OBJECTIVE {
                        todo!("{:?}", target_obj)
                    }

                    let val = *self.rust_scores.get(target).unwrap_or_else(|| panic!("read from uninitialized variable {}", target));
                    let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2)?;
                    self.set_word(val, index as usize)?;
                } else {
                    todo!("{:?}", sg)
                }
            }
            Command::Execute(Execute { run: Some(run), subcommands }) => {
                if cmd.to_string().starts_with("execute at @e[tag=turtle] run setblock ~ ~ ~") {
                    if let Command::SetBlock(SetBlock { pos: _, block, kind: _ }) = &**run {
                        eprintln!(
                            "Placed block at {} {} {}: {:?}",
                            self.turtle_pos.0,
                            self.turtle_pos.1,
                            self.turtle_pos.2,
                            block
                        );
                    } else {
                        unreachable!()
                    }
                } else {
                    let mut passes = true;
                    let mut dest_pos = pos;

                    for subcmd in subcommands {
                        match subcmd {
                            ExecuteSubCmd::Condition { is_unless, cond } => {
                                if !self.check_cond(*is_unless, cond) {
                                    passes = false;
                                }
                            }
                            ExecuteSubCmd::At { target: Target::Selector(selector) } => {
                                match selector.to_string().as_str() {
                                    "@e[tag=next]" => {
                                        if let RunState::Chain { next_pos, .. } = &self.run_state {
                                            dest_pos = *next_pos;
                                        } else {
                                            todo!()
                                        }
                                    }
                                    s => todo!("{}", s)
                                }
                            }
                            ExecuteSubCmd::Positioned { pos } => {
                                if pos == "-2 1 1" {
                                    dest_pos = (-2, 1, 1);
                                } else {
                                    todo!()
                                }
                            }
                            _ => todo!("{:?}", subcmd)
                        }
                    }

                    if passes {
                        self.execute_cmd(dest_pos, &**run)?;
                    }
                }
            }
            Command::Execute(Execute { run: None, subcommands }) => {
                let store_target = if let ExecuteSubCmd::Store { is_success: true, kind: ExecuteStoreKind::Score { target: Target::Uuid(target), objective } } = &subcommands[0] {
                    if objective != OBJECTIVE {
                        todo!("{:?}", objective)
                    }

                    target
                } else {
                    todo!("{:?}", subcommands[0])
                };

                if subcommands[1..].iter().all(|sc| matches!(sc, ExecuteSubCmd::Condition { .. })) {
                    let mut result = true;
                    for subcmd in subcommands[1..].iter() {
                        if let ExecuteSubCmd::Condition { is_unless, cond } = subcmd {
                            result = result && self.check_cond(*is_unless, cond);
                        } else {
                            unreachable!()
                        }
                    }

                    self.rust_scores.insert(store_target.clone(), result as i32);
                } else {
                    todo!()
                }
            }
            Command::CloneCmd(c) if c.to_string() == "clone ~ ~1 ~1 ~ ~1 ~1 ~ ~ ~1" && pos == (-2, 0, 0) => {
                if let RunState::Chain { resume_cmd, cmd_1, .. } = &mut self.run_state {
                    *cmd_1 = resume_cmd.clone();
                } else {
                    todo!()
                }
            }
            Command::Teleport(Teleport { target, pos: block_pos }) if target.to_string() == "@e[tag=next]" => {
                if let RunState::Chain { next_pos, .. } = &mut self.run_state {
                    *next_pos = add_rel_pos(pos, parse_rel_coords(block_pos).unwrap());
                } else {
                    todo!()
                }
            }
            Command::Comment(c) if c == "!INTERPRETER: TODO" => {
                return Err(InterpError::EnteredTodo);
            }
            Command::Comment(c) if c == "!INTERPRETER: UNREACHABLE" => {
                return Err(InterpError::EnteredUnreachable);
            }
            Command::Comment(c) if c.starts_with("!INTERPRETER: ASSERT ") => {
                let c = &c["!INTERPRETER: ASSERT ".len()..];
                let (c, is_unless) = if c.starts_with("unless ") {
                    (&c["unless ".len()..], true)
                } else if c.starts_with("if ") {
                    (&c["if ".len()..], false)
                } else {
                    todo!()
                };

                let cond = ExecuteCondition::from_str(c).unwrap();

                if !self.check_cond(is_unless, &cond) {
                    eprintln!("Currently at:");
                    for (f, c, p) in self.call_stack.iter() {
                        eprintln!("{}, {} at {:?}", self.program[*f].id, c, p);
                    }
                    return Err(InterpError::AssertionFailed);
                }
            }
            Command::Comment(_) => {}
            cmd => todo!("{}", cmd)
        }

        Ok(())
    }

    pub fn step(&mut self) -> Result<(), InterpError> {
        // In chain mode, a step goes like this:
        // 1. Process the current command block if the call stack is empty (i.e. we've just started)
        // 2. Run a command
        // 3. Update the call stack
        // 4. Determine which command block to use next, if necessary

        if self.commands_run >= 60_000 {
            return Err(InterpError::MaxCommandsRun);
        }

        let top_func_idx = self.call_stack.first().unwrap().0;
        let top_func = self.program[top_func_idx].id.to_string();

        let (func_idx, cmd_idx, pos) = self.call_stack.last_mut().unwrap();
        let pos = *pos;

        //println!("Function {} at command {}", self.program[*func_idx].id, cmd_idx);

        /*let pos = match &self.run_state {
            RunState::Grid { .. } => {
            }
            RunState::Chain { idx, .. } => {
            }
        };*/

        let cmd = &self.program[*func_idx].cmds[*cmd_idx].clone();
        *cmd_idx += 1;
        self.execute_cmd(pos, cmd)?;

        self.commands_run += 1;

        loop {
            if self.call_stack.is_empty() {
                self.tick += 1;
                println!(
                    "Executed {} commands from function '{}'",
                    self.commands_run, top_func,
                );
                self.commands_run = 0;

                match &mut self.run_state {
                    RunState::Grid { next_pos } => {
                        if let Some(next_pos) = std::mem::take(next_pos) {
                            let (x, z) = func_idx_to_pos(top_func_idx);

                            eprintln!("\nNow about to execute {}", &self.program[next_pos.0].id);
                            self.call_stack.push((next_pos.0, next_pos.1, (x, 0, z)));
                        }
                    }
                    RunState::Chain { cmd_1, cmd_2, idx, tick_queued, .. } => {
                        // If the current command block is finished, we can get the next one ready
                        if let Some(prev_idx) = idx {
                            *prev_idx = match prev_idx {
                                0 => 1,
                                1 => 2,
                                2 => 1,
                                _ => unreachable!(),
                            };
                        }

                        // This basically only ever happens at the very beginning of the program
                        if idx.is_none() && *tick_queued {
                            *tick_queued = false;
                            *idx = Some(0);
                        }

                        let on_tick_func = Some(FuncCall { id: FunctionId::new("__langcraft_on_tick") }.into());

                        let mut next_cmd = match idx {
                            Some(0) => &on_tick_func,
                            Some(1) => cmd_1,
                            Some(2) => cmd_2,
                            None => &None,
                            _ => unreachable!(),
                        };

                        if next_cmd.is_none() && *tick_queued {
                            *idx = Some(0);
                            *tick_queued = false;
                            next_cmd = &on_tick_func;
                        }

                        if let Some(next_cmd) = next_cmd {
                            if let Command::FuncCall(FuncCall { id }) = next_cmd {
                                let mut id_no_rust = id.clone();
                                if id_no_rust.name.starts_with("rust:") {
                                    id_no_rust.name = id_no_rust.name[5..].to_owned();
                                }

                                let pos = match idx {
                                    Some(0) => (-2, 0, 0),
                                    Some(1) => (-2, 0, 1),
                                    Some(2) => (-2, 0, 2),
                                    None => (0, 0, 0),
                                    _ => unreachable!(),
                                };

                                let called_idx = self.program.iter().enumerate().find(|(_, f)| &f.id == id || f.id == id_no_rust).unwrap_or_else(|| todo!("{:?}", id)).0;
                                self.call_stack.push((called_idx, 0, pos));
                            }
                        }
                    }
                }

                break;
            }

            let (func_idx, cmd_idx, _) = self.call_stack.last().unwrap();

            if self.program[*func_idx].cmds.len() == *cmd_idx {
                self.call_stack.pop();
            } else {
                break;
            }
        }

        Ok(())
    }

    pub fn halted(&self) -> bool {
        let tick_queued = if let RunState::Chain { tick_queued, .. } = &self.run_state {
            *tick_queued
        } else {
            false
        };

        matches!(self.call_stack.last(), Some(&(0xFFFF_FFFF_FFFF_FFFF, 0, _))) || (self.call_stack.is_empty() && !tick_queued)
    }
}
