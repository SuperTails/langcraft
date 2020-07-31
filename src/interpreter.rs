use crate::cir::*;
use crate::compile_ir::{get_index, pos_to_func_idx, OBJECTIVE};
use crate::Datapack;
use std::collections::HashMap;
use std::str::FromStr;

// FIXME: Multiple conditions and a `store success` does not work like I think it does!!!

#[derive(Debug, Clone, PartialEq)]
pub enum InterpError {
    OutOfBoundsAccess(i32, i32, i32),
    MaxCommandsRun,
    EnteredUnreachable,
    EnteredTodo,
    AssertionFailed,
    BreakpointHit,
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
        }
    }
}

impl std::error::Error for InterpError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BreakKind {
    Read,
    Write,
    Access,
}

pub struct Interpreter {
    pub rust_scores: HashMap<ScoreHolder, i32>,
    pub(crate) call_stack: Vec<(usize, usize)>,
    program: Vec<Function>,
    pub memory: [i32; 64 * 16 * 16],
    ptr_pos: (i32, i32, i32),
    turtle_pos: (i32, i32, i32),
    letters: HashMap<(i32, i32, i32), char>,
    next_pos: Option<(usize, usize)>,
    pub output: Vec<String>,
    pub tick: usize,
    commands_run: usize,
    memory_points: HashMap<usize, BreakKind>,
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
            call_stack: vec![(func_idx, 0)],
            memory: [0; 64 * 16 * 16],
            rust_scores: HashMap::new(),
            ptr_pos: (0, 0, 0),
            turtle_pos: (0, 0, 0),
            next_pos: None,
            commands_run: 0,
            tick: 0,
            output: Vec::new(),
            memory_points: HashMap::new(),
            letters,
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

        Interpreter {
            program: datapack.functions,
            call_stack: vec![(start_idx, 0)],
            memory: [0x55_55_55_55; 64 * 16 * 16],
            rust_scores: HashMap::new(),
            ptr_pos: (0, 0, 0),
            turtle_pos: (0, 0, 0),
            next_pos: None,
            tick: 0,
            commands_run: 0,
            output: Vec::new(),
            memory_points: HashMap::new(),
            letters,
        }
    }

    pub fn program(&self) -> &[Function] {
        &self.program
    }

    pub fn call_stack(&self) -> Vec<(&Function, usize)> {
        self.call_stack
            .iter()
            .copied()
            .map(|(f, c)| (&self.program[f], c))
            .collect()
    }

    /// `word_start` is in bytes, must be aligned to a multiple of 4
    pub fn set_mem_breakpoint(&mut self, word_start: usize, kind: BreakKind) {
        assert_eq!(word_start % 4, 0);

        self.memory_points.insert(word_start / 4, kind);
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
            let (func_idx, cmd_idx) = self.call_stack.last().unwrap();
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

    fn execute_cmd(&mut self, cmd: &Command) -> Result<(), InterpError> {
        if !self
            .call_stack
            .iter()
            .any(|(i, _)| self.program[*i].id.name.contains("intrinsic"))
        {
            eprintln!("{}", cmd);
        }

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
                let called_idx = self.program.iter().enumerate().find(|(_, f)| &f.id == id).unwrap_or_else(|| todo!("{:?}", id)).0;
                self.call_stack.push((called_idx, 0));
            }
            Command::Fill(Fill { start, end, block }) => {
                if !(start == "-2 0 0" && end == "-15 0 64" && block == "minecraft:air") {
                    todo!("{:?} {:?} {:?}", start, end, block)
                }
            }
            Command::Data(Data { target, kind }) => {
                match (target, kind) {
                    (DataTarget::Block(block), DataKind::Modify { path, kind: DataModifyKind::Set, source }) => {
                        if path == "RecordItem.tag.Memory" {
                            let DataModifySource::Value(score) = source;

                            if let [x, y, z] = block.split_whitespace().map(|c| c.parse::<i32>().unwrap()).collect::<Vec<_>>()[..] {
                                self.set_word(*score, get_index(x, y, z)? as usize)?;
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
            Command::SetBlock(SetBlock { pos, block, kind: _kind }) => {
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

                if block.starts_with("minecraft:command_block") {
                    // Command block placement
                    println!("Command block placement at {} block {}", pos, block);
                } else if x == 0 && x_rel && y == 1 && y_rel && z == 0 && z_rel {
                    if block == "minecraft:redstone_block" {
                        println!("Branching to self");

                        if let [(func_idx, _)] = &self.call_stack[..] {
                            assert_eq!(self.next_pos, None);
                            self.next_pos = Some((*func_idx as usize, 0));
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

                        assert_eq!(self.next_pos, None);
                        self.next_pos = Some((idx as usize, 0));
                    }
                }
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
                        self.execute_cmd(run)?;
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
                        println!("Dynamic branch to {}", idx);
                        assert_eq!(self.next_pos, None);
                        self.next_pos = Some((idx as usize, 0));
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
                if subcommands.iter().all(|s| matches!(s, ExecuteSubCmd::Condition { .. })) {
                    if subcommands.iter().all(|s| if let ExecuteSubCmd::Condition { is_unless, cond } = s {
                        self.check_cond(*is_unless, cond)
                    } else {
                        unreachable!()
                    }) {
                        self.execute_cmd(run)?;
                    }
                } else if cmd.to_string().starts_with("execute at @e[tag=turtle] run setblock ~ ~ ~") {
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
                    todo!("{}", cmd)
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
                    for (f, c) in self.call_stack.iter() {
                        eprintln!("{}, {}", self.program[*f].id, c);
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
        if self.commands_run >= 60_000 {
            return Err(InterpError::MaxCommandsRun);
        }

        let (top_func_idx, _) = self.call_stack.first().unwrap();
        let top_func = self.program[*top_func_idx].id.to_string();

        let (func_idx, cmd_idx) = self.call_stack.last_mut().unwrap();

        //println!("Function {} at command {}", self.program[*func_idx].id, cmd_idx);

        let cmd = &self.program[*func_idx].cmds[*cmd_idx].clone();
        *cmd_idx += 1;
        self.execute_cmd(cmd)?;

        self.commands_run += 1;

        loop {
            if self.call_stack.is_empty() {
                self.tick += 1;
                if let Some(n) = self.next_pos.take() {
                    eprintln!("\nNow about to execute {}", &self.program[n.0].id);
                    self.call_stack.push(n);
                }
                println!(
                    "Executed {} commands from function '{}'",
                    self.commands_run, top_func,
                );
                self.commands_run = 0;
                break;
            }

            let (func_idx, cmd_idx) = self.call_stack.last().unwrap();

            if self.program[*func_idx].cmds.len() == *cmd_idx {
                self.call_stack.pop();
            } else {
                break;
            }
        }

        Ok(())
    }

    pub fn halted(&self) -> bool {
        self.call_stack.last() == Some(&(0xFFFF_FFFF_FFFF_FFFF, 0)) || self.call_stack.is_empty()
    }
}
