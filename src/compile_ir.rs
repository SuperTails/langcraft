use llvm_ir::{Module, Function, Terminator, Operand, Constant, Instruction, Name, IntPredicate};
use llvm_ir::terminator::{Ret, Br, CondBr};
use llvm_ir::instruction::{Store, Load, Add, Mul, ICmp};
use crate::cir::Function as McFunction;
use crate::cir::FuncCall as McFuncCall;
use crate::cir::{self, ScoreSet, ScoreOp, ScoreOpKind, Target, Command, Execute, ExecuteSubCmd, ExecuteCondition, ExecuteStoreKind, ExecuteCondKind};
use std::sync::Mutex;
use lazy_static::lazy_static;

pub const OBJECTIVE: &str = "rust";

pub fn compile_module(module: &Module) -> Vec<McFunction> {
    let init_cmds = module.global_vars.iter().map(|v| {
        let score = if let Some(Constant::Int { bits: 32, value }) = &v.initializer {
            *value as i32
        } else {
            todo!("constant {:?}", v.initializer)
        };

        let temp = v.name.to_string();
        let target = format!("%@{}", &temp[1..temp.len() - 1]);

        Command::from(ScoreSet {
            target: Target::Uuid(target),
            target_obj: OBJECTIVE.to_string(),
            score,
        })
    }).collect();

    std::iter::once(McFunction { name: "init".to_string(), cmds: init_cmds }).chain(module.functions.iter().flat_map(|f| compile_function(f))).collect()
}

pub fn mc_block_name(func_name: &str, block_name: &Name) -> String {
    match block_name {
        Name::Name(s) => s.clone(),
        Name::Number(n) => format!("{}-block{}", func_name, n),
    }
}

pub fn compile_function(func: &Function) -> Vec<McFunction> {
    if !func.parameters.is_empty() {
        todo!("functions with parameters");
    }

    if func.is_var_arg {
        todo!("functions with variadic arguments");
    }

    if func.basic_blocks.is_empty() {
        todo!("functions with no basic blocks");
    }

    func.basic_blocks.iter().enumerate().map(|(_, block)| {
        let name = mc_block_name(&func.name, &block.name);

        let mut this = McFunction { name, cmds: vec![] };
        
        for instr in block.instrs.iter() {
            this.cmds.extend(compile_instr(instr));
        }

        match &block.term {
            Terminator::Ret(Ret { return_operand: None, .. }) => {},
            Terminator::Ret(Ret { return_operand: Some(operand), .. }) => {
                let (cmds, source) = eval_operand(operand);
                this.cmds.extend(cmds);
                this.cmds.push(ScoreOp {
                    target: Target::Uuid("%return".to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(source),
                    source_obj: OBJECTIVE.to_string(),
                }.into());
            },
            Terminator::Br(Br { dest, .. }) => {
                this.cmds.push(McFuncCall {
                    name: mc_block_name(&func.name, dest)
                }.into());
            }
            Terminator::CondBr(CondBr { condition, true_dest, false_dest, .. }) => {
                let (cmds, cond) = eval_operand(condition);
                this.cmds.extend(cmds);

                let true_dest = mc_block_name(&func.name, true_dest);
                let false_dest = mc_block_name(&func.name, false_dest);

                this.cmds.push(Execute {
                    subcommands: vec![
                        ExecuteSubCmd::Condition {
                            is_unless: false,
                            cond: ExecuteCondition::Score {
                                target: Target::Uuid(cond.clone()),
                                target_obj: OBJECTIVE.to_string(),
                                kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                            }
                        }
                    ],
                    run: Some(Box::new(
                        McFuncCall { name: true_dest }.into()
                    ))
                }.into());

                this.cmds.push(Execute {
                    subcommands: vec![
                        ExecuteSubCmd::Condition {
                            is_unless: true,
                            cond: ExecuteCondition::Score {
                                target: Target::Uuid(cond),
                                target_obj: OBJECTIVE.to_string(),
                                kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                            }
                        }
                    ],
                    run: Some(Box::new(
                        McFuncCall { name: false_dest }.into()
                    ))
                }.into());

            }
            term => todo!("terminator {:?}", term),
        }

        this
    }).collect()
}

pub fn compile_instr(instr: &Instruction) -> Vec<Command> {
    match instr {
        Instruction::Alloca(_) => { eprintln!("FIXME: ACTUALLY DO AN ALLOCA"); vec![] },
        Instruction::Store(Store { address, value, .. }) => {
            if let Operand::LocalOperand { name, ty } = address {
                let (mut cmds, source) = eval_operand(value);
                cmds.push(ScoreOp {
                    target: Target::Uuid(name.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(source),
                    source_obj: OBJECTIVE.to_string(),
                }.into());
                cmds
            } else {
                todo!("{:?}", address)
            }
        },
        Instruction::Load(Load { dest, address, .. }) => {
            let (mut cmds, source) = eval_operand(address);
            cmds.push(ScoreOp {
                target: Target::Uuid(dest.to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::Assign,
                source: Target::Uuid(source),
                source_obj: OBJECTIVE.to_string(),
            }.into());
            cmds
        }
        Instruction::Add(Add { operand0, operand1, dest, .. }) => {
            let (mut cmds, source0) = eval_operand(operand0);
            let (tmp, source1) = eval_operand(operand1);
            cmds.extend(tmp.into_iter());
            cmds.push(ScoreOp {
                target: Target::Uuid(dest.to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::Assign,
                source: Target::Uuid(source0),
                source_obj: OBJECTIVE.to_string(),
            }.into());
            cmds.push(ScoreOp {
                target: Target::Uuid(dest.to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::AddAssign,
                source: Target::Uuid(source1),
                source_obj: OBJECTIVE.to_string(),
            }.into());
            cmds
        }
        // TODO: Refactor, because this is *identical* to Add except for the line
        Instruction::Mul(Mul { operand0, operand1, dest, .. }) => {
            let (mut cmds, source0) = eval_operand(operand0);
            let (tmp, source1) = eval_operand(operand1);
            cmds.extend(tmp.into_iter());
            cmds.push(ScoreOp {
                target: Target::Uuid(dest.to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::Assign,
                source: Target::Uuid(source0),
                source_obj: OBJECTIVE.to_string(),
            }.into());
            cmds.push(ScoreOp {
                target: Target::Uuid(dest.to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::MulAssign,
                source: Target::Uuid(source1),
                source_obj: OBJECTIVE.to_string(),
            }.into());
            cmds
        }
        Instruction::ICmp(ICmp { predicate, operand0, operand1, dest, .. }) => {
            let relation = match predicate {
                IntPredicate::SGE => cir::Relation::GreaterThanEq,
                IntPredicate::SGT => cir::Relation::GreaterThan,
                IntPredicate::SLT => cir::Relation::LessThan,
                IntPredicate::SLE => cir::Relation::LessThanEq,
                IntPredicate::EQ => cir::Relation::Eq,
                p => todo!("predicate {:?}", p),
            };

            let (mut cmds, target) = eval_operand(operand0);
            let (tmp_cmds, source) = eval_operand(operand1);
            cmds.extend(tmp_cmds);
            cmds.push(Execute {
                subcommands: vec![
                    ExecuteSubCmd::Store {
                        is_success: true,
                        kind: ExecuteStoreKind::Score {
                            target: Target::Uuid(dest.to_string()),
                            objective: OBJECTIVE.to_string(),
                        }
                    },
                    ExecuteSubCmd::Condition {
                        is_unless: false,
                        cond: ExecuteCondition::Score {
                            target: Target::Uuid(target),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ExecuteCondKind::Relation {
                                relation,
                                source: Target::Uuid(source),
                                source_obj: OBJECTIVE.to_string(),
                            }
                        }
                    }
                ],
                run: None,
            }.into());
            cmds
        }
        _ => todo!("instruction {:?}", instr)
    }
}

pub fn eval_operand(op: &Operand) -> (Vec<Command>, String) {
    match op {
        Operand::LocalOperand { name, ty } => {
            (vec![], name.to_string())  
        },
        Operand::ConstantOperand(Constant::GlobalReference { name, ty }) => {
            let temp = name.to_string();
            (vec![], format!("%@{}", &temp[1..temp.len() - 1]))
        },
        Operand::ConstantOperand(Constant::Int { bits: 32, value }) => {
            let target = format!("%temp{}", get_unique_num());
            (vec![ScoreSet {
                target: Target::Uuid(target.clone()),
                target_obj: OBJECTIVE.to_string(),
                score: *value as i32,
            }.into()], target)
        }
        _ => todo!("operand {:?}", op),
    }
}

lazy_static! {
    pub static ref TEMP_CNT: Mutex<u32> = Mutex::new(0);
}

fn get_unique_num() -> u32 {
    let mut lock = TEMP_CNT.lock().unwrap();
    let result = *lock;
    *lock += 1;
    result
}

