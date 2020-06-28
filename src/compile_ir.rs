use crate::cir::FuncCall as McFuncCall;
use crate::cir::Function as McFunction;
use crate::cir::{
    self, Command, Data, DataKind, DataTarget, Execute, ExecuteCondKind, ExecuteCondition,
    ExecuteStoreKind, ExecuteSubCmd, ScoreGet, ScoreOp, ScoreOpKind, ScoreSet, Target,
};
use lazy_static::lazy_static;
use llvm_ir::instruction::{Add, Alloca, GetElementPtr, ICmp, Load, Mul, Store};
use llvm_ir::terminator::{Br, CondBr, Ret};
use llvm_ir::{
    Constant, Function, Instruction, IntPredicate, Module, Name, Operand, Terminator, Type,
};
use std::sync::Mutex;

pub const OBJECTIVE: &str = "rust";

// `intrinsic:setptr` sets the pointer to the value in `%ptr` for objective `rust`

// reads the current pointer location into some target for objective `rust`
pub fn read_ptr(target: String) -> Command {
    let mut exec = Execute::new();
    exec.with_subcmd(ExecuteSubCmd::At {
        target: Target::Selector(cir::Selector {
            var: cir::SelectorVariable::AllEntities,
            args: vec![cir::SelectorArg("type=armor_stand".to_string())],
        }),
    });
    exec.with_subcmd(ExecuteSubCmd::Store {
        is_success: false,
        kind: ExecuteStoreKind::Score {
            target: Target::Uuid(target),
            objective: OBJECTIVE.to_string(),
        },
    });
    exec.with_run(
        Data {
            target: DataTarget::Block("~ ~ ~".to_string()),
            kind: DataKind::Get {
                path: "RecordItem.tag.Memory".to_string(),
                scale: 1.0,
            },
        }
        .into(),
    );

    exec.into()
}

pub fn write_ptr(target: String) -> Command {
    let mut exec = Execute::new();
    exec.with_subcmd(ExecuteSubCmd::At {
        target: Target::Selector(cir::Selector {
            var: cir::SelectorVariable::AllEntities,
            args: vec![cir::SelectorArg("type=armor_stand".to_string())],
        }),
    });
    exec.with_subcmd(ExecuteSubCmd::Store {
        is_success: false,
        kind: ExecuteStoreKind::Data {
            target: DataTarget::Block("~ ~ ~".to_string()),
            path: "RecordItem.tag.Memory".to_string(),
            ty: "int".to_string(),
            scale: 1.0,
        },
    });
    exec.with_run(
        ScoreGet {
            target: Target::Uuid(target),
            target_obj: OBJECTIVE.to_string(),
        }
        .into(),
    );

    exec.into()
}

pub fn compile_module(module: &Module) -> Vec<McFunction> {
    let init_cmds = module
        .global_vars
        .iter()
        .flat_map(|v| {
            let temp = v.name.to_string();
            let target = format!("%@{}", &temp[1..temp.len() - 1]);

            match &v.initializer {
                Some(Constant::Int { bits: 32, value }) => vec![Command::from(ScoreSet {
                    target: Target::Uuid(target),
                    target_obj: OBJECTIVE.to_string(),
                    score: *value as i32,
                })],
                Some(Constant::Array {
                    element_type: Type::IntegerType { bits: 32 },
                    elements,
                }) => {
                    let start = get_alloc(elements.len() as u32);

                    let mut cmds = vec![ScoreSet {
                        target: Target::Uuid(target),
                        target_obj: OBJECTIVE.to_string(),
                        score: start as i32,
                    }
                    .into()];

                    for (idx, elem) in elements.iter().enumerate() {
                        let score = if let Constant::Int { bits: 32, value } = elem {
                            *value as i32
                        } else {
                            todo!("{:?}", elem);
                        };

                        cmds.push(
                            ScoreSet {
                                target: Target::Uuid("%ptr".to_string()),
                                target_obj: OBJECTIVE.to_string(),
                                score: start as i32 + idx as i32,
                            }
                            .into(),
                        );
                        cmds.push(
                            McFuncCall {
                                name: "intrinsic:setptr".to_string(),
                            }
                            .into(),
                        );
                        cmds.push(
                            ScoreSet {
                                target: Target::Uuid("%temp".to_string()),
                                target_obj: OBJECTIVE.to_string(),
                                score,
                            }
                            .into(),
                        );
                        cmds.push(write_ptr("%temp".to_string()));
                    }

                    cmds
                }
                _ => todo!("constant {:?}", v.initializer),
            }
        })
        .collect();

    std::iter::once(McFunction {
        name: "init".to_string(),
        cmds: init_cmds,
    })
    .chain(module.functions.iter().flat_map(|f| compile_function(f)))
    .collect()
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

    func.basic_blocks
        .iter()
        .enumerate()
        .map(|(_, block)| {
            let name = mc_block_name(&func.name, &block.name);

            let mut this = McFunction { name, cmds: vec![] };

            for instr in block.instrs.iter() {
                this.cmds.extend(compile_instr(instr));
            }

            match &block.term {
                Terminator::Ret(Ret {
                    return_operand: None,
                    ..
                }) => {}
                Terminator::Ret(Ret {
                    return_operand: Some(operand),
                    ..
                }) => {
                    let (cmds, source) = eval_operand(operand);
                    this.cmds.extend(cmds);
                    this.cmds.push(
                        ScoreOp {
                            target: Target::Uuid("%return".to_string()),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ScoreOpKind::Assign,
                            source: Target::Uuid(source),
                            source_obj: OBJECTIVE.to_string(),
                        }
                        .into(),
                    );
                }
                Terminator::Br(Br { dest, .. }) => {
                    this.cmds.push(
                        McFuncCall {
                            name: mc_block_name(&func.name, dest),
                        }
                        .into(),
                    );
                }
                Terminator::CondBr(CondBr {
                    condition,
                    true_dest,
                    false_dest,
                    ..
                }) => {
                    let (cmds, cond) = eval_operand(condition);
                    this.cmds.extend(cmds);

                    let true_dest = mc_block_name(&func.name, true_dest);
                    let false_dest = mc_block_name(&func.name, false_dest);

                    this.cmds.push(
                        Execute {
                            subcommands: vec![ExecuteSubCmd::Condition {
                                is_unless: false,
                                cond: ExecuteCondition::Score {
                                    target: Target::Uuid(cond.clone()),
                                    target_obj: OBJECTIVE.to_string(),
                                    kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                                },
                            }],
                            run: Some(Box::new(McFuncCall { name: true_dest }.into())),
                        }
                        .into(),
                    );

                    this.cmds.push(
                        Execute {
                            subcommands: vec![ExecuteSubCmd::Condition {
                                is_unless: true,
                                cond: ExecuteCondition::Score {
                                    target: Target::Uuid(cond),
                                    target_obj: OBJECTIVE.to_string(),
                                    kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                                },
                            }],
                            run: Some(Box::new(McFuncCall { name: false_dest }.into())),
                        }
                        .into(),
                    );
                }
                term => todo!("terminator {:?}", term),
            }

            this
        })
        .collect()
}

pub fn compile_instr(instr: &Instruction) -> Vec<Command> {
    match instr {
        // TODO: Implement a proper stack pointer
        Instruction::Alloca(Alloca {
            allocated_type: Type::IntegerType { bits: 32 },
            num_elements,
            dest,
            ..
        }) => {
            let num = if let Operand::ConstantOperand(Constant::Int { bits: 32, value: 1 }) =
                num_elements
            {
                1
            } else {
                todo!("{:?}", num_elements);
            };

            vec![ScoreSet {
                target: Target::Uuid(dest.to_string()),
                target_obj: OBJECTIVE.to_string(),
                score: get_alloc(num) as i32,
            }
            .into()]
        }
        Instruction::GetElementPtr(GetElementPtr {
            address,
            indices,
            in_bounds: true,
            dest,
            ..
        }) => {
            assert_eq!(indices.len(), 2);

            assert!(matches!(
                indices[0],
                Operand::ConstantOperand(Constant::Int { value: 0, .. })
            ));

            let (mut cmds, address) = eval_operand(address);
            let (tmp, source) = eval_operand(&indices[1]);
            cmds.extend(tmp);

            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(address),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::AddAssign,
                    source: Target::Uuid(source),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds
        }
        Instruction::Store(Store { address, value, .. }) => {
            let (mut cmds, addr) = eval_operand(address);
            let (tmp, source) = eval_operand(value);
            cmds.extend(tmp);

            cmds.push(
                ScoreOp {
                    target: Target::Uuid("%ptr".to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(addr),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                McFuncCall {
                    name: "intrinsic:setptr".into(),
                }
                .into(),
            );
            cmds.push(write_ptr(source));
            cmds
        }
        Instruction::Load(Load { dest, address, .. }) => {
            let (mut cmds, source) = eval_operand(address);
            cmds.push(
                ScoreOp {
                    target: Target::Uuid("%ptr".to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(source),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                McFuncCall {
                    name: "intrinsic:setptr".into(),
                }
                .into(),
            );
            cmds.push(read_ptr(dest.to_string()));
            cmds
        }
        Instruction::Add(Add {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            let (mut cmds, source0) = eval_operand(operand0);
            let (tmp, source1) = eval_operand(operand1);
            cmds.extend(tmp.into_iter());
            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(source0),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::AddAssign,
                    source: Target::Uuid(source1),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds
        }
        // TODO: Refactor, because this is *identical* to Add except for the line
        Instruction::Mul(Mul {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            let (mut cmds, source0) = eval_operand(operand0);
            let (tmp, source1) = eval_operand(operand1);
            cmds.extend(tmp.into_iter());
            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid(source0),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::MulAssign,
                    source: Target::Uuid(source1),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds
        }
        Instruction::ICmp(ICmp {
            predicate,
            operand0,
            operand1,
            dest,
            ..
        }) => {
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
            cmds.push(
                Execute {
                    subcommands: vec![
                        ExecuteSubCmd::Store {
                            is_success: true,
                            kind: ExecuteStoreKind::Score {
                                target: Target::Uuid(dest.to_string()),
                                objective: OBJECTIVE.to_string(),
                            },
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
                                },
                            },
                        },
                    ],
                    run: None,
                }
                .into(),
            );
            cmds
        }
        _ => todo!("instruction {:?}", instr),
    }
}

pub fn eval_operand(op: &Operand) -> (Vec<Command>, String) {
    match op {
        Operand::LocalOperand { name, ty } => (vec![], name.to_string()),
        Operand::ConstantOperand(Constant::GlobalReference { name, ty }) => {
            let temp = name.to_string();
            (vec![], format!("%@{}", &temp[1..temp.len() - 1]))
        }
        Operand::ConstantOperand(Constant::Int { bits: 32, value }) => {
            let target = format!("%temp{}", get_unique_num());
            (
                vec![ScoreSet {
                    target: Target::Uuid(target.clone()),
                    target_obj: OBJECTIVE.to_string(),
                    score: *value as i32,
                }
                .into()],
                target,
            )
        }
        _ => todo!("operand {:?}", op),
    }
}

lazy_static! {
    pub static ref TEMP_CNT: Mutex<u32> = Mutex::new(0);
    pub static ref FREE_PTR: Mutex<u32> = Mutex::new(4);
}

fn get_alloc(amount: u32) -> u32 {
    let mut lock = FREE_PTR.lock().unwrap();
    let result = *lock;
    *lock += amount;
    result
}

fn get_unique_num() -> u32 {
    let mut lock = TEMP_CNT.lock().unwrap();
    let result = *lock;
    *lock += 1;
    result
}
