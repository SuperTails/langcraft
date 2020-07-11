use crate::cir::*;
use crate::compile_ir::{OBJECTIVE, get_index};
use crate::intrinsics::INTRINSICS;
use std::collections::HashMap;
use std::convert::TryInto;

pub struct Interpreter {
    pub rust_scores: HashMap<ScoreHolder, i32>,
    call_stack: Vec<(usize, usize)>,
    program: Vec<Function>,
    pub memory: [i32; 4096],
    ptr_pos: (i32, i32, i32),
    turtle_pos: (i32, i32, i32),
    letters: HashMap<(i32, i32, i32), char>,
    next_pos: Option<(usize, usize)>,
    pub output: Vec<String>,
    commands_run: usize,
}

impl Interpreter {
    pub fn new(mut program: Vec<Function>) -> Self {
        let func_idx = program.len() - 1;

        for func in INTRINSICS.iter() {
            program.push(func.clone());
        }

        let mut letters = HashMap::new();
        for (z, letter) in ['A', 'B', 'A', 'A', '[', ']'].iter().enumerate() {
            letters.insert((-16, 16, -(z as i32)), *letter);
        }

        Interpreter {
            program,
            call_stack: vec![(func_idx, 0)],
            memory: [0; 4096],
            rust_scores: HashMap::new(),
            ptr_pos: (0, 0, 0),
            turtle_pos: (0, 0, 0),
            next_pos: None,
            commands_run: 0,
            output: Vec::new(),
            letters,
        }
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
            if obj != "rust" {
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
        let exec = if let Command::Execute(Execute { run: Some(exec), .. }) = cmd {
            &**exec
        } else {
            unreachable!()
        };

        if let Command::ScoreGet(ScoreGet { target: Target::Uuid(target), target_obj }) = exec {
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

    pub fn get_rust_score(&self, holder: &ScoreHolder) -> Result<i32, String> {
        self.rust_scores.get(&holder).copied().ok_or_else(|| format!("read from uninitialized variable {}", holder))
    }

    pub fn check_cond(&self, is_unless: bool, cond: &ExecuteCondition) -> bool {
        let result = match cond {
            ExecuteCondition::Score { target: Target::Uuid(target), target_obj, kind } => {
                if target_obj != OBJECTIVE {
                    todo!("{:?}", target_obj)
                }

                let target = self.get_rust_score(target).unwrap();

                match kind {
                    ExecuteCondKind::Relation { relation, source: Target::Uuid(source), source_obj } => {
                        if source_obj != OBJECTIVE {
                            todo!("{:?}", source_obj)
                        }
                        
                        let source = self.get_rust_score(source).unwrap();

                        match relation {
                            Relation::LessThan => target < source,
                            Relation::LessThanEq => target <= source,
                            Relation::Eq => target == source,
                            Relation::GreaterThan => target > source,
                            Relation::GreaterThanEq => target >= source
                        }
                    }
                    ExecuteCondKind::Matches(m) => {
                        m.contains(target)
                    }
                    _ => todo!("{:?}", kind)
                }
            }
            _ => todo!("{:?}", cond)
        };

        if is_unless {
            !result
        } else {
            result
        }
    }

    fn read_mem(&self) -> i32 {
        let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2);
        self.memory[index as usize / 4]
    }

    fn execute_cmd(&mut self, cmd: &Command) {
        match cmd {
            Command::ScoreAdd(ScoreAdd { target: Target::Uuid(target), target_obj, score }) => {
                if target_obj != OBJECTIVE {
                    todo!("{:?}", target_obj)
                }

                let mut lhs = self.get_rust_score(target).unwrap();
                lhs += *score;
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
                                val *= rhs;
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::DivAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                val /= rhs;
                                self.rust_scores.insert(target.clone(), val);
                            }
                            ScoreOpKind::ModAssign => {
                                let mut val = self.get_rust_score(target).unwrap();
                                if val < 0 {
                                    todo!("DETERMINE BEHAVIOR")
                                }
                                val %= rhs;
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
            Command::Comment(c) => {
                println!("# {}", c)
            }
            Command::FuncCall(FuncCall { id }) => {
                let called_idx = self.program.iter().enumerate().find(|(_, f)| &f.id == id).unwrap_or_else(|| todo!("{:?}", id)).0;
                self.call_stack.push((called_idx, 0));
            }
            Command::Fill(Fill { start, end, block }) => {
                if !(start == "-2 0 0" && end == "-2 0 150" && block == "minecraft:air") {
                    todo!("{:?} {:?} {:?}", start, end, block)
                }
            }
            Command::Data(Data { target, kind }) => {
                match (target, kind) {
                    (DataTarget::Block(block), DataKind::Modify { path, kind: DataModifyKind::Set, source }) => {
                        if path == "RecordItem.tag.Memory" {
                            let DataModifySource::Value(score) = source;

                            if let [x, y, z] = block.split_whitespace().map(|c| c.parse::<i32>().unwrap()).collect::<Vec<_>>()[..] {
                                self.memory[get_index(x, y, z) as usize / 4] = *score;
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
                if pos.starts_with("-2 1 ") && block == "minecraft:redstone_block" {
                    let z = pos["-2 1 ".len()..].parse::<i32>().unwrap();

                    println!("Branching to {}", self.program[z as usize].id);

                    assert_eq!(self.next_pos, None);
                    self.next_pos = Some((z.try_into().unwrap(), 0));
                } else if pos.starts_with("-2 0 ") && block.starts_with("minecraft:command_block") {
                    // Command block placement
                    println!("Command block placement at {} block {}", pos, block);
                } else if pos.starts_with("~ 1 ") && block == "minecraft:redstone_block" {
                    let z = pos["~ 1 ".len()..].parse::<i32>().unwrap();

                    println!("Branching to {}", self.program[z as usize].id);

                    assert_eq!(self.next_pos, None);
                    self.next_pos = Some((z.try_into().unwrap(), 0));
                } else if pos.starts_with("~ ~1 ") && block == "minecraft:redstone_block" {
                    let z = pos["~ ~1 ".len()..].parse::<i32>().unwrap();

                    println!("Branching to {}", self.program[z as usize].id);

                    assert_eq!(self.next_pos, None);
                    self.next_pos = Some((z.try_into().unwrap(), 0));
                } else if pos == "~ ~1 ~" && block == "minecraft:air" {
                    // Clearing command block activation
                } else {
                    todo!("{} {}", pos, block)
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
                        self.execute_cmd(run);
                    }
                } else {
                    unreachable!()
                }
            }
            cmd if cmd.to_string() == "execute at @e[tag=ptr] run setblock ~ ~ ~ minecraft:redstone_block replace" => {
                if self.ptr_pos.0 == -2 || self.ptr_pos.1 == 1 {
                    println!("Return to {}", self.ptr_pos.2);
                    assert_eq!(self.next_pos, None);
                    self.next_pos = Some((self.ptr_pos.2 as usize, 0));
                } else {
                    panic!("attempt to return improperly")
                }
            }
            cmd if cmd.to_string().starts_with("execute at @e[tag=ptr] run") => {
                let data = if let Command::Execute(Execute { run: Some(d), .. }) = cmd {
                    &**d
                } else {
                    unreachable!()
                };

                if let Command::Data(Data { target: DataTarget::Block(block), kind: DataKind::Modify { path, kind: DataModifyKind::Set, source: DataModifySource::Value(v) } }) = data {
                    if block != "~ ~ ~" {
                        todo!("{:?}", block);
                    }

                    if path != "RecordItem.tag.Memory" {
                        todo!("{:?}", path);
                    }

                    let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2);

                    self.memory[index as usize / 4] = *v;
                } else {
                    todo!("{:?}", data)
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
                        let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2);
                        self.rust_scores.insert(target.clone(), self.memory[index as usize / 4]);
                    }
                } else if subcmds[1].to_string() == "store result block ~ ~ ~ RecordItem.tag.Memory int 1" {
                    if let Command::ScoreGet(ScoreGet { target: Target::Uuid(target), target_obj }) = &**run {
                        if target_obj != OBJECTIVE {
                            todo!("{:?}", target_obj)
                        }

                        let val = *self.rust_scores.get(target).unwrap_or_else(|| panic!("read from uninitialized variable {}", target));
                        let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2);
                        self.memory[index as usize / 4] = val;
                    }
                } else {
                    todo!("{:?} {}", subcmds[1].to_string(), cmd)
                }
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] store result entity @s Pos[0] double 1 run scoreboard players get %x rust" => {
                let val = *self.rust_scores.get(&ScoreHolder::new("%x".into()).unwrap()).unwrap_or_else(|| panic!("read from uninitialized variable %x"));
                self.ptr_pos.0 = val;
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] store result entity @s Pos[1] double 1 run scoreboard players get %y rust" => {
                let val = *self.rust_scores.get(&ScoreHolder::new("%y".into()).unwrap()).unwrap_or_else(|| panic!("read from uninitialized variable %y"));
                self.ptr_pos.1 = val;
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] store result entity @s Pos[2] double 1 run scoreboard players get %z rust" => {
                let val = *self.rust_scores.get(&ScoreHolder::new("%z".into()).unwrap()).unwrap_or_else(|| panic!("read from uninitialized variable %z"));
                self.ptr_pos.2 = val;
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] at @s store result entity @s Pos[2] double 1 run data get block ~ ~ ~ RecordItem.tag.Memory 1" => {
                self.ptr_pos.2 = self.read_mem();
            }
            cmd if cmd.to_string() == "execute as @e[tag=ptr] at @s run tp @s -2 1 ~" => {
                self.ptr_pos.0 = -2;
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
                    let index = get_index(self.ptr_pos.0, self.ptr_pos.1, self.ptr_pos.2);
                    self.memory[index as usize / 4] = val;
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
                        self.execute_cmd(run);
                    }
                } else {
                    todo!("{:?}", subcommands)
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

                if let ExecuteSubCmd::Condition { is_unless, cond } = &subcommands[1] {
                    let value = self.check_cond(*is_unless, cond) as i32;

                    self.rust_scores.insert(store_target.clone(), value);
                } else {
                    todo!("{:?}", subcommands[1])
                };
            }
            cmd => todo!("{}", cmd)

        }
    }

    pub fn step(&mut self) {
        let (func_idx, cmd_idx) = self.call_stack.last_mut().unwrap();
        
        //println!("Function {} at command {}", self.program[*func_idx].id, cmd_idx);

        let cmd = &self.program[*func_idx].cmds[*cmd_idx].clone();
        *cmd_idx += 1;
        self.execute_cmd(cmd);

        self.commands_run += 1;

        loop {
            if self.call_stack.is_empty() {
                self.call_stack.push(self.next_pos.take().unwrap());
                println!("Executed {} commands from function 'TODO:'", self.commands_run);
                self.commands_run = 0;
                break;
            }

            let (func_idx, cmd_idx) = self.call_stack.last().unwrap();

            if self.program[*func_idx].cmds.len() == *cmd_idx {
                //println!("Returning");
                self.call_stack.pop();
            } else {
                break
            }
        }
    }

    pub fn halted(&self) -> bool {
        self.call_stack.last() == Some(&(0xFFFF_FFFF_FFFF_FFFF, 0))
    }
}