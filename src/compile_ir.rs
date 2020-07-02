use crate::cir::FuncCall as McFuncCall;
use crate::cir::Function as McFunction;
use crate::cir::{
    self, Command, Data, DataKind, DataTarget, Execute, ExecuteCondKind, ExecuteCondition,
    ExecuteStoreKind, ExecuteSubCmd, ScoreGet, ScoreOp, ScoreOpKind, ScoreSet, SetBlock,
    SetBlockKind, Target, Tellraw,
};
use either::Either;
use lazy_static::lazy_static;
use llvm_ir::instruction::{Add, Alloca, Call, GetElementPtr, ICmp, Load, Mul, Store};
use llvm_ir::module::GlobalVariable;
use llvm_ir::terminator::{Br, CondBr, Ret, Switch};
use llvm_ir::{
    Constant, Function, Instruction, IntPredicate, Module, Name, Operand, Terminator, Type,
};
use std::convert::TryFrom;
use std::sync::Mutex;

pub const OBJECTIVE: &str = "rust";

// `intrinsic:setptr` sets the pointer to the value in `%ptr` for objective `rust`

// reads the current pointer location into some target for objective `rust`
pub fn read_ptr(target: String) -> Command {
    let mut exec = Execute::new();
    exec.with_at(Target::Selector(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=ptr".to_string())],
    }));
    exec.with_subcmd(ExecuteSubCmd::Store {
        is_success: false,
        kind: ExecuteStoreKind::Score {
            target: Target::Uuid(target),
            objective: OBJECTIVE.to_string(),
        },
    });
    exec.with_run(Data {
        target: DataTarget::Block("~ ~ ~".to_string()),
        kind: DataKind::Get {
            path: "RecordItem.tag.Memory".to_string(),
            scale: 1.0,
        },
    });

    exec.into()
}

/// Returns xyz
pub fn get_address(mut address: i32) -> (i32, i32, i32) {
    assert!(0 < address);
    assert!(address < 16 * 16 * 16);
    let z = address % 16;
    address /= 16;
    let y = address % 16;
    address /= 16;
    let x = address % 16;
    (x, y, z)
}

/// Optimized form of setting and then writing to the pointer
/// when the address and value are known at compile time
pub fn set_memory(value: i32, address: i32) -> Command {
    let (x, y, z) = get_address(address);

    Data {
        target: DataTarget::Block(format!("{} {} {}", x, y, z)),
        kind: DataKind::Modify {
            path: "RecordItem.tag.Memory".to_string(),
            kind: cir::DataModifyKind::Set,
            source: cir::DataModifySource::Value(value),
        },
    }
    .into()
}

// TODO: Technically this can support other datatypes too, since it's stored in a block
/// Shorthand for `write_ptr` when the operand is a constant i32
pub fn write_ptr_const(value: i32) -> Command {
    let mut exec = Execute::new();
    exec.with_at(Target::Selector(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=ptr".to_string())],
    }));
    exec.with_run(Data {
        target: DataTarget::Block("~ ~ ~".to_string()),
        kind: DataKind::Modify {
            path: "RecordItem.tag.Memory".to_string(),
            kind: cir::DataModifyKind::Set,
            source: cir::DataModifySource::Value(value),
        },
    });
    exec.into()
}

/// Reads the score in the given `target` and writes to the current memory location
pub fn write_ptr(target: String) -> Command {
    let mut exec = Execute::new();
    exec.with_at(Target::Selector(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=ptr".to_string())],
    }));
    exec.with_subcmd(ExecuteSubCmd::Store {
        is_success: false,
        kind: ExecuteStoreKind::Data {
            target: DataTarget::Block("~ ~ ~".to_string()),
            path: "RecordItem.tag.Memory".to_string(),
            ty: "int".to_string(),
            scale: 1.0,
        },
    });
    exec.with_run(ScoreGet {
        target: Target::Uuid(target),
        target_obj: OBJECTIVE.to_string(),
    });

    exec.into()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    // FIXME: It is actually *not correct* to directly terminate with a call!
    // And, on the other hand, a Call instruction MUST be a call!
    direct_term: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { direct_term: false }
    }
}

pub fn compile_module(module: &Module, options: &Options) -> Vec<McFunction> {
    let init_cmds = module
        .global_vars
        .iter()
        .flat_map(compile_global_var_init)
        .collect();

    let mut funcs = std::iter::once(McFunction {
        name: "init".to_string(),
        cmds: init_cmds,
    })
    .chain(
        module
            .functions
            .iter()
            .flat_map(|f| compile_function(f, options)),
    )
    .collect::<Vec<McFunction>>();

    for func_idx in 0..funcs.len() {
        for cmd_idx in 0..funcs[func_idx].cmds.len() {
            if let Command::FuncCall(McFuncCall { name }) = &mut funcs[func_idx].cmds[cmd_idx] {
                // TODO: `strip_suffix` is nightly but it's exactly what I'm doing
                if name.ends_with("%%FIXUP") {
                    let mut name = std::mem::take(name);
                    name.truncate(name.len() - "%%FIXUP".len());

                    let idx = funcs
                        .iter()
                        .enumerate()
                        .find(|(_, f)| f.name == name)
                        .unwrap()
                        .0;

                    let pos = format!("~ 1 {}", idx);
                    let block = "minecraft:redstone_block".to_string();

                    funcs[func_idx].cmds[cmd_idx] = SetBlock {
                        pos,
                        block,
                        kind: SetBlockKind::Destroy,
                    }
                    .into();
                }
            } else if let Command::Execute(Execute {
                run: Some(func_call),
                ..
            }) = &mut funcs[func_idx].cmds[cmd_idx]
            {
                if let Command::FuncCall(McFuncCall { name }) = &mut **func_call {
                    if name.ends_with("%%FIXUP") {
                        let mut name = std::mem::take(name);
                        name.truncate(name.len() - "%%FIXUP".len());

                        let idx = funcs
                            .iter()
                            .enumerate()
                            .find(|(_, f)| f.name == name)
                            .unwrap_or_else(|| panic!("could not find {:?}", name))
                            .0;

                        let pos = format!("~ ~1 {}", idx);
                        let block = "minecraft:redstone_block".to_string();

                        if let Command::Execute(Execute { run: Some(run), .. }) =
                            &mut funcs[func_idx].cmds[cmd_idx]
                        {
                            *run = Box::new(
                                SetBlock {
                                    pos,
                                    block,
                                    kind: SetBlockKind::Replace,
                                }
                                .into(),
                            );
                        } else {
                            unreachable!()
                        }
                    }
                }
            }
        }
    }

    if !options.direct_term {
        let build_cmds = funcs
            .iter()
            .enumerate()
            .map(|(idx, func)| {
                let pos = format!("-2 0 {}", idx);
                let block = format!(
                    "minecraft:command_block{{Command:\"function rust:{}\"}}",
                    func.name
                );

                SetBlock {
                    pos,
                    block,
                    kind: SetBlockKind::Destroy,
                }
                .into()
            })
            .collect::<Vec<Command>>();

        funcs[0].cmds.extend(build_cmds);
    }

    funcs
}

fn compile_global_var_init(v: &GlobalVariable) -> Vec<Command> {
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

                cmds.push(set_memory(score, start as i32 + idx as i32));
            }

            cmds
        }
        _ => todo!("constant {:?}", v.initializer),
    }
}

pub fn mc_block_name(func_name: &str, block_name: &Name) -> String {
    match block_name {
        Name::Name(s) => s.clone(),
        Name::Number(n) => format!("{}-block{}", func_name, n),
    }
}

#[repr(i32)]
#[derive(Debug, PartialEq, PartialOrd, Clone)]
enum McBlock {
    Air,
    Cobblestone,
    Granite,
    Andesite,
    Diorite,
    LapisBlock,
    IronBlock,
    GoldBlock,
    DiamondBlock,
    RedstoneBlock,
}

impl std::fmt::Display for McBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "minecraft:")?;

        match self {
            McBlock::Air => write!(f, "air"),
            McBlock::Cobblestone => write!(f, "cobblestone"),
            McBlock::Granite => write!(f, "granite"),
            McBlock::Andesite => write!(f, "andesite"),
            McBlock::Diorite => write!(f, "diorite"),
            McBlock::LapisBlock => write!(f, "lapis_block"),
            McBlock::IronBlock => write!(f, "iron_block"),
            McBlock::GoldBlock => write!(f, "gold_block"),
            McBlock::DiamondBlock => write!(f, "diamond_block"),
            McBlock::RedstoneBlock => write!(f, "redstone_block"),
        }
    }
}

impl TryFrom<i32> for McBlock {
    type Error = ();

    fn try_from(val: i32) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(McBlock::Air),
            1 => Ok(McBlock::Cobblestone),
            2 => Ok(McBlock::Granite),
            3 => Ok(McBlock::Andesite),
            4 => Ok(McBlock::Diorite),
            5 => Ok(McBlock::LapisBlock),
            6 => Ok(McBlock::IronBlock),
            7 => Ok(McBlock::GoldBlock),
            8 => Ok(McBlock::DiamondBlock),
            9 => Ok(McBlock::RedstoneBlock),
            _ => Err(()),
        }
    }
}

pub fn compile_function(func: &Function, options: &Options) -> Vec<McFunction> {
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

            if !options.direct_term {
                this.cmds.push(
                    SetBlock {
                        pos: "~ ~1 ~".to_string(),
                        block: "minecraft:air".to_string(),
                        kind: SetBlockKind::Replace,
                    }
                    .into(),
                );
            }

            for instr in block.instrs.iter() {
                this.cmds.extend(compile_instr(instr, options));
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
                    let mut name = mc_block_name(&func.name, dest);

                    if !options.direct_term {
                        name.push_str("%%FIXUP");
                    }

                    this.cmds.push(McFuncCall { name }.into());
                }
                Terminator::CondBr(CondBr {
                    condition,
                    true_dest,
                    false_dest,
                    ..
                }) => {
                    let (cmds, cond) = eval_operand(condition);
                    this.cmds.extend(cmds);

                    let mut true_dest = mc_block_name(&func.name, true_dest);
                    let mut false_dest = mc_block_name(&func.name, false_dest);

                    if !options.direct_term {
                        true_dest.push_str("%%FIXUP");
                        false_dest.push_str("%%FIXUP");
                    }

                    let mut true_cmd = Execute::new();
                    true_cmd
                        .with_if(ExecuteCondition::Score {
                            target: Target::Uuid(cond.clone()),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                        })
                        .with_run(McFuncCall { name: true_dest });

                    let mut false_cmd = Execute::new();
                    false_cmd
                        .with_unless(ExecuteCondition::Score {
                            target: Target::Uuid(cond),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                        })
                        .with_run(McFuncCall { name: false_dest });

                    this.cmds.push(true_cmd.into());
                    this.cmds.push(false_cmd.into());
                }
                Terminator::Switch(Switch {
                    operand,
                    dests,
                    default_dest,
                    ..
                }) => {
                    let (cmds, operand) = eval_operand(operand);
                    this.cmds.extend(cmds);

                    let default_tracker = format!("%temp{}", get_unique_num());

                    this.cmds.push(
                        ScoreSet {
                            target: Target::Uuid(default_tracker.clone()),
                            target_obj: OBJECTIVE.to_string(),
                            score: 0,
                        }
                        .into(),
                    );

                    for (dest_value, dest_name) in dests.iter() {
                        let dest_value = if let Constant::Int { value, .. } = dest_value {
                            *value as i32
                        } else {
                            todo!("{:?}", dest_value)
                        };

                        let mut dest_name = mc_block_name(&func.name, dest_name);

                        if !options.direct_term {
                            dest_name.push_str("%%FIXUP");
                        }

                        let mut branch_cmd = Execute::new();
                        branch_cmd.with_if(ExecuteCondition::Score {
                            target: Target::Uuid(operand.clone()),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ExecuteCondKind::Matches(cir::McRange::Between(
                                dest_value..=dest_value,
                            )),
                        });

                        let mut add_cmd = branch_cmd.clone();

                        add_cmd.with_run(ScoreSet {
                            target: Target::Uuid(default_tracker.clone()),
                            target_obj: OBJECTIVE.to_string(),
                            score: 1,
                        });
                        branch_cmd.with_run(McFuncCall { name: dest_name });

                        this.cmds.push(add_cmd.into());
                        this.cmds.push(branch_cmd.into());
                    }

                    let mut default_dest = mc_block_name(&func.name, default_dest);

                    if !options.direct_term {
                        default_dest.push_str("%%FIXUP");
                    }

                    let mut default_cmd = Execute::new();
                    default_cmd.with_if(ExecuteCondition::Score {
                        target: Target::Uuid(default_tracker),
                        target_obj: OBJECTIVE.to_string(),
                        kind: ExecuteCondKind::Matches(cir::McRange::Between(0..=0)),
                    });
                    default_cmd.with_run(McFuncCall { name: default_dest });

                    this.cmds.push(default_cmd.into());
                }
                term => todo!("terminator {:?}", term),
            }

            this
        })
        .collect()
}

pub fn compile_instr(instr: &Instruction, _options: &Options) -> Vec<Command> {
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
            if indices.len() != 2 {
                todo!("{:?}", indices);
            }

            if !matches!(
                indices[0],
                Operand::ConstantOperand(Constant::Int { value: 0, .. })
            ) {
                todo!("{:?}", indices[0]);
            }

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

            // If we're directly storing a constant,
            // we can skip writing to a temporary value
            let write_cmd = match eval_maybe_const(value) {
                MaybeConst::Const(value) => write_ptr_const(value),
                MaybeConst::NonConst(eval_cmds, id) => {
                    cmds.extend(eval_cmds);
                    write_ptr(id)
                }
            };

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

            cmds.push(write_cmd);

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
            // TODO: When operand1 is a constant, we can optimize the direct comparison into a `matches`

            let mut normal = true;

            let relation = match predicate {
                IntPredicate::SGE => cir::Relation::GreaterThanEq,
                IntPredicate::SGT => cir::Relation::GreaterThan,
                IntPredicate::SLT => cir::Relation::LessThan,
                IntPredicate::SLE => cir::Relation::LessThanEq,
                IntPredicate::EQ => cir::Relation::Eq,
                IntPredicate::NE => {
                    normal = false;
                    cir::Relation::Eq
                }
                p => todo!("predicate {:?}", p),
            };

            let (mut cmds, target) = eval_operand(operand0);
            let (tmp_cmds, source) = eval_operand(operand1);
            cmds.extend(tmp_cmds);

            let mut cmd = Execute::new();
            cmd.with_subcmd(ExecuteSubCmd::Store {
                is_success: true,
                kind: ExecuteStoreKind::Score {
                    target: Target::Uuid(dest.to_string()),
                    objective: OBJECTIVE.to_string(),
                },
            })
            .with_subcmd(ExecuteSubCmd::Condition {
                is_unless: !normal,
                cond: ExecuteCondition::Score {
                    target: Target::Uuid(target),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ExecuteCondKind::Relation {
                        relation,
                        source: Target::Uuid(source),
                        source_obj: OBJECTIVE.to_string(),
                    },
                },
            });

            cmds.push(cmd.into());

            cmds
        }
        Instruction::Call(Call {
            function,
            arguments,
            dest,
            ..
        }) => {
            let function = match function {
                Either::Left(asm) => todo!("inline assembly {:?}", asm),
                Either::Right(operand) => operand,
            };

            if let Operand::ConstantOperand(Constant::GlobalReference {
                name: Name::Name(name),
                ..
            }) = function
            {
                match name.as_str() {
                    "print" => {
                        assert_eq!(arguments.len(), 1);

                        assert!(dest.is_none());

                        let (mut cmds, name) = eval_operand(&arguments[0].0);

                        cmds.push(Tellraw {
                            target: Target::Selector(cir::Selector { var: cir::SelectorVariable::AllPlayers, args: vec![] }),
                            message: format!("[{{\"score\": {{\"name\": \"{}\", \"objective\": \"rust\" }} }}]", name),
                        }.into());

                        cmds
                    }
                    "turtle_x" | "turtle_y" | "turtle_z" => {
                        assert_eq!(arguments.len(), 1);

                        assert!(dest.is_none());

                        let coord = if name.ends_with('x') {
                            0
                        } else if name.ends_with('y') {
                            1
                        } else {
                            2
                        };

                        // TODO: Optimize for const argument
                        let (mut cmds, pos) = eval_operand(&arguments[0].0);

                        let mut cmd = Execute::new();
                        cmd.with_as(Target::Selector(cir::Selector {
                            var: cir::SelectorVariable::AllEntities,
                            args: vec![cir::SelectorArg("tag=turtle".to_string())],
                        }));
                        cmd.with_subcmd(ExecuteSubCmd::Store {
                            is_success: false,
                            kind: ExecuteStoreKind::Data {
                                path: format!("Pos[{}]", coord),
                                ty: "double".to_string(),
                                scale: 1.0,
                                target: DataTarget::Entity(Target::Selector(cir::Selector {
                                    var: cir::SelectorVariable::ThisEntity,
                                    args: vec![],
                                })),
                            },
                        });
                        cmd.with_run(ScoreGet {
                            target: Target::Uuid(pos),
                            target_obj: OBJECTIVE.to_string(),
                        });

                        cmds.push(cmd.into());

                        cmds
                    }
                    "turtle_set" => {
                        assert_eq!(arguments.len(), 1);

                        assert!(dest.is_none());

                        let mc_block =
                            if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0) {
                                c
                            } else {
                                todo!("non-constant block {:?}", &arguments[0].0)
                            };

                        let mc_block = McBlock::try_from(mc_block).unwrap();

                        let mut cmd = Execute::new();
                        cmd.with_at(Target::Selector(cir::Selector {
                            var: cir::SelectorVariable::AllEntities,
                            args: vec![cir::SelectorArg("tag=turtle".to_string())],
                        }));
                        cmd.with_run(SetBlock {
                            block: mc_block.to_string(),
                            pos: "~ ~ ~".to_string(),
                            kind: SetBlockKind::Replace,
                        });

                        vec![cmd.into()]
                    }
                    "turtle_check" => {
                        assert_eq!(arguments.len(), 1);

                        let dest = dest.as_ref().expect("turtle_check should return a value");

                        let mc_block =
                            if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0) {
                                c
                            } else {
                                todo!("non-constant block {:?}", &arguments[0].0)
                            };

                        let block = McBlock::try_from(mc_block).unwrap().to_string();

                        let mut cmd = Execute::new();
                        cmd.with_subcmd(ExecuteSubCmd::Store {
                            is_success: true,
                            kind: ExecuteStoreKind::Score {
                                target: Target::Uuid(dest.to_string()),
                                objective: OBJECTIVE.to_string(),
                            },
                        });
                        cmd.with_at(Target::Selector(cir::Selector {
                            var: cir::SelectorVariable::AllEntities,
                            args: vec![cir::SelectorArg("tag=turtle".to_string())],
                        }));
                        cmd.with_if(ExecuteCondition::Block {
                            pos: "~ ~ ~".to_string(),
                            block,
                        });

                        vec![cmd.into()]
                    }
                    _ => {
                        if !arguments.is_empty() {
                            todo!("functions with parameters {:?}", arguments);
                        }

                        let block_name = mc_block_name(name, &Name::Number(0));

                        let mut cmds = Vec::new();

                        for i in 0..40 {
                            cmds.push(
                                ScoreOp {
                                    target: Target::Uuid(format!("%{}%{}", i, i)),
                                    target_obj: OBJECTIVE.to_string(),
                                    kind: ScoreOpKind::Assign,
                                    source: Target::Uuid(format!("%{}", i)),
                                    source_obj: OBJECTIVE.to_string(),
                                }
                                .into(),
                            )
                        }
                        cmds.push(McFuncCall { name: block_name }.into());
                        for i in 0..40 {
                            cmds.push(
                                ScoreOp {
                                    target: Target::Uuid(format!("%{}", i)),
                                    target_obj: OBJECTIVE.to_string(),
                                    kind: ScoreOpKind::Assign,
                                    source: Target::Uuid(format!("%{}%{}", i, i)),
                                    source_obj: OBJECTIVE.to_string(),
                                }
                                .into(),
                            )
                        }

                        if let Some(dest) = dest {
                            cmds.push(
                                ScoreOp {
                                    target: Target::Uuid(dest.to_string()),
                                    target_obj: OBJECTIVE.to_string(),
                                    kind: ScoreOpKind::Assign,
                                    source: Target::Uuid("%return".to_string()),
                                    source_obj: OBJECTIVE.to_string(),
                                }
                                .into(),
                            );
                        }

                        cmds
                    }
                }
            } else {
                todo!("non-constant function call {:?}", function)
            }
        }
        _ => todo!("instruction {:?}", instr),
    }
}

pub enum MaybeConst {
    Const(i32),
    NonConst(Vec<Command>, String),
}

impl MaybeConst {
    pub fn force_eval(self) -> (Vec<Command>, String) {
        match self {
            MaybeConst::Const(score) => {
                let target = format!("%temp{}", get_unique_num());
                (
                    vec![ScoreSet {
                        target: Target::Uuid(target.clone()),
                        target_obj: OBJECTIVE.to_string(),
                        score,
                    }
                    .into()],
                    target,
                )
            }
            MaybeConst::NonConst(cmds, id) => (cmds, id),
        }
    }
}

pub fn eval_maybe_const(op: &Operand) -> MaybeConst {
    match op {
        Operand::LocalOperand { name, .. } => MaybeConst::NonConst(vec![], name.to_string()),
        Operand::ConstantOperand(Constant::GlobalReference { name, .. }) => {
            let temp = name.to_string();
            MaybeConst::NonConst(vec![], format!("%@{}", &temp[1..temp.len() - 1]))
        }
        Operand::ConstantOperand(Constant::Int { bits: 32, value }) => {
            MaybeConst::Const(*value as i32)
        }
        _ => todo!("operand {:?}", op),
    }
}

pub fn eval_operand(op: &Operand) -> (Vec<Command>, String) {
    eval_maybe_const(op).force_eval()
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
