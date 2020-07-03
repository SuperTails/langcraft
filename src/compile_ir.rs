use crate::cir::FuncCall as McFuncCall;
use crate::cir::Function as McFunction;
use crate::cir::FunctionId as McFuncId;
use crate::cir::{
    self, Command, Data, DataKind, DataTarget, Execute, ExecuteCondKind, ExecuteCondition,
    ExecuteStoreKind, ExecuteSubCmd, ScoreAdd, ScoreGet, ScoreOp, ScoreOpKind, ScoreSet, SetBlock,
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
use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;
use std::sync::Mutex;

pub const OBJECTIVE: &str = "rust";

// %ptr, %x, %y, %z, %param<X> are caller-saved registers
// all other registers are callee-saved
// %stackptr is... weird
// %temp<X> are... weird

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
    // FIXME: It is actually *not correct* to directly terminate with a call sometimes!
    // And, on the other hand, a Call instruction MUST be a call!
    // edit: oh god functions are awful why do we have abstraction
    direct_term: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { direct_term: false }
    }
}

// This doesn't change what the function clobbers
fn apply_fixups(funcs: &mut [McFunction]) {
    for func_idx in 0..funcs.len() {
        for cmd_idx in 0..funcs[func_idx].cmds.len() {
            if let Command::FuncCall(McFuncCall { id }) = &mut funcs[func_idx].cmds[cmd_idx] {
                // TODO: `strip_suffix` is nightly but it's exactly what I'm doing
                if id.name.ends_with("%%FIXUP") {
                    // It doesn't matter what we replace it with
                    // because the whole command gets removed
                    let mut id = std::mem::replace(id, McFuncId::new(""));
                    id.name.truncate(id.name.len() - "%%FIXUP".len());

                    let idx = funcs
                        .iter()
                        .enumerate()
                        .find(|(_, f)| f.id == id)
                        .map(|(i, _)| i)
                        .unwrap_or_else(|| {
                            funcs
                                .iter()
                                .enumerate()
                                .find(|(_, f)| f.id.name == id.name)
                                .map(|(i, _)| i)
                                .unwrap_or_else(|| panic!("could not find {:?}", id))
                        });

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
                if let Command::FuncCall(McFuncCall { id }) = &mut **func_call {
                    if id.name.ends_with("%%FIXUP") {
                        let mut id = std::mem::replace(id, McFuncId::new(""));
                        id.name.truncate(id.name.len() - "%%FIXUP".len());

                        let idx = funcs
                            .iter()
                            .enumerate()
                            .find(|(_, f)| f.id == id)
                            .map(|(i, _)| i)
                            .unwrap_or_else(|| {
                                funcs
                                    .iter()
                                    .enumerate()
                                    .find(|(_, f)| f.id.name == id.name)
                                    .map(|(i, _)| i)
                                    .unwrap_or_else(|| panic!("could not find {:?}", id))
                            });

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
                } else if let Command::ScoreGet(ScoreGet { target: Target::Uuid(target), .. }) = &mut **func_call {
                    if target == "%%FIXUP" {

                        // This is a return address
                        let mut return_id = funcs[func_idx].id.clone();
                        return_id.sub += 1;

                        let idx = funcs
                            .iter()
                            .enumerate()
                            .find(|(_, f)| f.id == return_id)
                            .unwrap_or_else(|| panic!("could not find {:?}", return_id))
                            .0;

                        let mut cmd = Execute::new();
                        cmd.with_at(Target::Selector(cir::Selector { var: cir::SelectorVariable::AllEntities, args: vec![cir::SelectorArg("tag=ptr".to_string())]}));
                        cmd.with_run(Data {
                            target: DataTarget::Block("~ ~ ~".to_string()),
                            kind: DataKind::Modify {
                                path: "RecordItem.tag.Memory".to_string(),
                                kind: cir::DataModifyKind::Set,
                                source: cir::DataModifySource::Value(idx as i32)
                            }
                        });
                        funcs[func_idx].cmds[cmd_idx] = cmd.into();
                    }
                }
            }
        }
    }
}

pub fn compile_module(module: &Module, options: &Options) -> Vec<McFunction> {
    let main_return = get_alloc(1);

    let init_cmds = module
        .global_vars
        .iter()
        .flat_map(compile_global_var_init)
        .chain(std::iter::once(set_memory(-1, main_return as i32)))
        .chain(std::iter::once(
            ScoreSet {
                target: Target::Uuid("%stackptr".to_string()),
                target_obj: OBJECTIVE.to_string(),
                score: *FREE_PTR.lock().unwrap() as i32,
            }
            .into()
        ))
        .collect();

    let init_func = McFunction {
        id: McFuncId::new("init"),
        cmds: init_cmds,
    };

    let mut clobber_list = HashMap::<String, _>::new();
    let mut funcs = vec![init_func];

    for (mc_funcs, mut clobbers) in module
        .functions
        .iter()
        .map(|f| compile_function(f, options))
    {
        clobbers.remove("%stackptr");
        clobbers.remove("%return");
        clobbers.remove("%ptr");
        clobbers.remove("%%FIXUP");

        for McFunction { id, .. } in mc_funcs.iter() {
            clobber_list.insert(id.name.clone(), clobbers.clone());
        }
        funcs.extend(mc_funcs);
    }

    println!("{:?}", clobber_list);

    apply_fixups(&mut funcs);

    for func in funcs.iter_mut() {
        let get_save_idx = |cmds: &[Command]| {
            cmds.iter()
                .enumerate()
                .find(|(_, c)| {
                    if let Command::FuncCall(McFuncCall { id }) = c {
                        id.name == "%%SAVEREGS"
                    } else {
                        false
                    }
                })
                .map(|(i, _)| i)
        };

        while let Some(save_idx) = get_save_idx(&func.cmds) {
            println!("Adding save code at {} idx {}", func.id, save_idx);
            func.cmds.remove(save_idx);

            let base_set = ScoreOp {
                target: Target::Uuid("%stackbaseptr".to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::Assign,
                source: Target::Uuid("%stackptr".to_string()),
                source_obj: OBJECTIVE.to_string(),
            }
            .into();

            let save_code = clobber_list
                .get(&func.id.name)
                .unwrap()
                .iter()
                .cloned()
                .chain(std::iter::once("%stackbaseptr".to_string()))
                .map(push)
                .flatten()
                .chain(std::iter::once(base_set));

            func.cmds.splice(save_idx..save_idx, save_code);
        }

        let get_load_idx = |cmds: &[Command]| {
            cmds.iter()
                .enumerate()
                .find(|(_, c)| {
                    if let Command::FuncCall(McFuncCall { id }) = c {
                        id.name == "%%LOADREGS"
                    } else {
                        false
                    }
                })
                .map(|(i, _)| i)
        };

        while let Some(load_idx) = get_load_idx(&func.cmds) {
            println!("Adding load code at {} idx {}", func.id, load_idx);
            func.cmds.remove(load_idx);

            let base_read = ScoreOp {
                target: Target::Uuid("%stackptr".to_string()),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::Assign,
                source: Target::Uuid("%stackbaseptr".to_string()),
                source_obj: OBJECTIVE.to_string(),
            }
            .into();

            let load_code = std::iter::once(base_read).chain(
                clobber_list
                    .get(&func.id.name)
                    .unwrap()
                    .iter()
                    .cloned()
                    .chain(std::iter::once("%stackbaseptr".to_string()))
                    .rev()
                    .map(pop)
                    .flatten(),
            );

            func.cmds.splice(load_idx..load_idx, load_code);
        }
    }

    if !options.direct_term {
        let build_cmds = funcs
            .iter()
            .enumerate()
            .map(|(idx, func)| {
                let pos = format!("-2 0 {}", idx);
                let block = format!(
                    "minecraft:command_block{{Command:\"{}\"}}",
                    McFuncCall { id: func.id.clone() }
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
#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
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

static MC_BLOCKS: [McBlock; 10] = [McBlock::Air, McBlock::Cobblestone, McBlock::Granite, McBlock::Andesite, McBlock::Diorite, McBlock::LapisBlock, McBlock::IronBlock, McBlock::GoldBlock, McBlock::DiamondBlock, McBlock::RedstoneBlock];

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

fn compile_call(
    Call {
        function,
        arguments,
        dest,
        ..
    }: &Call,
) -> (Vec<Command>, Option<Vec<Command>>) {
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

                cmds.push(
                    Tellraw {
                        target: Target::Selector(cir::Selector {
                            var: cir::SelectorVariable::AllPlayers,
                            args: vec![],
                        }),
                        message: format!(
                            "[{{\"score\": {{\"name\": \"{}\", \"objective\": \"rust\" }} }}]",
                            name
                        ),
                    }
                    .into(),
                );

                (cmds, None)
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

                (cmds, None)
            }
            "turtle_set" => {
                assert_eq!(arguments.len(), 1);

                assert!(dest.is_none());

                let mc_block = if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0) {
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

                (vec![cmd.into()], None)
            }
            "turtle_check" => {
                assert_eq!(arguments.len(), 1);

                let dest = dest.as_ref().expect("turtle_check should return a value");

                let mc_block = if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0) {
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

                (vec![cmd.into()], None)
            }
            "turtle_get" => {
                assert_eq!(arguments.len(), 0);

                let dest = dest.as_ref().expect("turtle_get should return a value");

                let mut cmds = Vec::new();

                // Default value (Air)
                cmds.push(ScoreSet {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    score: 0,
                }.into());

                for block in MC_BLOCKS[1..].iter() {
                    let mut cmd = Execute::new();
                    cmd.with_at(Target::Selector(cir::Selector {
                        var: cir::SelectorVariable::AllEntities,
                        args: vec![cir::SelectorArg("tag=turtle".to_string())]
                    }));
                    cmd.with_if(ExecuteCondition::Block {
                        pos: "~ ~ ~".to_string(),
                        block: block.to_string(),
                    });
                    cmd.with_run(ScoreSet {
                        target: Target::Uuid(dest.to_string()),
                        target_obj: OBJECTIVE.to_string(),
                        score: *block as i32,
                    });
                    cmds.push(cmd.into());
                }

                (cmds, None)
            }
            _ => {
                let mut callee_id = McFuncId::new(name);

                callee_id.name.push_str("%%FIXUP");

                let mut before_cmds = Vec::new();
                // Push return address
                before_cmds.extend(push("%%FIXUP".to_string()));

                // Set arguments
                for (idx, (arg, _attrs)) in arguments.iter().enumerate() {
                    match eval_maybe_const(arg) {
                        MaybeConst::Const(score) => {
                            before_cmds.push(ScoreSet {
                                target: Target::Uuid(format!("%param{}", idx)),
                                target_obj: OBJECTIVE.to_string(),
                                score,
                            }.into());
                        }
                        MaybeConst::NonConst(cmds, source) => {
                            before_cmds.extend(cmds);
                            before_cmds.push(ScoreOp {
                                target: Target::Uuid(format!("%param{}", idx)),
                                target_obj: OBJECTIVE.to_string(),
                                kind: ScoreOpKind::Assign,
                                source: Target::Uuid(source),
                                source_obj: OBJECTIVE.to_string(),
                            }.into());
                        }
                    }
                }

                // Branch to function
                before_cmds.push(McFuncCall { id: callee_id }.into());

                let after_cmds = if let Some(dest) = dest {
                    vec![
                        ScoreOp {
                            target: Target::Uuid(dest.to_string()),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ScoreOpKind::Assign,
                            source: Target::Uuid("%return".to_string()),
                            source_obj: OBJECTIVE.to_string(),
                        }
                        .into(),
                    ]
                } else {
                    Vec::new()
                };

                (before_cmds, Some(after_cmds))
            }
        }
    } else {
        todo!("non-constant function call {:?}", function)
    }
}

type ScoreHolder = String;

pub fn compile_function(
    func: &Function,
    options: &Options,
) -> (Vec<McFunction>, BTreeSet<ScoreHolder>) {
    if func.is_var_arg {
        todo!("functions with variadic arguments");
    }

    if func.basic_blocks.is_empty() {
        todo!("functions with no basic blocks");
    }

    let funcs = func
        .basic_blocks
        .iter()
        .enumerate()
        .flat_map(|(idx, block)| {
            let mut result = Vec::new();

            let mut sub = 0;

            let make_new_func = |sub| {
                McFunction {
                    id: McFuncId { name: func.name.clone(), block: block.name.clone(), sub },
                    cmds: vec![]
                }
            };

            let mut this = make_new_func(sub);
            sub += 1;

            if idx == 0 {
                this.cmds.push(
                    McFuncCall {
                        id: McFuncId::new("%%SAVEREGS"),
                    }
                    .into(),
                );

                for (idx, arg) in func.parameters.iter().enumerate() {
                    this.cmds.push(ScoreOp {
                        target: Target::Uuid(arg.name.to_string()),
                        target_obj: OBJECTIVE.to_string(),
                        kind: ScoreOpKind::Assign,
                        source: Target::Uuid(format!("%param{}", idx)),
                        source_obj: OBJECTIVE.to_string(),
                    }.into());
                }
            }

            for instr in block.instrs.iter() {
                let (before, after) = compile_instr(instr, options);
                this.cmds.extend(before);

                if let Some(after) = after {
                    result.push(std::mem::replace(&mut this, make_new_func(sub)));
                    sub += 1;
                    this.cmds.extend(after);
                }
            }

            match &block.term {
                Terminator::Ret(Ret {
                    return_operand: None,
                    ..
                }) => {
                    this.cmds.push(
                        McFuncCall {
                            id: McFuncId::new("%%LOADREGS"),
                        }
                        .into(),
                    );

                    this.cmds.push(McFuncCall { id: McFuncId::new("intrinsic:pop_and_branch")}.into());
                }
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

                    this.cmds.push(
                        McFuncCall {
                            id: McFuncId::new("%%LOADREGS"),
                        }
                        .into(),
                    );

                    this.cmds.push(McFuncCall { id: McFuncId::new("intrinsic:pop_and_branch")}.into());
                }
                Terminator::Br(Br { dest, .. }) => {
                    let mut id = McFuncId::new_block(&func.name, dest.clone());

                    if !options.direct_term {
                        id.name.push_str("%%FIXUP");
                    }

                    this.cmds.push(McFuncCall { id }.into());
                }
                Terminator::CondBr(CondBr {
                    condition,
                    true_dest,
                    false_dest,
                    ..
                }) => {
                    let (cmds, cond) = eval_operand(condition);
                    this.cmds.extend(cmds);

                    let mut true_dest = McFuncId::new_block(&func.name, true_dest.clone());
                    let mut false_dest = McFuncId::new_block(&func.name, false_dest.clone());

                    if !options.direct_term {
                        true_dest.name.push_str("%%FIXUP");
                        false_dest.name.push_str("%%FIXUP");
                    }

                    let mut true_cmd = Execute::new();
                    true_cmd
                        .with_if(ExecuteCondition::Score {
                            target: Target::Uuid(cond.clone()),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                        })
                        .with_run(McFuncCall { id: true_dest });

                    let mut false_cmd = Execute::new();
                    false_cmd
                        .with_unless(ExecuteCondition::Score {
                            target: Target::Uuid(cond),
                            target_obj: OBJECTIVE.to_string(),
                            kind: ExecuteCondKind::Matches(cir::McRange::Between(1..=1)),
                        })
                        .with_run(McFuncCall { id: false_dest });

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

                        let mut dest_id = McFuncId::new_block(&func.name, dest_name.clone());

                        if !options.direct_term {
                            dest_id.name.push_str("%%FIXUP");
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
                        branch_cmd.with_run(McFuncCall { id: dest_id });

                        this.cmds.push(add_cmd.into());
                        this.cmds.push(branch_cmd.into());
                    }

                    let mut default_dest = McFuncId::new_block(&func.name, default_dest.clone());

                    if !options.direct_term {
                        default_dest.name.push_str("%%FIXUP");
                    }

                    let mut default_cmd = Execute::new();
                    default_cmd.with_if(ExecuteCondition::Score {
                        target: Target::Uuid(default_tracker),
                        target_obj: OBJECTIVE.to_string(),
                        kind: ExecuteCondKind::Matches(cir::McRange::Between(0..=0)),
                    });
                    default_cmd.with_run(McFuncCall { id: default_dest });

                    this.cmds.push(default_cmd.into());
                }
                term => todo!("terminator {:?}", term),
            }

            result.push(this);

            if !options.direct_term {
                for sub_block in result.iter_mut() {
                    sub_block.cmds.insert(0, 
                        SetBlock {
                            pos: "~ ~1 ~".to_string(),
                            block: "minecraft:air".to_string(),
                            kind: SetBlockKind::Replace,
                        }
                        .into(),
                    );
                }
            }

            result
        })
        .collect::<Vec<_>>();

    let mut clobbers = BTreeSet::new();
    for cmd in funcs.iter().flat_map(|f| f.cmds.iter()) {
        let cmd_str = cmd.to_string();
        for holder in cmd_str.split_whitespace().filter(|s| s.starts_with('%')) {
            clobbers.insert(holder.to_owned());
        }
    }

    (funcs, clobbers)
}

pub fn compile_arithmetic(
    operand0: &Operand,
    operand1: &Operand,
    dest: &Name,
    kind: ScoreOpKind,
) -> Vec<Command> {
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
            kind,
            source: Target::Uuid(source1),
            source_obj: OBJECTIVE.to_string(),
        }
        .into(),
    );
    cmds
}

pub fn push(target: String) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(
        ScoreOp {
            target: Target::Uuid("%ptr".to_string()),
            target_obj: OBJECTIVE.to_string(),
            kind: ScoreOpKind::Assign,
            source: Target::Uuid("%stackptr".to_string()),
            source_obj: OBJECTIVE.to_string(),
        }
        .into(),
    );
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(write_ptr(target));
    cmds.push(
        ScoreAdd {
            target: Target::Uuid("%stackptr".to_string()),
            target_obj: OBJECTIVE.to_string(),
            score: 1,
        }
        .into(),
    );

    cmds
}

pub fn pop(target: String) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(
        ScoreAdd {
            target: Target::Uuid("%stackptr".to_string()),
            target_obj: OBJECTIVE.to_string(),
            score: -1,
        }
        .into(),
    );
    cmds.push(
        ScoreOp {
            target: Target::Uuid("%ptr".to_string()),
            target_obj: OBJECTIVE.to_string(),
            kind: ScoreOpKind::Assign,
            source: Target::Uuid("%stackptr".to_string()),
            source_obj: OBJECTIVE.to_string(),
        }
        .into(),
    );
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(read_ptr(target));

    cmds
}

pub fn compile_instr(instr: &Instruction, _options: &Options) -> (Vec<Command>, Option<Vec<Command>>) {
    let result = match instr {
        // We use an empty stack
        Instruction::Alloca(Alloca {
            allocated_type: Type::IntegerType { bits: 32 },
            num_elements,
            dest,
            ..
        }) => {
            // TODO: This is never deallocated lol

            let num = if let Operand::ConstantOperand(Constant::Int { bits: 32, value: 1 }) =
                num_elements
            {
                1
            } else {
                todo!("{:?}", num_elements);
            };

            let mut cmds = Vec::new();

            cmds.push(
                ScoreOp {
                    target: Target::Uuid(dest.to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::Assign,
                    source: Target::Uuid("%stackptr".to_string()),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                ScoreAdd {
                    target: Target::Uuid("%stackptr".to_string()),
                    target_obj: OBJECTIVE.to_string(),
                    score: num,
                }
                .into(),
            );

            cmds
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
                    id: McFuncId::new("intrinsic:setptr"),
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
                    id: McFuncId::new("intrinsic:setptr"),
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
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::AddAssign),
        Instruction::Mul(Mul {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::MulAssign),
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
        Instruction::Call(call) => return compile_call(call),
        _ => todo!("instruction {:?}", instr),
    };

    (result, None)
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
