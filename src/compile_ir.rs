use crate::cir::FuncCall as McFuncCall;
use crate::cir::Function as McFunction;
use crate::cir::FunctionId as McFuncId;
use crate::cir::{
    self, Command, Data, DataKind, DataTarget, Execute, ExecuteCondKind, ExecuteCondition,
    ExecuteStoreKind, ExecuteSubCmd, ScoreAdd, ScoreGet, ScoreOp, ScoreOpKind, ScoreSet, SetBlock,
    SetBlockKind, Target, Tellraw, ScoreHolder,
};
use either::Either;
use lazy_static::lazy_static;
use llvm_ir::instruction::{
    Add, Alloca, BitCast, Call, GetElementPtr, ICmp, InsertValue, Load, Mul, Store, Sub, Trunc, Select, ExtractValue, ZExt, Phi
};
use llvm_ir::module::GlobalVariable;
use llvm_ir::terminator::{Br, CondBr, Ret, Switch, Unreachable};
use llvm_ir::{
    Constant, Function, Instruction, IntPredicate, Module, Name, Operand, Terminator, Type,
};
use llvm_ir::types::Typed;
use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;
use std::sync::Mutex;

pub const OBJECTIVE: &str = "rust";

pub fn stackptr() -> ScoreHolder {
    ScoreHolder::new("%stackptr".to_string()).unwrap()
}

pub fn stackbaseptr() -> ScoreHolder {
    ScoreHolder::new("%stackbaseptr".to_string()).unwrap()
}

pub fn ptr() -> ScoreHolder {
    ScoreHolder::new("%ptr".to_string()).unwrap()
}

pub fn param(index: usize, word_index: usize) -> ScoreHolder {
    ScoreHolder::new(format!("%param{}%{}", index, word_index)).unwrap()
}

pub fn return_holder(word_index: usize) -> ScoreHolder {
    ScoreHolder::new(format!("%return%{}", word_index)).unwrap()
}

pub fn print_entry(location: &McFuncId) -> Command {
    Tellraw {
        target: cir::Selector { var: cir::SelectorVariable::AllPlayers, args: Vec::new() }.into(),
        message: format!("[{{\"text\": \"entered block {}\"}}]", location),

    }.into()
}


// %ptr, %x, %y, %z, %param<X> are caller-saved registers
// all other registers are callee-saved
// %stackptr is... weird
// %temp<X> are... weird

// `intrinsic:setptr` sets the pointer to the value in `%ptr` for objective `rust`

// reads the current pointer location into some target for objective `rust`
pub fn read_ptr(target: ScoreHolder) -> Command {
    let mut exec = Execute::new();
    exec.with_at(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=ptr".to_string())],
    }.into());
    exec.with_subcmd(ExecuteSubCmd::Store {
        is_success: false,
        kind: ExecuteStoreKind::Score {
            target: target.into(),
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

/// Shorthand for a `ScoreOp` assignment between two score holders on the default objective
pub fn assign(target: ScoreHolder, source: ScoreHolder) -> Command {
    ScoreOp {
        target: target.into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ScoreOpKind::Assign,
        source: source.into(),
        source_obj: OBJECTIVE.to_string(),
    }.into()
}

/// Returns xyz
pub fn get_address(mut address: i32) -> (i32, i32, i32) {
    if address % 4 != 0 {
        todo!("{:?}", address);
    }
    address /= 4;

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
    exec.with_at(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=ptr".to_string())],
    }.into());
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
pub fn write_ptr(target: ScoreHolder) -> Command {
    let mut exec = Execute::new();
    exec.with_at(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=ptr".to_string())],
    }.into());
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
        target: target.into(),
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
                if id.name.ends_with("%%fixup") {
                    // It doesn't matter what we replace it with
                    // because the whole command gets removed
                    let mut id = std::mem::replace(id, McFuncId::new(""));
                    id.name.truncate(id.name.len() - "%%fixup".len());

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
                    if id.name.ends_with("%%fixup") {
                        let mut id = std::mem::replace(id, McFuncId::new(""));
                        id.name.truncate(id.name.len() - "%%fixup".len());

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
                } else if let Command::ScoreGet(ScoreGet {
                    target: Target::Uuid(target),
                    ..
                }) = &mut **func_call
                {
                    if target.as_ref() == "%%fixup" {
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
                        cmd.with_at(Target::Selector(cir::Selector {
                            var: cir::SelectorVariable::AllEntities,
                            args: vec![cir::SelectorArg("tag=ptr".to_string())],
                        }));
                        cmd.with_run(Data {
                            target: DataTarget::Block("~ ~ ~".to_string()),
                            kind: DataKind::Modify {
                                path: "RecordItem.tag.Memory".to_string(),
                                kind: cir::DataModifyKind::Set,
                                source: cir::DataModifySource::Value(idx as i32),
                            },
                        });
                        funcs[func_idx].cmds[cmd_idx] = cmd.into();
                    }
                }
            }
        }
    }
}

pub fn compile_module(module: &Module, options: &Options) -> Vec<McFunction> {
    let mut init_cmds = module
        .global_vars
        .iter()
        .flat_map(compile_global_var_init)
        .collect::<Vec<_>>();

    let main_return = get_alloc(4);

    init_cmds.push(set_memory(-1, main_return as i32));

    init_cmds.push(
        ScoreSet {
            target: stackptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: get_alloc(4) as i32,
        }
        .into(),
    );

    let init_func = McFunction {
        id: McFuncId::new("init"),
        cmds: init_cmds,
    };

    let mut clobber_list = HashMap::<String, BTreeSet<ScoreHolder>>::new();
    let mut funcs = vec![init_func];

    for (mc_funcs, mut clobbers) in module
        .functions
        .iter()
        .map(|f| compile_function(f, options))
    {
        clobbers.remove(&stackptr());
        clobbers.remove(&ptr());
        clobbers.remove(&ScoreHolder::new("%%fixup".to_string()).unwrap());
        clobbers.remove(&ScoreHolder::new("%phi".to_string()).unwrap());
        clobbers = clobbers.into_iter().filter(|e| !e.as_ref().starts_with("%return%")).collect();

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
                        id.name == "%%saveregs"
                    } else {
                        false
                    }
                })
                .map(|(i, _)| i)
        };

        while let Some(save_idx) = get_save_idx(&func.cmds) {
            println!("Adding save code at {} idx {}", func.id, save_idx);
            func.cmds.remove(save_idx);

            let base_set = assign(stackbaseptr(), stackptr());

            let save_code = std::iter::once(Tellraw {
                target: cir::Selector { var: cir::SelectorVariable::AllPlayers, args: vec![] }.into(),
                message: format!("[{{\"text\": \"%stackptr at start of {} is \"}}, {{\"score\": {{\"name\": \"%stackptr\", \"objective\": \"rust\" }} }}]", func.id),
            }.into()).chain(clobber_list
                .get(&func.id.name)
                .unwrap()
                .iter()
                .cloned()
                .chain(std::iter::once(stackbaseptr()))
                .map(push)
                .flatten()
                .chain(std::iter::once(base_set)));

            func.cmds.splice(save_idx..save_idx, save_code);
        }

        let get_load_idx = |cmds: &[Command]| {
            cmds.iter()
                .enumerate()
                .find(|(_, c)| {
                    if let Command::FuncCall(McFuncCall { id }) = c {
                        id.name == "%%loadregs"
                    } else {
                        false
                    }
                })
                .map(|(i, _)| i)
        };

        while let Some(load_idx) = get_load_idx(&func.cmds) {
            println!("Adding load code at {} idx {}", func.id, load_idx);
            func.cmds.remove(load_idx);

            let base_read = assign(stackptr(), stackbaseptr());

            let load_code = std::iter::once(base_read).chain(
                clobber_list
                    .get(&func.id.name)
                    .unwrap()
                    .iter()
                    .cloned()
                    .chain(std::iter::once(stackbaseptr()))
                    .rev()
                    .map(pop)
                    .flatten(),
            );

            func.cmds.splice(load_idx..load_idx, load_code);
        }
    }

    if !options.direct_term {
        let mut build_cmds = funcs
            .iter()
            .enumerate()
            .map(|(idx, func)| {
                let pos = format!("-2 0 {}", idx);
                let block = format!(
                    "minecraft:command_block{{Command:\"function rust:{}\"}}",
                    func.id
                );

                SetBlock {
                    pos,
                    block,
                    kind: SetBlockKind::Destroy,
                }
                .into()
            })
            .collect::<Vec<Command>>();

        build_cmds.insert(
            0,
            cir::Fill {
                start: "-2 0 0".to_string(),
                end: "-2 0 150".to_string(),
                block: "minecraft:air".to_string(),
            }
            .into(),
        );

        funcs[0].cmds.extend(build_cmds);
    }

    let main_idx = funcs
        .iter()
        .enumerate()
        .find(|(_, f)| f.id == McFuncId::new("main"))
        .map(|(i, _)| i)
        .unwrap_or_else(|| {
            funcs
                .iter()
                .enumerate()
                .find(|(_, f)| f.id.name == "main")
                .map(|(i, _)| i)
                .unwrap_or_else(|| panic!("could not find main"))
        });

    for func in &mut funcs[1..] {
        func.cmds.insert(0, print_entry(&func.id));
    }

    funcs.push(McFunction {
        id: McFuncId::new("run"),
        cmds: vec![SetBlock {
            pos: format!("-2 1 {}", main_idx),
            block: "minecraft:redstone_block".to_string(),
            kind: SetBlockKind::Replace,
        }.into()]
    });

    funcs
}

fn compile_global_var_init(v: &GlobalVariable) -> Vec<Command> {
    if matches!(v.name, Name::Number(_)) {
        todo!()
    }

    let temp = v.name.to_string();
    let target = ScoreHolder::new(format!("%@{}", &temp[1..temp.len() - 1])).unwrap();

    match &v.initializer {
        Some(Constant::Int { bits: 32, value }) => vec![Command::from(ScoreSet {
            target: target.into(),
            target_obj: OBJECTIVE.to_string(),
            score: *value as i32,
        })],
        Some(Constant::Array {
            element_type: Type::IntegerType { bits: 32 },
            elements,
        }) => {
            let start = get_alloc(4 * elements.len() as u32);

            let mut cmds = vec![ScoreSet {
                target: target.into(),
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

                cmds.push(set_memory(score, start as i32 + 4 * idx as i32));
            }

            cmds
        }
        Some(Constant::Struct {
            name: None,
            values,
            is_packed: false,
        }) => {
            let start = get_alloc(4 * values.len() as u32);

            let mut cmds = vec![ScoreSet {
                target: Target::Uuid(target),
                target_obj: OBJECTIVE.to_string(),
                score: start as i32,
            }
            .into()];

            for (idx, v) in values.iter().enumerate() {
                // TODO: Support nested structs
                if let Constant::Int { bits: 32, value } = v {
                    cmds.push(set_memory(*value as i32, start as i32 + 4 * idx as i32));
                } else {
                    todo!("value {:?}", v);
                }
            }

            cmds
        }
        _ => todo!("constant {:?}", v),
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

static MC_BLOCKS: [McBlock; 10] = [
    McBlock::Air,
    McBlock::Cobblestone,
    McBlock::Granite,
    McBlock::Andesite,
    McBlock::Diorite,
    McBlock::LapisBlock,
    McBlock::IronBlock,
    McBlock::GoldBlock,
    McBlock::DiamondBlock,
    McBlock::RedstoneBlock,
];

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

// FIXME: This whole function will break on both large lengths and with lengths not a multiple of 4
fn compile_memcpy(arguments: &[(Operand, Vec<llvm_ir::function::ParameterAttribute>)]) -> Vec<Command> {
    if let [(dest, _), (src, _), (len, _), (volatile, _)] = &arguments[..] {
        let (mut cmds, src1) = eval_operand(src);
        let (tmp, dest1) = eval_operand(dest);
        cmds.extend(tmp);
        let (tmp, len1) = eval_operand(len);
        cmds.extend(tmp);

        assert_eq!(src1.len(), 1, "multiword pointer {:?}", src);
        assert_eq!(dest1.len(), 1, "multiword pointer {:?}", dest);
        assert_eq!(len1.len(), 1, "multiword length {:?}", len);
        
        cmds.push(assign(param(0, 0), dest1[0].clone()));
        cmds.push(assign(param(1, 0), src1[0].clone()));
        cmds.push(assign(param(2, 0), len1[0].clone()));

        if !matches!(
            volatile,
            Operand::ConstantOperand(Constant::Int { bits: 1, value: 0 })
        ) {
            todo!("{:?}", volatile)
        }

        cmds.push(McFuncCall { id: McFuncId::new("intrinsic:memcpy") }.into());

        cmds
    } else {
        panic!("{:?}", arguments);
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
        ty: Type::FuncType { result_type, is_var_arg: false, .. },
    }) = function
    {
        let dest_size = size_of_type(result_type);
        let dest = dest.clone().map(|d| ScoreHolder::from_local_name(d, dest_size));

        match name.as_str() {
            "print" => {
                assert_eq!(arguments.len(), 1);

                assert!(dest.is_none());

                let (mut cmds, name) = eval_operand(&arguments[0].0);

                let name = name[0].clone();

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

                let pos = pos[0].clone();

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
                assert_eq!(dest.len(), 1);
                let dest = dest[0].clone();

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
                        target: dest.into(),
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
            "turtle_get_char" => {
                assert_eq!(arguments.len(), 0);

                let dest = dest.as_ref().expect("turtle_get_char should return a value");

                assert_eq!(dest.len(), 1, "wrong length for dest");
                let dest = dest[0].clone();

                let mut cmds = Vec::new();

                // Default value (a space)
                cmds.push(
                    ScoreSet {
                        target: dest.clone().into(),
                        target_obj: OBJECTIVE.to_string(),
                        score: b' ' as i32,
                    }
                    .into(),
                );

                let mut valid_chars = (b'A'..=b'Z').collect::<Vec<_>>();
                valid_chars.push(b'[');
                valid_chars.push(b']');

                for c in valid_chars {
                    let is_white = c == b'H' || c == b'Q' || c == b'S';

                    let mut block = if is_white {
                        "minecraft:white_wall_banner"
                    } else {
                        "minecraft:light_blue_wall_banner"
                    }.to_string();

                    block.push_str(&format!("{{ CustomName: \"{{\\\"text\\\":\\\"{}\\\"}}\"}}", char::from(c)));

                    let mut cmd = Execute::new();
                    cmd.with_at(cir::Selector {
                        var: cir::SelectorVariable::AllEntities,
                        args: vec![cir::SelectorArg("tag=turtle".to_string())]
                    }.into());
                    cmd.with_if(ExecuteCondition::Block {
                        pos: "~ ~ ~".to_string(),
                        block,
                    });
                    cmd.with_run(ScoreSet {
                        target: dest.clone().into(),
                        target_obj: OBJECTIVE.into(),
                        score: c as i32,
                    });
                    cmds.push(cmd.into());
                }

                (cmds, None)
            }
            "turtle_get" => {
                assert_eq!(arguments.len(), 0);

                let dest = dest.as_ref().expect("turtle_get should return a value");

                assert_eq!(dest.len(), 1, "wrong length for dest");
                let dest = dest[0].clone();

                let mut cmds = Vec::new();

                // Default value (Air)
                cmds.push(
                    ScoreSet {
                        target: dest.clone().into(),
                        target_obj: OBJECTIVE.to_string(),
                        score: 0,
                    }
                    .into(),
                );

                for block in MC_BLOCKS[1..].iter() {
                    let mut cmd = Execute::new();
                    cmd.with_at(cir::Selector {
                        var: cir::SelectorVariable::AllEntities,
                        args: vec![cir::SelectorArg("tag=turtle".to_string())],
                    }.into());
                    cmd.with_if(ExecuteCondition::Block {
                        pos: "~ ~ ~".to_string(),
                        block: block.to_string(),
                    });
                    cmd.with_run(ScoreSet {
                        target: dest.clone().into(),
                        target_obj: OBJECTIVE.to_string(),
                        score: *block as i32,
                    });
                    cmds.push(cmd.into());
                }

                (cmds, None)
            }
            "llvm.dbg.declare" => {
                (vec![], None)
            }
            "llvm.dbg.value" => {
                (vec![], None)
            }
            "llvm.memcpy.p0i8.p0i8.i32" => {
                assert_eq!(dest, None);
                (compile_memcpy(arguments), None)
            }
            "llvm.lifetime.start.p0i8" => {
                assert_eq!(dest, None);
                (vec![], None)
            }
            "llvm.lifetime.end.p0i8" => {
                assert_eq!(dest, None);
                (vec![], None)
            }
            _ => {
                let mut callee_id = McFuncId::new(name);

                callee_id.name.push_str("%%fixup");

                let mut before_cmds = Vec::new();

                // Push return address
                before_cmds.extend(push(ScoreHolder::new("%%fixup".to_string()).unwrap()));

                // Set arguments
                for (idx, (arg, _attrs)) in arguments.iter().enumerate() {
                    match eval_maybe_const(arg) {
                        MaybeConst::Const(score) => {
                            before_cmds.push(
                                ScoreSet {
                                    target: param(idx, 0).into(),
                                    target_obj: OBJECTIVE.to_string(),
                                    score,
                                }
                                .into(),
                            );
                        }
                        MaybeConst::NonConst(cmds, source) => {
                            before_cmds.extend(cmds);

                            for (word_idx, source_word) in source.into_iter().enumerate() {
                                before_cmds.push(assign(param(idx, word_idx), source_word));
                            }
                        }
                    }
                }

                // Branch to function
                before_cmds.push(McFuncCall { id: callee_id }.into());

                let after_cmds = if let Some(dest) = dest {
                    dest
                        .into_iter()
                        .enumerate()
                        .map(|(idx, dest)|
                            assign(dest, return_holder(idx))
                        )
                        .collect()
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

    println!("Function {}, {:?}", func.name, func.debugloc);

    let funcs = func
        .basic_blocks
        .iter()
        .enumerate()
        .flat_map(|(idx, block)| {
            let mut result = Vec::new();

            let mut sub = 0;

            let make_new_func = |sub| McFunction {
                id: McFuncId::new_sub(func.name.clone(), block.name.clone(), sub),
                cmds: vec![],
            };

            let mut this = make_new_func(sub);
            sub += 1;

            if idx == 0 {
                this.cmds.push(
                    McFuncCall {
                        id: McFuncId::new("%%saveregs"),
                    }
                    .into(),
                );
                
                for (idx, arg) in func.parameters.iter().enumerate() {
                    let arg_size = size_of_type(&arg.ty);

                    if arg_size != 4 {
                        todo!("multiword parameters {:?}", arg.ty);
                    }

                    let arg_holder = ScoreHolder::from_local_name(arg.name.clone(), arg_size);
                    let arg_holder = arg_holder[0].clone();

                    this.cmds.push(assign(arg_holder, param(idx, 0)));
                }
            }

            for instr in block.instrs.iter() {
                let (before, after) = compile_instr(instr, func, options);
                this.cmds.extend(before);

                if let Some(after) = after {
                    result.push(std::mem::replace(&mut this, make_new_func(sub)));
                    sub += 1;
                    this.cmds.extend(after);
                }
            }

            this.cmds.push(ScoreSet {
                target: ScoreHolder::new("%phi".to_string()).unwrap().into(),
                target_obj: OBJECTIVE.into(),
                score: idx as i32,
            }.into());

            match &block.term {
                Terminator::Ret(Ret {
                    return_operand: None,
                    ..
                }) => {
                    this.cmds.push(
                        McFuncCall {
                            id: McFuncId::new("%%loadregs"),
                        }
                        .into(),
                    );

                    this.cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:pop_and_branch"),
                        }
                        .into(),
                    );
                }
                Terminator::Ret(Ret {
                    return_operand: Some(operand),
                    ..
                }) => {
                    let (cmds, source) = eval_operand(operand);

                    this.cmds.extend(cmds);

                    for (idx, word) in source.into_iter().enumerate() {
                        this.cmds.push(assign(return_holder(idx), word));
                    }

                    this.cmds.push(
                        McFuncCall {
                            id: McFuncId::new("%%loadregs"),
                        }
                        .into(),
                    );

                    this.cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:pop_and_branch"),
                        }
                        .into(),
                    );
                }
                Terminator::Br(Br { dest, .. }) => {
                    let mut id = McFuncId::new_block(&func.name, dest.clone());

                    if !options.direct_term {
                        id.name.push_str("%%fixup");
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

                    assert_eq!(cond.len(), 1);
                    let cond = cond[0].clone();

                    let mut true_dest = McFuncId::new_block(&func.name, true_dest.clone());
                    let mut false_dest = McFuncId::new_block(&func.name, false_dest.clone());

                    if !options.direct_term {
                        true_dest.name.push_str("%%fixup");
                        false_dest.name.push_str("%%fixup");
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

                    if operand.len() != 1 {
                        todo!("multibyte operand in switch {:?}", operand);
                    }

                    let operand = operand[0].clone();

                    let default_tracker = get_unique_holder();

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
                            dest_id.name.push_str("%%fixup");
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
                            target: default_tracker.clone().into(),
                            target_obj: OBJECTIVE.to_string(),
                            score: 1,
                        });
                        branch_cmd.with_run(McFuncCall { id: dest_id });

                        this.cmds.push(add_cmd.into());
                        this.cmds.push(branch_cmd.into());
                    }

                    let mut default_dest = McFuncId::new_block(&func.name, default_dest.clone());

                    if !options.direct_term {
                        default_dest.name.push_str("%%fixup");
                    }

                    let mut default_cmd = Execute::new();
                    default_cmd.with_if(ExecuteCondition::Score {
                        target: default_tracker.into(),
                        target_obj: OBJECTIVE.to_string(),
                        kind: ExecuteCondKind::Matches(cir::McRange::Between(0..=0)),
                    });
                    default_cmd.with_run(McFuncCall { id: default_dest });

                    this.cmds.push(default_cmd.into());
                }
                Terminator::Unreachable(Unreachable { .. }) => this.cmds.push(
                    Tellraw {
                        target: cir::Selector {
                            var: cir::SelectorVariable::AllPlayers,
                            args: Vec::new(),
                        }.into(),
                        message: "[{\"text\": \"ENTERED UNREACHABLE CODE\"}]".to_string(),
                    }
                    .into(),
                ),
                Terminator::Resume(_) => this.cmds.push(
                    Tellraw {
                        target: cir::Selector {
                            var: cir::SelectorVariable::AllPlayers,
                            args: Vec::new(),
                        }.into(),
                        message: "[{\"text\": \"OH NO EXCEPTION HANDLING TODO\"}]".to_string(),
                    }
                    .into(),
                ),
                term => todo!("terminator {:?}", term),
            }

            result.push(this);

            if !options.direct_term {
                for sub_block in result.iter_mut() {
                    sub_block.cmds.insert(
                        0,
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
        for mut holder in cmd_str.split_whitespace().filter(|s| s.contains('%')) {
            if holder.ends_with(',') {
                holder = &holder[..holder.len() - 1];
            }
            clobbers.insert(ScoreHolder::new(holder.to_string()).unwrap());
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
    let dest = ScoreHolder::from_local_name(dest.clone(), 4);

    assert_eq!(source0.len(), 1);
    assert_eq!(source1.len(), 1);
    assert_eq!(dest.len(), 1);

    let source0 = source0[0].clone();
    let source1 = source1[0].clone();
    let dest = dest[0].clone();

    cmds.extend(tmp.into_iter());
    cmds.push(assign(dest.clone(), source0));
    cmds.push(ScoreOp {
        target: dest.into(),
        target_obj: OBJECTIVE.to_string(),
        kind,
        source: Target::Uuid(source1),
        source_obj: OBJECTIVE.to_string(),
    }.into());
    cmds
}

pub fn push(target: ScoreHolder) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(assign(ptr(), stackptr()));
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(write_ptr(target));
    cmds.push(
        ScoreAdd {
            target: stackptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: 4,
        }
        .into(),
    );

    cmds
}

pub fn pop(target: ScoreHolder) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(
        ScoreAdd {
            target: stackptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: -4,
        }
        .into(),
    );
    cmds.push(assign(ptr(), stackptr()));
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(read_ptr(target));

    cmds
}

pub fn offset_of(element_types: &[Type], is_packed: bool, field: u32) -> usize {
    if is_packed {
        todo!("packed struct")
    }

    // FIXME: This doesn't account for alignment
    element_types[0..field as usize].iter().map(size_of_type).sum::<usize>()
}

pub fn size_of_type(ty: &Type) -> usize {
    match ty {
        // TODO: This should really be 1
        Type::IntegerType { bits: 1 } => 4,
        // TODO: This should really be 1
        Type::IntegerType { bits: 8 } => 4,
        Type::IntegerType { bits: 32 } => 4,
        Type::IntegerType { bits: 64 } => 8,
        Type::StructType {
            element_types,
            is_packed: false,
        } => {
            // FIXME: This doesn't account for alignment
            element_types.iter().map(size_of_type).sum::<usize>()
        }
        Type::NamedStructType { ty: Some(ty), .. } => {
            let ty = ty.upgrade().expect("Failed to upgrade type");

            let ty_read = ty.read().unwrap();
            size_of_type(&ty_read)
        }
        Type::VectorType { element_type, num_elements } => {
            size_of_type(element_type) * num_elements
        }
        Type::ArrayType { element_type, num_elements } => {
            // FIXME: This also doesn't account for alignment
            size_of_type(element_type) * num_elements
        }
        Type::PointerType { .. } => 4,
        Type::VoidType => 0,
        _ => todo!("size of type {:?}", ty),
    }
}

pub fn compile_alloca(
    Alloca {
        allocated_type,
        num_elements,
        dest,
        ..
    }: &Alloca,
) -> Vec<Command> {
    let type_size = size_of_type(allocated_type);

    let dest = ScoreHolder::from_local_name(dest.clone(), 4);
    assert_eq!(dest.len(), 1);
    let dest = dest[0].clone();

    let num_elements =
        if let Operand::ConstantOperand(Constant::Int { bits: 32, value }) = num_elements {
            *value as i32
        } else {
            todo!("{:?}", num_elements);
        };

    let mut cmds = Vec::new();

    cmds.push(assign(dest, stackptr()));
    cmds.push(
        ScoreAdd {
            target: stackptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: type_size as i32 * num_elements,
        }
        .into(),
    );

    cmds
}

// This whole thing could be optimized into a single command with a Predicate but... ugh
fn compile_ult_ule(lhs: ScoreHolder, rhs: ScoreHolder, dest: ScoreHolder, or_eq: bool) -> Vec<Command> {
    let mut cmds = Vec::new();

    // Reset the flag
    cmds.push(ScoreSet {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        score: 0,
    }.into());

    let mut check1 = Execute::new();
    check1.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((0..).into())
    });
    check1.with_if(ExecuteCondition::Score {
        target: rhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((0..).into())
    });
    check1.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Relation {
            relation: cir::Relation::LessThan,
            source: rhs.clone().into(),
            source_obj: OBJECTIVE.to_string(),
        }
    });
    check1.with_run(ScoreSet {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        score: 1,
    });
    cmds.push(check1.into());

    let mut check2 = Execute::new();
    check2.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((..=-1).into())
    });
    check2.with_if(ExecuteCondition::Score {
        target: rhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((..=-1).into())
    });
    check2.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Relation {
            relation: cir::Relation::LessThan,
            source: rhs.clone().into(),
            source_obj: OBJECTIVE.to_string(),
        }
    });
    check2.with_run(ScoreSet {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        score: 1,
    });
    cmds.push(check2.into());

    let mut check3 = Execute::new();
    check3.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    check3.with_if(ExecuteCondition::Score {
        target: rhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    check3.with_run(ScoreSet {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        score: 1,
    });
    cmds.push(check3.into());

    if or_eq {
        let mut eq_check = Execute::new();
        eq_check.with_if(ExecuteCondition::Score {
            target: lhs.into(),
            target_obj: OBJECTIVE.to_string(),
            kind: ExecuteCondKind::Relation {
                relation: cir::Relation::Eq,
                source: rhs.into(),
                source_obj: OBJECTIVE.to_string(),
            }
        });
        eq_check.with_run(ScoreSet {
            target: dest.into(),
            target_obj: OBJECTIVE.to_string(),
            score: 1,
        });
        cmds.push(eq_check.into());
    }

    cmds
}

fn compile_signed_cmp(target: ScoreHolder, source: ScoreHolder, dest: ScoreHolder, relation: cir::Relation, normal: bool) -> Command {
    let mut cmd = Execute::new();
    cmd.with_subcmd(ExecuteSubCmd::Store {
        is_success: true,
        kind: ExecuteStoreKind::Score {
            target: dest.into(),
            objective: OBJECTIVE.to_string(),
        },
    })
    .with_subcmd(ExecuteSubCmd::Condition {
        is_unless: !normal,
        cond: ExecuteCondition::Score {
            target: target.into(),
            target_obj: OBJECTIVE.to_string(),
            kind: ExecuteCondKind::Relation {
                relation,
                source: source.into(),
                source_obj: OBJECTIVE.to_string(),
            },
        },
    });

    cmd.into()
}

pub fn compile_instr(
    instr: &Instruction,
    parent: &Function,
    _options: &Options,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let result = match instr {
        // We use an empty stack
        Instruction::Alloca(alloca) => compile_alloca(alloca),
        Instruction::GetElementPtr(GetElementPtr {
            address,
            indices,
            in_bounds: true,
            dest,
            ..
        }) => {
            // TODO: Optimize this instruction for when the indices are constant

            let dest = ScoreHolder::from_local_name(dest.clone(), 4);
            let dest = dest[0].clone();

            let mut ty = address.get_type();
            let (mut cmds, addr) = eval_operand(address);

            let mut index_holders = Vec::new();
            for index in indices.iter() {
                let (tmp, index_holder) = eval_operand(index);
                cmds.extend(tmp);

                assert_eq!(index_holder.len(), 1);
                index_holders.push((index_holder[0].clone(), index));
            }

            assert_eq!(addr.len(), 1, "multiword address");
            let addr = addr[0].clone();

            cmds.push(assign(dest.clone(), addr));

            while !index_holders.is_empty() {
                let (index, raw_index) = index_holders.remove(0);

                match ty {
                    Type::PointerType { pointee_type, .. } => {
                        ty = (*pointee_type).clone();

                        // FIXME: This doesn't account for alignment
                        for _ in 0..size_of_type(&pointee_type) {
                            cmds.push(ScoreOp {
                                target: dest.clone().into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ScoreOpKind::AddAssign,
                                source: index.clone().into(),
                                source_obj: OBJECTIVE.into(),
                            }.into());
                        }
                    }
                    Type::NamedStructType { ty: struct_ty, .. } => {
                        // Have to end here, because we don't know the type until runtime
                        assert_eq!(index_holders, Vec::new());

                        let struct_ty = struct_ty.unwrap().upgrade().unwrap();
                        let struct_ty = struct_ty.read().unwrap();

                        let (element_types, is_packed) = if let Type::StructType { element_types, is_packed } = &*struct_ty {
                            (element_types, *is_packed)
                        } else {
                            todo!("{:?}", &*struct_ty)
                        };

                        let raw_index_maybe = eval_maybe_const(raw_index);

                        if !element_types.iter().all(|e| size_of_type(e) == 4) {
                            if let MaybeConst::Const(c) = raw_index_maybe {
                                cmds.push(ScoreAdd {
                                    target: dest.into(),
                                    target_obj: OBJECTIVE.to_string(),
                                    score: offset_of(element_types, is_packed, c as u32) as i32,
                                }.into())
                            } else {
                                todo!()
                            }
                        } else if let MaybeConst::Const(c) = raw_index_maybe {
                            cmds.push(ScoreAdd {
                                target: dest.into(),
                                target_obj: OBJECTIVE.to_string(),
                                score: offset_of(element_types, is_packed, c as u32) as i32,
                            }.into())
                        } else {
                            todo!()
                        }

                        break
                    }
                    Type::StructType { element_types, is_packed: false } => {
                        // Have to end here, because we don't know the type until runtime
                        assert_eq!(index_holders, Vec::new());

                        if !element_types.iter().all(|e| size_of_type(e) == 4) {
                            todo!("{:?}", element_types);
                        }

                        // FIXME: This should actually calculate the offset of the field
                        for _ in 0..4 {
                            cmds.push(
                                ScoreOp {
                                    target: dest.clone().into(),
                                    target_obj: OBJECTIVE.to_string(),
                                    kind: ScoreOpKind::AddAssign,
                                    source: index.clone().into(),
                                    source_obj: OBJECTIVE.to_string(),
                                }.into()
                            );
                        }
                        
                        break
                    }
                    _ => todo!("{:?}", ty)
                }
            }

            cmds
        }
        Instruction::Select(Select { condition, true_value, false_value, dest, .. }) => {
            let (mut cmds, true_val) = eval_operand(true_value);
            let (tmp, false_val) = eval_operand(false_value);
            cmds.extend(tmp);
            let (tmp, cond) = eval_operand(condition);
            cmds.extend(tmp);

            let dest = ScoreHolder::from_local_name(dest.clone(), size_of_type(&true_value.get_type()));

            if dest.len() != 1 {
                todo!()
            }
            if true_val.len() != 1 {
                todo!()
            }
            if false_val.len() != 1 {
                todo!()
            }
            if cond.len() != 1 {
                todo!()
            }
            let true_val = true_val[0].clone();
            let false_val = false_val[0].clone();
            let cond = cond[0].clone();
            let dest = dest[0].clone();

            let mut true_cmd = Execute::new();
            true_cmd.with_if(ExecuteCondition::Score {
                target: cond.clone().into(),
                target_obj: OBJECTIVE.to_string(),
                kind: ExecuteCondKind::Matches((1..=1).into())
            });
            true_cmd.with_run(assign(dest.clone(), true_val));
            cmds.push(true_cmd.into());

            let mut false_cmd = Execute::new();
            false_cmd.with_unless(ExecuteCondition::Score {
                target: cond.into(),
                target_obj: OBJECTIVE.to_string(),
                kind: ExecuteCondKind::Matches((1..=1).into())
            });
            false_cmd.with_run(assign(dest, false_val));
            cmds.push(false_cmd.into());

            cmds
        }
        Instruction::Store(Store { address, value, .. }) => {
            let (mut cmds, addr) = eval_operand(address);

            assert_eq!(addr.len(), 1, "multiword addr {:?}", address);

            let addr = addr[0].clone();

            // If we're directly storing a constant,
            // we can skip writing to a temporary value
            let write_cmds = match eval_maybe_const(value) {
                MaybeConst::Const(value) => vec![write_ptr_const(value)],
                MaybeConst::NonConst(eval_cmds, ids) => {
                    cmds.extend(eval_cmds);

                    ids.into_iter().map(write_ptr).collect()
                }
            };

            for (idx, write_cmd) in write_cmds.into_iter().enumerate() {
                cmds.push(assign(ptr(), addr.clone()));
                cmds.push(
                    ScoreAdd {
                        target: ptr().into(),
                        target_obj: OBJECTIVE.to_string(),
                        score: idx as i32,
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
            }

            cmds
        }
        Instruction::Load(Load { dest, address, .. }) => {
            let pointee_type = if let Type::PointerType { pointee_type, .. } = address.get_type() {
                pointee_type
            } else {
                unreachable!()
            };

            let (mut cmds, addr) = eval_operand(address);

            assert_eq!(addr.len(), 1, "multiword address {:?}", address);
            let addr = addr[0].clone();

            cmds.push(assign(ptr(), addr));
            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:setptr"),
                }
                .into(),
            );

            let pointee_size = size_of_type(&pointee_type);

            if pointee_size != 4 {
                todo!("multiword pointee {:?}", pointee_size)
            }

            let dest = ScoreHolder::from_local_name(dest.clone(), pointee_size);
            let dest = dest[0].clone();

            cmds.push(read_ptr(dest));

            cmds
        }
        Instruction::Add(Add {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::AddAssign),
        Instruction::Sub(Sub {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::SubAssign),
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
            let (mut cmds, target) = eval_operand(operand0);
            let (tmp_cmds, source) = eval_operand(operand1);
            cmds.extend(tmp_cmds);

            assert_eq!(target.len(), 1, "multiword operand0 {:?}", operand0);
            assert_eq!(source.len(), 1, "multiword operand1 {:?}", operand1);

            let target = target[0].clone();
            let source = source[0].clone();

            // Technically it's an i1, but there's probably no harm in this
            let dest = ScoreHolder::from_local_name(dest.clone(), 4);
            let dest = dest[0].clone();

            let signed_cmp = |rel, normal| {
                compile_signed_cmp(target.clone(), source.clone(), dest.clone(), rel, normal)
            };

            match predicate {
                IntPredicate::SGE => cmds.push(signed_cmp(cir::Relation::GreaterThanEq, true)),
                IntPredicate::SGT => cmds.push(signed_cmp(cir::Relation::GreaterThan, true)),
                IntPredicate::SLT => cmds.push(signed_cmp(cir::Relation::LessThan, true)),
                IntPredicate::SLE => cmds.push(signed_cmp(cir::Relation::LessThanEq, true)),
                IntPredicate::EQ => cmds.push(signed_cmp(cir::Relation::Eq, true)),
                IntPredicate::NE => cmds.push(signed_cmp(cir::Relation::Eq, false)),
                IntPredicate::ULT => cmds.extend(compile_ult_ule(target, source, dest, false)),
                IntPredicate::ULE => cmds.extend(compile_ult_ule(target, source, dest, true)),
                p => todo!("predicate {:?}", p),
            }

            cmds
        }
        Instruction::Phi(Phi { incoming_values, dest, to_type, .. }) => {
            let to_type_size = size_of_type(to_type);

            let dst = ScoreHolder::from_local_name(dest.clone(), to_type_size);

            let mut cmds = Vec::new();

            for (value, block) in incoming_values {
                let block_idx = parent.basic_blocks.iter().enumerate().find(|(_, b)| &b.name == block).unwrap().0 as i32;

                let (tmp, val) = eval_operand(value);
                cmds.extend(tmp);

                assert_eq!(val.len(), dst.len());

                for (val_word, dst_word) in val.into_iter().zip(dst.iter().cloned()) {
                    let mut cmd = Execute::new();
                    cmd.with_if(ExecuteCondition::Score {
                        target: ScoreHolder::new("%phi".to_string()).unwrap().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((block_idx..=block_idx).into()),
                    });
                    cmd.with_run(assign(dst_word, val_word));
                    cmds.push(cmd.into());
                }
            }

            cmds
        }
        Instruction::Call(call) => return compile_call(call),
        Instruction::BitCast(BitCast { operand, dest, to_type, .. }) => {
            let (mut cmds, source) = eval_operand(operand);

            if source.len() != 1 {
                todo!("multiword source {:?}", source);
            }

            let source = source[0].clone();

            let dest = ScoreHolder::from_local_name(dest.clone(), size_of_type(to_type));

            if dest.len() != 1 {
                todo!("multiword dest {:?}", dest);
            }

            let dest = dest[0].clone();

            cmds.push(assign(dest, source));

            cmds
        }
        Instruction::Trunc(Trunc { operand, to_type: Type::IntegerType { bits: 1 }, dest, .. }) => {
            let (mut cmds, op) = eval_operand(operand);

            assert_eq!(op.len(), 1);
            let op = op[0].clone();
            let dest = ScoreHolder::from_local_name(dest.clone(), 4)[0].clone();

            cmds.push(assign(dest.clone(), op));
            cmds.push(ScoreOp {
                target: dest.clone().into(),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::MulAssign,
                source: ScoreHolder::new("%%31BITSHIFT".to_string()).unwrap().into(),
                source_obj: OBJECTIVE.to_string(),
            }.into());
            cmds.push(ScoreOp {
                target: dest.into(),
                target_obj: OBJECTIVE.to_string(),
                kind: ScoreOpKind::DivAssign,
                source: ScoreHolder::new("%%31BITSHIFT".to_string()).unwrap().into(),
                source_obj: OBJECTIVE.to_string(),
            }.into());

            cmds
        }
        Instruction::ExtractValue(ExtractValue {
            aggregate,
            indices,
            dest,
            ..
        }) => {
            let (mut cmds, aggr) = eval_operand(aggregate);

            if indices.len() != 1 {
                todo!("{:?}", indices)
            }

            if let Type::StructType { element_types, is_packed } = aggregate.get_type() {
                let size = size_of_type(&element_types[indices[0] as usize]);
                
                let offset = offset_of(&element_types, is_packed, indices[0]);

                let dest = ScoreHolder::from_local_name(dest.clone(), size);

                if size != 4 {
                    todo!()
                }

                if dest.len() != 1 {
                    todo!()
                }

                let dest = dest[0].clone();

                if offset % 4 != 0 {
                    todo!()
                }

                // FIXME: If the element is smaller than a register this won't work
                cmds.push(assign(dest, aggr[offset as usize / 4].clone()))
            } else {
                todo!("{:?}", aggregate)
            }

            cmds
        }
        Instruction::InsertValue(InsertValue {
            aggregate,
            element,
            indices,
            dest,
            ..
        }) => {
            let aggr_size = size_of_type(&aggregate.get_type());

            let (mut cmds, aggr) = eval_operand(aggregate);
            let (tmp, elem) = eval_operand(element);
            cmds.extend(tmp);


            if elem.len() != 1 {
                todo!("multiword elem {:?}", element);
            }
            let elem = elem[0].clone();

            let (element_types, is_packed) = if let Operand::ConstantOperand(Constant::Undef(Type::StructType { element_types, is_packed })) = aggregate {
                (element_types, *is_packed)
            } else if let Operand::LocalOperand { ty: Type::StructType { element_types, is_packed }, .. } = aggregate {
                (element_types, *is_packed)
            } else {
                todo!("{:?}", aggregate)
            };

            if indices.len() != 1 {
                todo!("indices {:?}", indices)
            }
            let index = indices[0];

            let offset = offset_of(element_types, is_packed, index);

            if offset % 4 != 0 {
                todo!("{:?}", offset);
            }

            let dest = ScoreHolder::from_local_name(dest.clone(), aggr_size);

            let insert_idx = offset / 4;

            assert_eq!(dest.len(), aggr.len());

            let mut cmds = Vec::new();

            for (dest_word, aggr_word) in dest.iter().zip(aggr.into_iter()) {
                cmds.push(assign(dest_word.clone(), aggr_word.clone()));
            }

            // FIXME: This will break if `elem` is ever smaller than 4
            cmds.push(assign(dest[insert_idx].clone(), elem));

            cmds
        }
        Instruction::ZExt(ZExt { operand, to_type, dest, .. }) => {
            let (mut cmds, op) = eval_operand(operand);

            if op.len() != 1 {
                todo!()
            }

            let dst = ScoreHolder::from_local_name(dest.clone(), size_of_type(to_type));

            if dst.len() != 1 {
                todo!()
            }

            cmds.push(assign(dst[0].clone(), op[0].clone()));

            cmds
        }
        _ => todo!("instruction {:?}", instr),
    };

    (result, None)
}

pub enum MaybeConst {
    Const(i32),
    NonConst(Vec<Command>, Vec<ScoreHolder>),
}

impl MaybeConst {
    pub fn force_eval(self) -> (Vec<Command>, Vec<ScoreHolder>) {
        match self {
            MaybeConst::Const(score) => {
                let target = ScoreHolder::new(format!("%temp{}", get_unique_num())).unwrap();
                (
                    vec![ScoreSet {
                        target: target.clone().into(),
                        target_obj: OBJECTIVE.to_string(),
                        score,
                    }
                    .into()],
                    vec![target],
                )
            }
            MaybeConst::NonConst(cmds, id) => (cmds, id),
        }
    }
}

pub fn eval_constant(con: &Constant) -> MaybeConst {
    match con {
        Constant::GlobalReference { name, .. } => {
            let temp = name.to_string();
            let holder = ScoreHolder::new(format!("%@{}", &temp[1..temp.len() - 1])).unwrap();
            MaybeConst::NonConst(vec![], vec![holder])
        }
        Constant::Int { bits: 8, value } => MaybeConst::Const(*value as i32),
        Constant::Int { bits: 32, value } => MaybeConst::Const(*value as i32),
        Constant::Int { bits: 64, value } => {
            // TODO: I mean it's *const* but not convenient...
            let num = get_unique_num();

            let lo_word = ScoreHolder::new(format!("%temp{}%0", num)).unwrap();
            let hi_word = ScoreHolder::new(format!("%temp{}%1", num)).unwrap();

            let cmds = vec![ScoreSet { 
                target: lo_word.clone().into(),
                target_obj: OBJECTIVE.into(),
                score: *value as i32,
            }.into(),
            ScoreSet {
                target: hi_word.clone().into(),
                target_obj: OBJECTIVE.into(),
                score: (*value >> 32) as i32,
            }.into()];

            MaybeConst::NonConst(cmds, vec![lo_word, hi_word])
        }
        Constant::BitCast(bitcast) => eval_constant(&bitcast.operand),
        Constant::Undef(ty) => {
            // TODO: This can literally be *anything* you want it to be

            let len = size_of_type(ty);

            if len % 4 != 0 {
                todo!("type with size ({}) not a multiple of 4: {:?}", len, ty);
            }

            let num = get_unique_num();

            let holders = (0..(len / 4))
                .map(|idx| ScoreHolder::new(format!("%temp{}%{}", num, idx)).unwrap())
                .collect();

            MaybeConst::NonConst(Vec::new(), holders)
        },
        _ => todo!("evaluate constant {:?}", con),
    }
}

pub fn eval_maybe_const(op: &Operand) -> MaybeConst {
    match op {
        Operand::LocalOperand { name, ty } => {
            let len = size_of_type(ty);

            if len % 4 != 0 {
                todo!("type with size not a multiple of 4: {:?}", ty)
            }

            let holders = ScoreHolder::from_local_name(name.clone(), len);

            MaybeConst::NonConst(Vec::new(), holders)
        }
        Operand::ConstantOperand(con) => eval_constant(con),
        _ => todo!("operand {:?}", op),
    }
}

pub fn eval_operand(op: &Operand) -> (Vec<Command>, Vec<ScoreHolder>) {
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

fn get_unique_holder() -> ScoreHolder {
    ScoreHolder::new(format!("%temp{}", get_unique_num())).unwrap()
}
