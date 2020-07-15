use crate::cir::FuncCall as McFuncCall;
use crate::cir::Function as McFunction;
use crate::cir::FunctionId as McFuncId;
use crate::cir::{
    self, Command, Data, DataKind, DataTarget, Execute, ExecuteCondKind, ExecuteCondition,
    ExecuteStoreKind, ExecuteSubCmd, ScoreAdd, ScoreGet, ScoreHolder, ScoreOp, ScoreOpKind,
    ScoreSet, SetBlock, SetBlockKind, Target, Tellraw,
};
use crate::interpreter::InterpError;
use either::Either;
use lazy_static::lazy_static;
use llvm_ir::constant::BitCast as BitCastConst;
use llvm_ir::constant::GetElementPtr as GetElementPtrConst;
use llvm_ir::debugloc::HasDebugLoc;
use llvm_ir::instruction::{
    Add, Alloca, And, BitCast, Call, ExtractElement, ExtractValue, GetElementPtr, ICmp,
    InsertValue, IntToPtr, LShr, Load, Mul, Or, Phi, PtrToInt, Select, Shl, ShuffleVector, Store,
    Sub, Trunc, Xor, ZExt,
};
use llvm_ir::module::GlobalVariable;
use llvm_ir::terminator::{Br, CondBr, Ret, Switch, Unreachable};
use llvm_ir::types::Typed;
use llvm_ir::{
    Constant, Function, Instruction, IntPredicate, Module, Name, Operand, Terminator, Type,
};
use std::alloc::Layout;
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
        target: cir::Selector {
            var: cir::SelectorVariable::AllPlayers,
            args: Vec::new(),
        }
        .into(),
        message: cir::TextBuilder::new()
            .append_text(format!("entered block {}", location))
            .build(),
    }
    .into()
}

// %ptr, %x, %y, %z, %param<X> are caller-saved registers
// all other registers are callee-saved
// %stackptr is... weird
// %temp<X> are... weird

// `intrinsic:setptr` sets the pointer to the value in `%ptr` for objective `rust`

/// This reads from %ptr, does a %setptr, and then gets either a halfword or a byte
///
/// ... and clobbers %param0%0 in the process
pub fn read_ptr_small(dest: ScoreHolder, is_halfword: bool) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(read_ptr(param(0, 0)));

    if is_halfword {
        todo!("halfword read")
    } else {
        // %param1%0 = shifts[ptr % 4]

        cmds.push(assign(param(1, 0), ptr()));
        cmds.push(
            ScoreOp {
                target: param(1, 0).into(),
                target_obj: OBJECTIVE.into(),
                kind: ScoreOpKind::ModAssign,
                source: ScoreHolder::new("%%FOUR".into()).unwrap().into(),
                source_obj: OBJECTIVE.into(),
            }
            .into(),
        );

        let shifts = [3, 2, 1, 0];
        for (idx, byte_amt) in shifts.iter().copied().enumerate() {
            let shift = 1 << (8 * byte_amt);

            let mut exec = Execute::new();
            exec.with_if(ExecuteCondition::Score {
                target: param(1, 0).into(),
                target_obj: OBJECTIVE.into(),
                kind: ExecuteCondKind::Matches((idx as i32..=idx as i32).into()),
            });
            exec.with_run(ScoreSet {
                target: param(1, 0).into(),
                target_obj: OBJECTIVE.into(),
                score: shift,
            });
            cmds.push(exec.into());
        }

        cmds.push(
            ScoreOp {
                target: param(0, 0).into(),
                target_obj: OBJECTIVE.into(),
                kind: ScoreOpKind::MulAssign,
                source: param(1, 0).into(),
                source_obj: OBJECTIVE.into(),
            }
            .into(),
        );

        cmds.push(
            ScoreSet {
                target: param(1, 0).into(),
                target_obj: OBJECTIVE.into(),
                score: 24,
            }
            .into(),
        );

        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:lshr"),
            }
            .into(),
        );

        cmds.push(assign(dest, param(0, 0)));

        cmds
    }
}

/// Reads the current pointer location into some target for objective `rust`
pub fn read_ptr(target: ScoreHolder) -> Command {
    let mut exec = Execute::new();
    exec.with_at(
        cir::Selector {
            var: cir::SelectorVariable::AllEntities,
            args: vec![cir::SelectorArg("tag=ptr".to_string())],
        }
        .into(),
    );
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
    }
    .into()
}

pub fn get_index(x: i32, y: i32, z: i32) -> Result<i32, InterpError> {
    if 0 <= x && x < 16 && 0 <= y && y < 16 && 0 <= z && z < 16 {
        Ok((x * 16 * 16 + y * 16 + z) * 4)
    } else {
        Err(InterpError::OutOfBoundsAccess(x, y, z))
    }
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
    exec.with_at(
        cir::Selector {
            var: cir::SelectorVariable::AllEntities,
            args: vec![cir::SelectorArg("tag=ptr".to_string())],
        }
        .into(),
    );
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
    exec.with_at(
        cir::Selector {
            var: cir::SelectorVariable::AllEntities,
            args: vec![cir::SelectorArg("tag=ptr".to_string())],
        }
        .into(),
    );
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
        let mut cmd_idx = 0;
        while cmd_idx < funcs[func_idx].cmds.len() {
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
                    funcs[func_idx]
                        .cmds
                        .insert(cmd_idx, Command::Comment(format!("{}", id)));
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

                        funcs[func_idx]
                            .cmds
                            .insert(cmd_idx, Command::Comment(format!("{}", id)));
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

            cmd_idx += 1;
        }
    }
}

pub fn compile_module(module: &Module, options: &Options) -> Vec<McFunction> {
    let (mut init_cmds, mut globals) =
        compile_global_var_init(&module.global_vars, &module.functions);

    for (idx, func) in module.functions.iter().enumerate() {
        let name = Box::leak(Box::new(Name::Name(func.name.clone())));
        globals.insert(name, (idx as u32 + 1, None));
    }

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

    init_cmds.push(
        ScoreSet {
            target: stackbaseptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: 0,
        }
        .into(),
    );

    let init_func = McFunction {
        id: McFuncId::new("init"),
        cmds: init_cmds,
    };

    let mut clobber_list = HashMap::<String, BTreeSet<ScoreHolder>>::new();
    let mut funcs = vec![init_func];

    let mut after_blocks = Vec::new();

    for (mc_funcs, mut clobbers) in module
        .functions
        .iter()
        .map(|f| compile_function(f, &globals, options))
    {
        clobbers.remove(&stackptr());
        clobbers.remove(&ptr());
        clobbers.remove(&ScoreHolder::new("%%fixup".to_string()).unwrap());
        clobbers.remove(&ScoreHolder::new("%phi".to_string()).unwrap());
        clobbers = clobbers
            .into_iter()
            .filter(|e| !e.as_ref().starts_with("%return%") && !e.as_ref().contains("%%fixup"))
            .collect();

        for McFunction { id, .. } in mc_funcs.iter() {
            clobber_list.insert(id.name.clone(), clobbers.clone());
        }

        let mut f = mc_funcs.into_iter();
        funcs.push(f.next().unwrap());
        after_blocks.extend(f);
    }

    funcs.extend(after_blocks);

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

            let message = cir::TextBuilder::new()
                .append_text(format!("%stackptr at start of {} is ", func.id))
                .append_score(stackptr(), OBJECTIVE.into(), None)
                .build();

            let save_code = std::iter::once(
                Tellraw {
                    target: cir::Selector {
                        var: cir::SelectorVariable::AllPlayers,
                        args: vec![],
                    }
                    .into(),
                    message,
                }
                .into(),
            )
            .chain(
                clobber_list
                    .get(&func.id.name)
                    .unwrap()
                    .iter()
                    .cloned()
                    .chain(std::iter::once(stackbaseptr()))
                    .map(push)
                    .flatten()
                    .chain(std::iter::once(base_set)),
            );

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
        cmds: vec![
            McFuncCall {
                id: McFuncId::new("init"),
            }
            .into(),
            SetBlock {
                pos: format!("-2 1 {}", main_idx),
                block: "minecraft:redstone_block".to_string(),
                kind: SetBlockKind::Replace,
            }
            .into(),
        ],
    });

    let mut all_clobbers = BTreeSet::new();
    for c in clobber_list.values() {
        all_clobbers.extend(c);
    }

    funcs[0].cmds.splice(
        0..0,
        all_clobbers.iter().map(|c| {
            ScoreSet {
                target: (*c).clone().into(),
                target_obj: OBJECTIVE.into(),
                score: 0,
            }
            .into()
        }),
    );

    funcs
}

fn getelementptr_const(
    GetElementPtrConst {
        address,
        indices,
        in_bounds,
    }: &GetElementPtrConst,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> u32 {
    if !in_bounds {
        todo!("not inbounds constant getelementptr")
    }

    if let Constant::GlobalReference { name, ty } = address {
        let mut offset = globals
            .get(&name)
            .unwrap_or_else(|| panic!("couldn't find global {:?}", name))
            .0;
        let mut ty = ty;

        for index in &indices[1..] {
            let index = if let Constant::Int { bits: 32, value } = index {
                *value as i32
            } else {
                unreachable!()
            };

            match ty {
                Type::StructType {
                    element_types,
                    is_packed,
                } => {
                    ty = &element_types[index as usize];
                    offset += offset_of(element_types, *is_packed, index as u32) as u32;
                }
                Type::ArrayType {
                    element_type,
                    num_elements,
                } => {
                    let elem_size = type_layout(element_type).pad_to_align().size();

                    ty = element_type;
                    offset += elem_size as u32 * *num_elements as u32;
                }
                _ => todo!("{:?}", ty),
            }
        }

        println!("next type would be {:?}", ty);

        offset
    } else {
        todo!("{:?}", address)
    }
}

type GlobalVarList<'a> = HashMap<&'a Name, (u32, Option<Constant>)>;

fn compile_global_var_init<'a>(
    vars: &'a [GlobalVariable],
    funcs: &[Function],
) -> (Vec<Command>, GlobalVarList<'a>) {
    let mut globals = global_var_layout(vars);
    let mut cmds = Vec::new();

    for var in vars {
        let (tmp, value) = one_global_var_init(var, &globals, funcs);
        cmds.extend(tmp);
        assert_eq!(globals.get_mut(&var.name).unwrap().1.replace(value), None);
    }

    // Currently used constants:
    // %%31BITSHIFT
    // %%FOUR
    // %%SIXTEEN
    // %%256
    // %%2
    // %%1
    // %%65536

    // TODO: This needs a better system
    static CONSTANTS: &[(&str, i32)] = &[
        ("%%31BITSHIFT", 1 << 31),
        ("%%FOUR", 4),
        ("%%16777216", 16777216),
        ("%%SIXTEEN", 16),
        ("%%65536", 65536),
        ("%%256", 256),
        ("%%2", 2),
        ("%%1", 1),
        ("%%-1", -1),
    ];

    for (name, value) in CONSTANTS {
        cmds.push(
            ScoreSet {
                target: ScoreHolder::new(name.to_string()).unwrap().into(),
                target_obj: OBJECTIVE.to_string(),
                score: *value,
            }
            .into(),
        )
    }

    (cmds, globals)
}

fn init_array_zeroed(start_addr: u32, element_type: &Type, num_elements: usize) -> Vec<Command> {
    if !matches!(element_type, Type::IntegerType { bits: 8 }) {
        todo!("{:?}", element_type)
    }

    let elements = vec![Constant::Int { bits: 8, value: 0 }; num_elements];

    init_array(start_addr, element_type, &elements)
}

fn init_array(start_addr: u32, element_type: &Type, elements: &[Constant]) -> Vec<Command> {
    if !matches!(element_type, Type::IntegerType { bits: 8 }) {
        todo!("{:?}", element_type)
    }

    let mut elements = elements
        .iter()
        .map(|e| {
            if let Constant::Int { bits: 8, value } = e {
                *value as u8
            } else {
                panic!()
            }
        })
        .collect::<Vec<_>>();
    for _ in 0..(4 - (elements.len() % 4)) {
        elements.push(0);
    }

    elements
        .chunks_exact(4)
        .enumerate()
        .map(|(word_idx, ch)| {
            let value = i32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]);
            set_memory(value, start_addr as i32 + 4 * word_idx as i32)
        })
        .collect()
}

fn init_struct_zeroed(
    start_addr: u32,
    element_types: &[Type],
    is_packed: bool,
    functions: &[Function],
) -> Vec<Command> {
    let values = element_types
        .iter()
        .map(|ty| match ty {
            Type::ArrayType {
                element_type: ty,
                num_elements: 0,
            } if **ty == Type::IntegerType { bits: 8 } => Constant::Array {
                element_type: Type::IntegerType { bits: 8 },
                elements: Vec::new(),
            },
            _ => todo!("{:?}", ty),
        })
        .collect::<Vec<_>>();

    init_struct(start_addr, &values, is_packed, functions)
}

fn init_struct(
    start_addr: u32,
    values: &[Constant],
    is_packed: bool,
    functions: &[Function],
) -> Vec<Command> {
    let mut cmds = Vec::new();

    if is_packed {
        if let [Constant::Array { elements, .. }] = values {
            if elements.is_empty() {
                // do nothing
            } else {
                todo!("{:?}", elements)
            }
        } else {
            todo!("{:?}", values)
        }
    } else {
        for (value_idx, value) in values.iter().enumerate() {
            if let Constant::Int { bits: 32, value } = value {
                cmds.push(set_memory(
                    *value as i32,
                    start_addr as i32 + 4 * value_idx as i32,
                ));
            } else if let Constant::GlobalReference { name, .. } = value {
                let idx = 1 + functions
                    .iter()
                    .enumerate()
                    .find(|(_, f)| format!("\"{}\"", f.name) == name.to_string())
                    .unwrap_or_else(|| panic!("failed to resolve function {}", name))
                    .0;

                cmds.push(set_memory(
                    idx as i32,
                    start_addr as i32 + 4 * value_idx as i32,
                ))
            } else if let Constant::BitCast(bc) = value {
                if let BitCastConst {
                    operand: Constant::GlobalReference { name, .. },
                    to_type: Type::PointerType { .. },
                } = &**bc
                {
                    let idx = 1 + functions
                        .iter()
                        .enumerate()
                        .find(|(_, f)| format!("\"{}\"", f.name) == name.to_string())
                        .unwrap_or_else(|| panic!("failed to resolve function {}", name))
                        .0;

                    cmds.push(set_memory(
                        idx as i32,
                        start_addr as i32 + 4 * value_idx as i32,
                    ))
                } else {
                    todo!("{:?}", value)
                }
            } else {
                todo!("{:?}", value)
            }
        }
    }

    cmds
}

fn global_var_layout(v: &[GlobalVariable]) -> HashMap<&Name, (u32, Option<Constant>)> {
    let mut result = HashMap::new();
    for v in v.iter() {
        let pointee_type = if let Type::PointerType { pointee_type, .. } = &v.ty {
            pointee_type
        } else {
            unreachable!()
        };

        let start = get_alloc(type_layout(pointee_type).size() as u32);
        result.insert(&v.name, (start, None));
    }
    result
}

fn one_global_var_init(
    v: &GlobalVariable,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
    functions: &[Function],
) -> (Vec<Command>, Constant) {
    if matches!(v.name, Name::Number(_)) {
        todo!()
    }

    let start = globals.get(&v.name).unwrap().0;

    println!("evaluating {}", v.name);

    let temp = v.name.to_string();
    let target = ScoreHolder::new_lossy(format!("%@{}", &temp[1..temp.len() - 1]));

    match &v.ty {
        // I'm pretty sure it's *always* a pointer...
        Type::PointerType { .. } => {
            let mut addr = start;

            let mut cmds = Vec::new();

            cmds.push(
                ScoreSet {
                    target: target.into(),
                    target_obj: OBJECTIVE.into(),
                    score: start as i32,
                }
                .into(),
            );

            let mut init = v.initializer.clone().unwrap();

            if let Constant::BitCast(bc) = init {
                let BitCastConst { operand, .. } = *bc;

                init = operand;
            }

            match init {
                Constant::AggregateZero(Type::StructType {
                    element_types,
                    is_packed,
                }) => cmds.extend(init_struct_zeroed(
                    addr as u32,
                    &element_types,
                    is_packed,
                    functions,
                )),
                Constant::Struct {
                    values,
                    is_packed: true,
                    ..
                } => {
                    if let [Constant::Array {
                        element_type: Type::IntegerType { bits: 8 },
                        elements,
                    }] = &values[..]
                    {
                        cmds.extend(init_array(
                            start as u32,
                            &Type::IntegerType { bits: 8 },
                            elements,
                        ))
                    // FIXME: this should actually increment addr but i don't feel like it
                    } else {
                        for mut v in values {
                            if let Constant::BitCast(bc) = v {
                                v = bc.operand;
                            }

                            match v {
                                Constant::GlobalReference { name, .. } => {
                                    let value = globals.get(&name).unwrap().0;

                                    cmds.push(set_memory(value as i32, addr as i32));

                                    // Width of a pointer
                                    addr += 4;
                                }
                                Constant::GetElementPtr(g) => {
                                    let value = getelementptr_const(&*g, globals);
                                    cmds.push(set_memory(value as i32, addr as i32));

                                    // Width of a pointer
                                    addr += 4;
                                }
                                Constant::AggregateZero(Type::ArrayType {
                                    element_type,
                                    num_elements,
                                }) => {
                                    let tmp = init_array_zeroed(addr, &element_type, num_elements);
                                    addr += 4 * tmp.len() as u32;
                                    cmds.extend(tmp);
                                }
                                Constant::Array {
                                    element_type,
                                    elements,
                                } => {
                                    let tmp = init_array(addr, &element_type, &elements);
                                    addr += 4 * tmp.len() as u32;
                                    cmds.extend(tmp);
                                }
                                _ => todo!("{:?}", v),
                            }
                        }
                    }
                }
                Constant::Struct {
                    values, is_packed, ..
                } => {
                    cmds.extend(init_struct(addr as u32, &values, is_packed, functions));
                }
                Constant::Int { bits: 32, value } => {
                    cmds.push(set_memory(value as i32, addr as i32));
                }
                Constant::Array {
                    element_type,
                    elements,
                } => {
                    // FIXME: this should actually increment addr but i don't feel like it
                    cmds.extend(init_array(addr as u32, &element_type, &elements));
                }
                init => {
                    println!("Type: {:?}", v.ty);
                    println!("Initializer: {:?}", init);
                    todo!()
                }
            }

            (cmds, v.initializer.clone().unwrap())
        }
        _ => todo!("{:?}", v.ty),
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
fn compile_memcpy(
    arguments: &[(Operand, Vec<llvm_ir::function::ParameterAttribute>)],
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> Vec<Command> {
    if let [(dest, _), (src, _), (len, _), (volatile, _)] = &arguments[..] {
        let (mut cmds, src1) = eval_operand(src, globals);
        let (tmp, dest1) = eval_operand(dest, globals);
        cmds.extend(tmp);
        let (tmp, len1) = eval_operand(len, globals);
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

        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:memcpy"),
            }
            .into(),
        );

        cmds
    } else {
        panic!("{:?}", arguments);
    }
}

fn setup_arguments(
    arguments: &[(Operand, Vec<llvm_ir::function::Attribute>)],
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> Vec<Command> {
    let mut before_cmds = Vec::new();

    // Set arguments
    for (idx, (arg, _attrs)) in arguments.iter().enumerate() {
        match eval_maybe_const(arg, globals) {
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

    before_cmds
}

fn compile_call(
    Call {
        function,
        arguments,
        dest,
        ..
    }: &Call,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let function = match function {
        Either::Left(asm) => todo!("inline assembly {:?}", asm),
        Either::Right(operand) => operand,
    };

    if let Operand::ConstantOperand(Constant::GlobalReference {
        name: Name::Name(name),
        ty:
            Type::FuncType {
                result_type,
                is_var_arg: false,
                ..
            },
    }) = function
    {
        let dest_size = type_layout(result_type).size();
        let dest = dest
            .clone()
            .map(|d| ScoreHolder::from_local_name(d, dest_size));

        match name.as_str() {
            "llvm.assume" => {
                assert_eq!(arguments.len(), 1);
                assert!(dest.is_none());
                println!("Assumption {:?}", arguments[0]);

                let cmds = vec![Command::Comment(format!("assumption {:?}", arguments[0]))];
                (cmds, None)
            }
            "print_raw" => {
                assert_eq!(arguments.len(), 2);

                let ptr = arguments[0].clone();
                let len = arguments[1].clone();

                let len = if let Operand::ConstantOperand(Constant::Int { bits: 32, value }) = len.0
                {
                    value as u32
                } else {
                    todo!("{:?}", len)
                };

                // TODO: this is so so terribly awful
                let addr = if let Operand::ConstantOperand(Constant::GetElementPtr(g)) = &ptr.0 {
                    let GetElementPtrConst {
                        address,
                        indices,
                        in_bounds: _in_bounds,
                    } = &**g;

                    let addr = if let Constant::GlobalReference { name, .. } = address {
                        name
                    } else {
                        todo!("{:?}", address)
                    };

                    if !matches!(
                        indices[..],
                        [
                            Constant::Int { bits: 32, value: 0 },
                            Constant::Int { bits: 32, value: 0 },
                            Constant::Int { bits: 32, value: 0 }
                        ]
                    ) {
                        todo!("{:?}", indices)
                    }

                    addr
                } else {
                    todo!("{:?}", ptr)
                };

                let data = &globals.get(addr).unwrap().1;

                let data = if let Constant::Struct {
                    values,
                    is_packed: true,
                    ..
                } = data.as_ref().unwrap()
                {
                    if let [Constant::Array {
                        element_type: Type::IntegerType { bits: 8 },
                        elements,
                    }] = &values[..]
                    {
                        elements
                    } else {
                        todo!("{:?}", values)
                    }
                } else {
                    todo!("{:?}", data)
                };

                let data = data[..len as usize]
                    .iter()
                    .map(|d| {
                        if let Constant::Int { bits: 8, value } = d {
                            *value as u8
                        } else {
                            unreachable!()
                        }
                    })
                    .collect::<Vec<u8>>();

                let text = std::str::from_utf8(&data).unwrap();

                if text.contains('"') {
                    todo!("{:?}", text)
                }

                (
                    vec![Tellraw {
                        target: cir::Selector {
                            var: cir::SelectorVariable::AllPlayers,
                            args: Vec::new(),
                        }
                        .into(),
                        message: cir::TextBuilder::new().append_text(text.into()).build(),
                    }
                    .into()],
                    None,
                )
            }
            "print" => {
                assert_eq!(arguments.len(), 1);

                assert!(dest.is_none());

                let (mut cmds, name) = eval_operand(&arguments[0].0, globals);

                let name = name[0].clone();

                cmds.push(
                    Tellraw {
                        target: Target::Selector(cir::Selector {
                            var: cir::SelectorVariable::AllPlayers,
                            args: vec![],
                        }),
                        message: cir::TextBuilder::new()
                            .append_score(name, OBJECTIVE.into(), None)
                            .build(),
                    }
                    .into(),
                );

                (cmds, None)
            }
            n @ "turtle_x" | n @ "turtle_y" | n @ "turtle_z" => {
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
                let (mut cmds, pos) = eval_operand(&arguments[0].0, globals);

                cmds.insert(0, Command::Comment(format!("call to {}", n)));

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

                let mc_block =
                    if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0, globals) {
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

                let cmds = vec![
                    Command::Comment("call to turtle_set".to_string()),
                    cmd.into(),
                ];

                (cmds, None)
            }
            "turtle_check" => {
                assert_eq!(arguments.len(), 1);

                let dest = dest.as_ref().expect("turtle_check should return a value");
                assert_eq!(dest.len(), 1);
                let dest = dest[0].clone();

                let mc_block =
                    if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0, globals) {
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

                let dest = dest
                    .as_ref()
                    .expect("turtle_get_char should return a value");

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
                valid_chars.push(b'(');
                valid_chars.push(b')');
                valid_chars.push(b'{');
                valid_chars.push(b'}');

                for c in valid_chars {
                    let is_white = c == b'H' || c == b'Q' || c == b'S' || c == b')' || c == b'(';

                    let mut block = if is_white {
                        "minecraft:white_wall_banner"
                    } else {
                        "minecraft:light_blue_wall_banner"
                    }
                    .to_string();

                    block.push_str(&format!(
                        "{{ CustomName: \"{{\\\"text\\\":\\\"{}\\\"}}\"}}",
                        char::from(c)
                    ));

                    let mut cmd = Execute::new();
                    cmd.with_at(
                        cir::Selector {
                            var: cir::SelectorVariable::AllEntities,
                            args: vec![cir::SelectorArg("tag=turtle".to_string())],
                        }
                        .into(),
                    );
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

                cmds.push(Command::Comment("call to turtle_get".to_string()));

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
                    cmd.with_at(
                        cir::Selector {
                            var: cir::SelectorVariable::AllEntities,
                            args: vec![cir::SelectorArg("tag=turtle".to_string())],
                        }
                        .into(),
                    );
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
            "llvm.dbg.declare" => (vec![], None),
            "llvm.dbg.value" => (vec![], None),
            "llvm.memcpy.p0i8.p0i8.i32" => {
                assert_eq!(dest, None);
                (compile_memcpy(arguments, globals), None)
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
                // TODO: Determine if we need %%fixup
                let mut callee_id = McFuncId::new(name);
                callee_id.name.push_str("%%fixup");

                let mut before_cmds = Vec::new();

                // Push return address
                before_cmds.extend(push(ScoreHolder::new("%%fixup".to_string()).unwrap()));

                before_cmds.extend(setup_arguments(arguments, globals));

                // Branch to function
                before_cmds.push(McFuncCall { id: callee_id }.into());

                let after_cmds = if let Some(dest) = dest {
                    dest.into_iter()
                        .enumerate()
                        .map(|(idx, dest)| assign(dest, return_holder(idx)))
                        .collect()
                } else {
                    Vec::new()
                };

                (before_cmds, Some(after_cmds))
            }
        }
    } else if let Operand::LocalOperand {
        name: _name,
        ty:
            Type::PointerType {
                pointee_type,
                addr_space: _addr_space,
            },
    } = function
    {
        let (mut before_cmds, func_ptr) = eval_operand(function, globals);
        assert_eq!(func_ptr.len(), 1);
        let func_ptr = func_ptr.into_iter().next().unwrap();

        if let Type::FuncType {
            result_type,
            is_var_arg: false,
            ..
        } = &**pointee_type
        {
            let dest_size = type_layout(result_type).size();
            let dest = dest
                .clone()
                .map(|d| ScoreHolder::from_local_name(d, dest_size));

            // Push return address
            before_cmds.extend(push(ScoreHolder::new("%%fixup".to_string()).unwrap()));

            before_cmds.extend(setup_arguments(arguments, globals));

            // execute as @e[tag=ptr] store result entity @s Pos[2] double 1 run scoreboard players get func_ptr 1
            // Set the ptr's Z coordinate to `func_ptr`
            let mut set_z = Execute::new();
            set_z.with_as(
                cir::Selector {
                    var: cir::SelectorVariable::AllEntities,
                    args: vec![cir::SelectorArg("tag=ptr".into())],
                }
                .into(),
            );
            set_z.with_subcmd(ExecuteSubCmd::Store {
                is_success: false,
                kind: ExecuteStoreKind::Data {
                    target: DataTarget::Entity(
                        cir::Selector {
                            var: cir::SelectorVariable::ThisEntity,
                            args: Vec::new(),
                        }
                        .into(),
                    ),
                    path: "Pos[2]".into(),
                    ty: "double".into(),
                    scale: 1.0,
                },
            });
            set_z.with_run(ScoreGet {
                target: func_ptr.into(),
                target_obj: OBJECTIVE.into(),
            });

            before_cmds.push(set_z.into());

            let mut set_block = Execute::new();
            set_block.with_at(
                cir::Selector {
                    var: cir::SelectorVariable::AllEntities,
                    args: vec![cir::SelectorArg("tag=ptr".into())],
                }
                .into(),
            );
            set_block.with_run(SetBlock {
                pos: "-2 1 ~".into(),
                block: "minecraft:redstone_block".into(),
                kind: SetBlockKind::Replace,
            });

            before_cmds.push(set_block.into());

            let after_cmds = if let Some(dest) = dest {
                dest.into_iter()
                    .enumerate()
                    .map(|(idx, dest)| assign(dest, return_holder(idx)))
                    .collect()
            } else {
                Vec::new()
            };

            (before_cmds, Some(after_cmds))
        } else {
            todo!("{:?}", pointee_type)
        }
    } else {
        todo!("{:?}", function)
    }
}

pub fn compile_function(
    func: &Function,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
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

            if !block.instrs.is_empty() {
                println!("{:?}", block.instrs[0].get_debug_loc());
            }

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
                    let arg_size = type_layout(&arg.ty).size();

                    for (arg_word, arg_holder) in
                        ScoreHolder::from_local_name(arg.name.clone(), arg_size)
                            .into_iter()
                            .enumerate()
                    {
                        this.cmds.push(assign(arg_holder, param(idx, arg_word)));
                    }
                }
            }

            for instr in block.instrs.iter() {
                let (before, after) = compile_instr(instr, func, globals, options);
                this.cmds.extend(before);

                if let Some(after) = after {
                    result.push(std::mem::replace(&mut this, make_new_func(sub)));
                    sub += 1;
                    this.cmds.extend(after);
                }
            }

            this.cmds.push(
                ScoreSet {
                    target: ScoreHolder::new("%phi".to_string()).unwrap().into(),
                    target_obj: OBJECTIVE.into(),
                    score: idx as i32,
                }
                .into(),
            );

            match &block.term {
                Terminator::Ret(Ret {
                    return_operand: None,
                    ..
                }) => {
                    this.cmds.push(Command::Comment("return".to_string()));

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
                    this.cmds
                        .push(Command::Comment(format!("return operand {:?}", operand)));

                    let (cmds, source) = eval_operand(operand, globals);

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
                    let (cmds, cond) = eval_operand(condition, globals);
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
                    let (cmds, operand) = eval_operand(operand, globals);
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
                        }
                        .into(),
                        message: cir::TextBuilder::new()
                            .append_text("ENTERED UNREACHABLE CODE".into())
                            .build(),
                    }
                    .into(),
                ),
                Terminator::Resume(_) => {
                    let message = cir::TextBuilder::new()
                        .append_text("OH NO EXCEPTION HANDLING TOOD".into())
                        .build();

                    this.cmds.push(
                        Tellraw {
                            target: cir::Selector {
                                var: cir::SelectorVariable::AllPlayers,
                                args: Vec::new(),
                            }
                            .into(),
                            message,
                        }
                        .into(),
                    )
                }
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
    for cmd in funcs
        .iter()
        .flat_map(|f| f.cmds.iter())
        .filter(|cmd| !matches!(cmd, Command::Comment(_) | Command::FuncCall(_)))
    {
        let cmd_str = cmd.to_string();
        for mut holder in cmd_str
            .split_whitespace()
            .filter(|s| s.contains('%') && !s.contains('{') && !s.contains("rust:"))
        {
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
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> Vec<Command> {
    let (mut cmds, source0) = eval_operand(operand0, globals);
    let (tmp, source1) = eval_operand(operand1, globals);
    cmds.extend(tmp.into_iter());

    let dest = ScoreHolder::from_local_name(dest.clone(), type_layout(&operand0.get_type()).size());

    if let Type::VectorType {
        element_type,
        num_elements,
    } = operand0.get_type()
    {
        if !matches!(&*element_type, Type::IntegerType { bits: 32 }) {
            todo!("{:?}", element_type)
        }

        assert_eq!(source0.len(), num_elements);
        assert_eq!(source1.len(), num_elements);
        assert_eq!(dest.len(), num_elements);
    } else {
        assert_eq!(source0.len(), 1);
        assert_eq!(source1.len(), 1);
        assert_eq!(dest.len(), 1);
    };

    for (source0, (source1, dest)) in source0
        .into_iter()
        .zip(source1.into_iter().zip(dest.into_iter()))
    {
        cmds.push(assign(dest.clone(), source0));
        cmds.push(
            ScoreOp {
                target: dest.into(),
                target_obj: OBJECTIVE.to_string(),
                kind,
                source: Target::Uuid(source1),
                source_obj: OBJECTIVE.to_string(),
            }
            .into(),
        );
    }
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
        element_types[0..field as usize]
            .iter()
            .map(|t| type_layout(t).size())
            .sum::<usize>()
    } else {
        let mut offset = 0;
        let mut result = Layout::from_size_align(0, 1).unwrap();
        for elem in &element_types[0..field as usize + 1] {
            let (r, o) = result.extend(type_layout(elem)).unwrap();
            offset = o;
            result = r;
        }
        offset
    }
}

pub fn type_layout(ty: &Type) -> Layout {
    match ty {
        Type::IntegerType { bits: 1 } => Layout::from_size_align(1, 1).unwrap(),
        Type::IntegerType { bits: 8 } => Layout::from_size_align(1, 1).unwrap(),
        Type::IntegerType { bits: 16 } => Layout::from_size_align(2, 2).unwrap(),
        Type::IntegerType { bits: 32 } => Layout::from_size_align(4, 4).unwrap(),
        Type::IntegerType { bits: 64 } => Layout::from_size_align(8, 4).unwrap(),
        Type::StructType {
            element_types,
            is_packed,
        } => {
            if *is_packed {
                // TODO: Determine if this applies to inner fields as well
                Layout::from_size_align(
                    element_types.iter().map(|e| type_layout(e).size()).sum(),
                    1,
                )
                .unwrap()
            } else if element_types.is_empty() {
                Layout::from_size_align(0, 1).unwrap()
            } else {
                let mut result = type_layout(&element_types[0]);
                for elem in &element_types[1..] {
                    result = result.extend(type_layout(elem)).unwrap().0;
                }
                result
            }
        }
        Type::NamedStructType { ty: Some(ty), .. } => {
            let ty = ty.upgrade().expect("Failed to upgrade type");

            let ty_read = ty.read().unwrap();

            type_layout(&ty_read)
        }
        Type::VectorType {
            element_type,
            num_elements,
        } => {
            let mut result = type_layout(element_type);
            for _ in 0..num_elements - 1 {
                result = result.extend(type_layout(element_type)).unwrap().0;
            }
            result.align_to(4).unwrap()
        }
        Type::ArrayType {
            element_type,
            num_elements,
        } => {
            if *num_elements == 0 {
                Layout::from_size_align(0, 1).unwrap()
            } else {
                let mut result = type_layout(element_type);
                for _ in 0..num_elements - 1 {
                    result = result.extend(type_layout(element_type)).unwrap().0;
                }
                result
            }
        }
        Type::PointerType { .. } => Layout::from_size_align(4, 4).unwrap(),
        Type::VoidType => Layout::from_size_align(0, 4).unwrap(),
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
    let type_size = type_layout(allocated_type)
        .align_to(4)
        .unwrap()
        .pad_to_align()
        .size();

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
fn compile_unsigned_cmp(
    lhs: ScoreHolder,
    rhs: ScoreHolder,
    dest: ScoreHolder,
    relation: cir::Relation,
) -> Vec<Command> {
    let mut cmds = Vec::new();

    let (invert, or_eq) = match relation {
        cir::Relation::LessThan => (false, false),
        cir::Relation::LessThanEq => (false, true),
        cir::Relation::GreaterThanEq => (true, false),
        cir::Relation::GreaterThan => (true, true),
        cir::Relation::Eq => panic!(),
    };

    // Reset the flag
    cmds.push(
        ScoreSet {
            target: dest.clone().into(),
            target_obj: OBJECTIVE.to_string(),
            score: invert as i32,
        }
        .into(),
    );

    let mut check1 = Execute::new();
    check1.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    check1.with_if(ExecuteCondition::Score {
        target: rhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    check1.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Relation {
            relation: cir::Relation::LessThan,
            source: rhs.clone().into(),
            source_obj: OBJECTIVE.to_string(),
        },
    });
    check1.with_run(ScoreSet {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        score: !invert as i32,
    });
    cmds.push(check1.into());

    let mut check2 = Execute::new();
    check2.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    check2.with_if(ExecuteCondition::Score {
        target: rhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    check2.with_if(ExecuteCondition::Score {
        target: lhs.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        kind: ExecuteCondKind::Relation {
            relation: cir::Relation::LessThan,
            source: rhs.clone().into(),
            source_obj: OBJECTIVE.to_string(),
        },
    });
    check2.with_run(ScoreSet {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.to_string(),
        score: !invert as i32,
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
        score: !invert as i32,
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
            },
        });
        eq_check.with_run(ScoreSet {
            target: dest.into(),
            target_obj: OBJECTIVE.to_string(),
            score: !invert as i32,
        });
        cmds.push(eq_check.into());
    }

    cmds
}

fn compile_signed_cmp(
    target: ScoreHolder,
    source: ScoreHolder,
    dest: ScoreHolder,
    relation: cir::Relation,
    normal: bool,
) -> Command {
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

pub fn shift_left_bytes(holder: ScoreHolder, byte: u32) -> Vec<Command> {
    assert!(byte < 4);

    let mut cmds = Vec::new();

    cmds.push(Command::Comment(format!(
        "shift_left_bytes by {} bytes",
        byte
    )));

    for _ in 0..byte {
        cmds.push(
            ScoreOp {
                target: holder.clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ScoreOpKind::MulAssign,
                source: ScoreHolder::new("%%256".to_string()).unwrap().into(),
                source_obj: OBJECTIVE.into(),
            }
            .into(),
        )
    }

    cmds
}

pub fn shift_right_bytes(holder: ScoreHolder, byte: u32) -> Vec<Command> {
    assert!(byte < 4);

    let mut cmds = Vec::new();

    cmds.push(Command::Comment(format!(
        "shift_right_bytes by {} bytes",
        byte
    )));

    for _ in 0..byte {
        cmds.push(
            ScoreOp {
                target: holder.clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ScoreOpKind::DivAssign,
                source: ScoreHolder::new("%%256".to_string()).unwrap().into(),
                source_obj: OBJECTIVE.into(),
            }
            .into(),
        )
    }

    cmds
}

/// Zeros out the lowest `bytes` bytes of `holder`
pub fn zero_low_bytes(holder: ScoreHolder, bytes: u32) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(Command::Comment(format!(
        "zero {} lowest bytes of {}",
        bytes, holder
    )));

    for _ in 0..bytes {
        // Zero out the lower bits
        cmds.push(
            ScoreOp {
                target: holder.clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ScoreOpKind::DivAssign,
                source: ScoreHolder::new("%%256".to_string()).unwrap().into(),
                source_obj: OBJECTIVE.into(),
            }
            .into(),
        );
    }

    for _ in 0..bytes {
        cmds.push(
            ScoreOp {
                target: holder.clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ScoreOpKind::MulAssign,
                source: ScoreHolder::new("%%256".to_string()).unwrap().into(),
                source_obj: OBJECTIVE.into(),
            }
            .into(),
        );
    }

    cmds
}

/// Truncates `holder` so that it is `bytes` long
pub fn truncate_to(holder: ScoreHolder, bytes: u32) -> Vec<Command> {
    assert!(bytes < 4);

    let mut cmds = Vec::new();

    let top_bits = get_unique_holder();

    cmds.push(Command::Comment(format!(
        "truncate {} to {} bytes",
        holder, bytes
    )));

    cmds.push(assign(top_bits.clone(), holder.clone()));

    cmds.extend(zero_low_bytes(holder.clone(), bytes));

    cmds.push(
        ScoreOp {
            target: holder.into(),
            target_obj: OBJECTIVE.into(),
            kind: ScoreOpKind::SubAssign,
            source: top_bits.into(),
            source_obj: OBJECTIVE.into(),
        }
        .into(),
    );

    cmds
}

pub fn compile_instr(
    instr: &Instruction,
    parent: &Function,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
    _options: &Options,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let result = match instr {
        // We use an empty stack
        Instruction::Alloca(alloca) => compile_alloca(alloca),
        Instruction::GetElementPtr(GetElementPtr {
            address,
            indices,
            dest,
            ..
        }) => {
            let dest = ScoreHolder::from_local_name(dest.clone(), 4);
            let dest = dest[0].clone();

            let mut offset = 0;
            let mut ty = address.get_type();

            let mut cmds = Vec::new();

            assert!(matches!(ty, Type::PointerType { .. }));

            for index in indices {
                match ty {
                    Type::PointerType { pointee_type, .. } => {
                        let pointee_size = type_layout(&pointee_type).pad_to_align().size();

                        ty = *pointee_type;

                        match eval_maybe_const(index, globals) {
                            MaybeConst::Const(c) => offset += pointee_size as u32 * c as u32,
                            MaybeConst::NonConst(a, b) => {
                                assert_eq!(b.len(), 1);
                                let b = b.into_iter().next().unwrap();

                                cmds.extend(a);
                                for _ in 0..pointee_size {
                                    cmds.push(
                                        ScoreOp {
                                            target: dest.clone().into(),
                                            target_obj: OBJECTIVE.into(),
                                            kind: ScoreOpKind::AddAssign,
                                            source: b.clone().into(),
                                            source_obj: OBJECTIVE.into(),
                                        }
                                        .into(),
                                    );
                                }
                            }
                        }
                    }
                    Type::StructType {
                        element_types,
                        is_packed,
                    } => {
                        let index = if let MaybeConst::Const(c) = eval_maybe_const(index, globals) {
                            c
                        } else {
                            unreachable!("attempt to index struct at runtime")
                        };

                        offset += offset_of(&element_types, is_packed, index as u32) as u32;

                        ty = element_types.into_iter().nth(index as usize).unwrap();
                    }
                    Type::NamedStructType { ty: struct_ty, .. } => {
                        let index = if let MaybeConst::Const(c) = eval_maybe_const(index, globals) {
                            c
                        } else {
                            unreachable!("attempt to index named struct at runtime")
                        };

                        let struct_ty = struct_ty.unwrap().upgrade().unwrap();
                        let struct_ty = struct_ty.try_read().unwrap();

                        if let Type::StructType {
                            element_types,
                            is_packed,
                        } = &*struct_ty
                        {
                            offset += offset_of(element_types, *is_packed, index as u32) as u32;

                            ty = element_types[index as usize].clone();
                        } else {
                            todo!("{:?}", &*struct_ty);
                        }
                    }
                    Type::ArrayType { element_type, .. } => {
                        let elem_size = type_layout(&element_type).pad_to_align().size();

                        match eval_maybe_const(index, globals) {
                            MaybeConst::Const(c) => {
                                offset += c as u32 * elem_size as u32;
                            }
                            MaybeConst::NonConst(a, b) => {
                                assert_eq!(b.len(), 1);
                                let b = b.into_iter().next().unwrap();

                                cmds.extend(a);
                                for _ in 0..elem_size {
                                    cmds.push(
                                        ScoreOp {
                                            target: dest.clone().into(),
                                            target_obj: OBJECTIVE.into(),
                                            kind: ScoreOpKind::AddAssign,
                                            source: b.clone().into(),
                                            source_obj: OBJECTIVE.into(),
                                        }
                                        .into(),
                                    );
                                }
                            }
                        }

                        ty = *element_type;
                    }
                    _ => todo!("{:?}", ty),
                }
            }

            let mut start_cmds = match eval_maybe_const(address, globals) {
                MaybeConst::Const(addr) => vec![ScoreSet {
                    target: dest.clone().into(),
                    target_obj: OBJECTIVE.into(),
                    score: addr + offset as i32,
                }
                .into()],
                MaybeConst::NonConst(mut cmds, addr) => {
                    assert_eq!(addr.len(), 1);
                    let addr = addr.into_iter().next().unwrap();

                    cmds.push(assign(dest.clone(), addr));
                    cmds.push(
                        ScoreAdd {
                            target: dest.clone().into(),
                            target_obj: OBJECTIVE.into(),
                            score: offset as i32,
                        }
                        .into(),
                    );
                    cmds
                }
            };

            start_cmds.insert(
                0,
                Command::Comment(format!(
                    "getelementptr\naddress: {:?}\nindices: {:?}\ndest: {}",
                    address, indices, dest
                )),
            );

            start_cmds.extend(cmds);

            start_cmds
        }
        Instruction::Select(Select {
            condition,
            true_value,
            false_value,
            dest,
            ..
        }) => {
            let (mut cmds, true_val) = eval_operand(true_value, globals);
            let (tmp, false_val) = eval_operand(false_value, globals);
            cmds.extend(tmp);
            let (tmp, cond) = eval_operand(condition, globals);
            cmds.extend(tmp);

            let dest_size = type_layout(&true_value.get_type()).size();

            let dest = ScoreHolder::from_local_name(dest.clone(), dest_size);

            if cond.len() != 1 {
                todo!()
            }

            let cond = cond[0].clone();

            for (true_val, (false_val, dest)) in true_val
                .into_iter()
                .zip(false_val.into_iter().zip(dest.into_iter()))
            {
                let mut true_cmd = Execute::new();
                true_cmd.with_if(ExecuteCondition::Score {
                    target: cond.clone().into(),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ExecuteCondKind::Matches((1..=1).into()),
                });
                true_cmd.with_run(assign(dest.clone(), true_val));
                cmds.push(true_cmd.into());

                let mut false_cmd = Execute::new();
                false_cmd.with_unless(ExecuteCondition::Score {
                    target: cond.clone().into(),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ExecuteCondKind::Matches((1..=1).into()),
                });
                false_cmd.with_run(assign(dest, false_val));
                cmds.push(false_cmd.into());
            }

            cmds
        }
        Instruction::Store(Store { address, value, .. }) => {
            let (mut cmds, addr) = eval_operand(address, globals);

            assert_eq!(addr.len(), 1, "multiword addr {:?}", address);

            let addr = addr[0].clone();

            let value_size = type_layout(&value.get_type()).size();
            if value_size % 4 == 0 {
                // If we're directly storing a constant,
                // we can skip writing to a temporary value
                let write_cmds = match eval_maybe_const(value, globals) {
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
                            score: 4 * idx as i32,
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
            } else if value_size == 1 {
                let (eval_cmds, value) = eval_operand(value, globals);
                let value = value.into_iter().next().unwrap();

                cmds.extend(eval_cmds);
                cmds.push(assign(ptr(), addr));
                cmds.push(assign(param(2, 0), value));
                cmds.push(
                    McFuncCall {
                        id: McFuncId::new("intrinsic:store_byte"),
                    }
                    .into(),
                )
            } else {
                todo!("{:?}", value)
            }

            cmds
        }
        Instruction::Load(Load { dest, address, .. }) => {
            let pointee_type = if let Type::PointerType { pointee_type, .. } = address.get_type() {
                pointee_type
            } else {
                unreachable!()
            };

            let (mut cmds, addr) = eval_operand(address, globals);

            assert_eq!(addr.len(), 1, "multiword address {:?}", address);
            let addr = addr[0].clone();

            let pointee_layout = type_layout(&pointee_type);

            let dest = ScoreHolder::from_local_name(dest.clone(), pointee_layout.size());

            if pointee_layout.size() % 4 == 0 && pointee_layout.align() == 4 {
                for (word_idx, dest_word) in dest.into_iter().enumerate() {
                    cmds.push(assign(ptr(), addr.clone()));
                    cmds.push(
                        ScoreAdd {
                            target: ptr().into(),
                            target_obj: OBJECTIVE.into(),
                            score: 4 * word_idx as i32,
                        }
                        .into(),
                    );
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:setptr"),
                        }
                        .into(),
                    );
                    cmds.push(read_ptr(dest_word));
                }
            } else if pointee_layout.size() == 1 {
                cmds.push(assign(ptr(), addr));
                cmds.extend(read_ptr_small(dest[0].clone(), false));
            } else if pointee_layout.size() == 2 {
                cmds.push(assign(ptr(), addr));
                cmds.extend(read_ptr_small(dest[0].clone(), true));
            } else {
                todo!("{:?} with layout {:?}", pointee_type, pointee_layout)
            }

            cmds
        }
        Instruction::Add(Add {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::AddAssign, globals),
        Instruction::Sub(Sub {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::SubAssign, globals),
        Instruction::Mul(Mul {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::MulAssign, globals),
        Instruction::ICmp(ICmp {
            predicate: IntPredicate::NE,
            operand0,
            operand1,
            dest,
            ..
        }) if operand0.get_type() == Type::IntegerType { bits: 64 } => {
            // TODO: When operand1 is a constant, we can optimize the direct comparison into a `matches`
            let (mut cmds, op0) = eval_operand(operand0, globals);
            let (tmp_cmds, op1) = eval_operand(operand1, globals);
            cmds.extend(tmp_cmds);

            let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(
                ScoreSet {
                    target: dest.clone().into(),
                    target_obj: OBJECTIVE.into(),
                    score: 1,
                }
                .into(),
            );

            let mut exec = Execute::new();
            exec.with_if(ExecuteCondition::Score {
                target: op0[0].clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ExecuteCondKind::Relation {
                    relation: cir::Relation::Eq,
                    source: op1[0].clone().into(),
                    source_obj: OBJECTIVE.into(),
                },
            });
            exec.with_if(ExecuteCondition::Score {
                target: op0[1].clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ExecuteCondKind::Relation {
                    relation: cir::Relation::Eq,
                    source: op1[1].clone().into(),
                    source_obj: OBJECTIVE.into(),
                },
            });
            exec.with_run(ScoreSet {
                target: dest.into(),
                target_obj: OBJECTIVE.into(),
                score: 0,
            });

            cmds.push(exec.into());

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
            let (mut cmds, target) = eval_operand(operand0, globals);
            let (tmp_cmds, source) = eval_operand(operand1, globals);
            cmds.extend(tmp_cmds);

            assert_eq!(target.len(), 1, "multiword operand0 {:?}", operand0);
            assert_eq!(source.len(), 1, "multiword operand1 {:?}", operand1);

            let target = target[0].clone();
            let source = source[0].clone();

            let dest = ScoreHolder::from_local_name(dest.clone(), 1);
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
                IntPredicate::ULT => cmds.extend(compile_unsigned_cmp(
                    target,
                    source,
                    dest,
                    cir::Relation::LessThan,
                )),
                IntPredicate::ULE => cmds.extend(compile_unsigned_cmp(
                    target,
                    source,
                    dest,
                    cir::Relation::LessThanEq,
                )),
                IntPredicate::UGT => cmds.extend(compile_unsigned_cmp(
                    target,
                    source,
                    dest,
                    cir::Relation::GreaterThan,
                )),
                IntPredicate::UGE => cmds.extend(compile_unsigned_cmp(
                    target,
                    source,
                    dest,
                    cir::Relation::GreaterThanEq,
                )),
            }

            cmds
        }
        Instruction::Phi(Phi {
            incoming_values,
            dest,
            to_type,
            ..
        }) => {
            let to_type_size = type_layout(to_type).size();

            let dst = ScoreHolder::from_local_name(dest.clone(), to_type_size);

            let mut cmds = Vec::new();

            for (value, block) in incoming_values {
                let block_idx = parent
                    .basic_blocks
                    .iter()
                    .enumerate()
                    .find(|(_, b)| &b.name == block)
                    .unwrap()
                    .0 as i32;

                cmds.push(Command::Comment(format!(
                    "block {}\nvalue {:?}",
                    block, value
                )));

                let (tmp, val) = eval_operand(value, globals);
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
        Instruction::Call(call) => return compile_call(call, globals),
        Instruction::BitCast(BitCast {
            operand,
            dest,
            to_type,
            ..
        }) => {
            let (mut cmds, source) = eval_operand(operand, globals);

            if source.len() != 1 {
                todo!("multiword source {:?}", source);
            }

            let source = source[0].clone();

            let dest = ScoreHolder::from_local_name(dest.clone(), type_layout(to_type).size());

            if dest.len() != 1 {
                todo!("multiword dest {:?}", dest);
            }

            let dest = dest[0].clone();

            cmds.push(assign(dest, source));

            cmds
        }
        Instruction::Trunc(Trunc {
            operand,
            to_type: Type::IntegerType { bits: 8 },
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals);

            let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(assign(dest.clone(), op[0].clone()));

            cmds.extend(truncate_to(dest, 1));

            cmds
        }
        Instruction::Trunc(Trunc {
            operand,
            to_type: Type::IntegerType { bits: 32 },
            dest,
            ..
        }) => {
            if !matches!(operand.get_type(), Type::IntegerType { bits: 64 }) {
                todo!("{:?}", operand);
            }

            let (mut cmds, op) = eval_operand(operand, globals);

            let dest = ScoreHolder::from_local_name(dest.clone(), 4)[0].clone();

            cmds.push(assign(dest, op[0].clone()));

            cmds
        }
        Instruction::Trunc(Trunc {
            operand,
            to_type: Type::IntegerType { bits: 1 },
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals);

            assert_eq!(op.len(), 1);
            let op = op[0].clone();
            let dest = ScoreHolder::from_local_name(dest.clone(), 1)[0].clone();

            cmds.push(assign(dest.clone(), op));
            cmds.push(
                ScoreOp {
                    target: dest.clone().into(),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::MulAssign,
                    source: ScoreHolder::new("%%31BITSHIFT".to_string()).unwrap().into(),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );
            cmds.push(
                ScoreOp {
                    target: dest.into(),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ScoreOpKind::DivAssign,
                    source: ScoreHolder::new("%%31BITSHIFT".to_string()).unwrap().into(),
                    source_obj: OBJECTIVE.to_string(),
                }
                .into(),
            );

            cmds
        }
        Instruction::ExtractValue(ExtractValue {
            aggregate,
            indices,
            dest,
            ..
        }) => {
            let (mut cmds, aggr) = eval_operand(aggregate, globals);

            if indices.len() != 1 {
                todo!("{:?}", indices)
            }

            if let Type::StructType {
                element_types,
                is_packed,
            } = aggregate.get_type()
            {
                let result_type = &element_types[indices[0] as usize];
                let size = type_layout(result_type).size();

                let offset = offset_of(&element_types, is_packed, indices[0]);

                let dest = ScoreHolder::from_local_name(dest.clone(), size);

                if size == 4 {
                    if dest.len() != 1 {
                        todo!()
                    }

                    let dest = dest[0].clone();

                    if offset % 4 != 0 {
                        todo!()
                    }

                    cmds.push(assign(dest, aggr[offset as usize / 4].clone()))
                } else if size == 1 {
                    let dest = dest[0].clone();

                    // Shift over to the relevant byte
                    for _ in 0..offset {
                        cmds.push(
                            ScoreOp {
                                target: dest.clone().into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ScoreOpKind::DivAssign,
                                source: ScoreHolder::new("%%256".to_string()).unwrap().into(),
                                source_obj: OBJECTIVE.into(),
                            }
                            .into(),
                        );
                    }

                    cmds.extend(truncate_to(dest, 1));
                } else {
                    println!("{:?}", aggregate);
                    todo!("multiword extract value {:?}", result_type);
                }
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
            let aggr_layout = type_layout(&aggregate.get_type());

            if indices.len() != 1 {
                todo!("indices {:?}", indices)
            }
            let index = indices[0];

            let (element_types, is_packed) = if let Type::StructType {
                element_types,
                is_packed,
            } = aggregate.get_type()
            {
                (element_types, is_packed)
            } else {
                todo!("{:?}", aggregate.get_type())
            };

            let (mut cmds, aggr) = eval_operand(aggregate, globals);
            let (tmp, elem) = eval_operand(element, globals);
            cmds.extend(tmp);

            let elem = elem[0].clone();

            let offset = offset_of(&element_types, is_packed, index);

            if offset % 4 != 0 {
                todo!("{:?}", offset);
            }

            let dest = ScoreHolder::from_local_name(dest.clone(), aggr_layout.size());

            let insert_idx = offset / 4;

            assert_eq!(dest.len(), aggr.len());

            let mut cmds = Vec::new();

            for (dest_word, aggr_word) in dest.iter().zip(aggr.into_iter()) {
                cmds.push(assign(dest_word.clone(), aggr_word.clone()));
            }

            if type_layout(&element.get_type()).size() == 4 && offset % 4 == 0 {
                cmds.push(assign(dest[insert_idx].clone(), elem));
            } else if type_layout(&element.get_type()).size() == 1 {
                if index == 0 {
                    cmds.extend(zero_low_bytes(dest[insert_idx].clone(), 1));
                    cmds.push(
                        ScoreOp {
                            target: dest[insert_idx].clone().into(),
                            target_obj: OBJECTIVE.into(),
                            kind: ScoreOpKind::AddAssign,
                            source: elem.into(),
                            source_obj: OBJECTIVE.into(),
                        }
                        .into(),
                    );
                } else if index + 1 == element_types.len() as u32 {
                    let trunc_len = offset % 4;
                    cmds.extend(truncate_to(dest[insert_idx].clone(), trunc_len as u32));
                    cmds.extend(shift_left_bytes(elem.clone(), trunc_len as u32));
                    cmds.push(
                        ScoreOp {
                            target: dest[insert_idx].clone().into(),
                            target_obj: OBJECTIVE.into(),
                            kind: ScoreOpKind::AddAssign,
                            source: elem.into(),
                            source_obj: OBJECTIVE.into(),
                        }
                        .into(),
                    );
                } else {
                    todo!()
                }
            } else {
                todo!();
            }

            cmds
        }
        Instruction::ZExt(ZExt {
            operand,
            to_type,
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals);

            if op.len() != 1 {
                todo!()
            }

            let to_size = type_layout(to_type).size();

            let dst = ScoreHolder::from_local_name(dest.clone(), to_size);

            cmds.push(assign(dst[0].clone(), op[0].clone()));
            for dst in dst[1..].iter().cloned() {
                cmds.push(
                    ScoreSet {
                        target: dst.into(),
                        target_obj: OBJECTIVE.into(),
                        score: 0,
                    }
                    .into(),
                );
            }

            cmds
        }
        Instruction::Or(Or {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            assert_eq!(operand0.get_type(), operand1.get_type());

            let (mut cmds, op0) = eval_operand(operand0, globals);
            let op0 = op0.into_iter().next().unwrap();

            let (tmp, op1) = eval_operand(operand1, globals);
            let op1 = op1.into_iter().next().unwrap();

            cmds.extend(tmp);

            match operand0.get_type() {
                Type::IntegerType { bits: 32 } => {
                    cmds.push(assign(param(0, 0), op0));
                    cmds.push(assign(param(1, 0), op1));

                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:or"),
                        }
                        .into(),
                    );

                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign(dest, return_holder(0)));

                    cmds
                }
                Type::IntegerType { bits: 1 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(
                        ScoreSet {
                            target: dest.clone().into(),
                            target_obj: OBJECTIVE.into(),
                            score: 1,
                        }
                        .into(),
                    );

                    let mut exec = Execute::new();
                    exec.with_if(ExecuteCondition::Score {
                        target: op0.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=0).into()),
                    });
                    exec.with_if(ExecuteCondition::Score {
                        target: op1.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=0).into()),
                    });
                    exec.with_run(ScoreSet {
                        target: dest.into(),
                        target_obj: OBJECTIVE.into(),
                        score: 0,
                    });
                    cmds.push(exec.into());

                    cmds
                }
                ty => todo!("{:?}", ty),
            }
        }
        Instruction::And(And {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            assert_eq!(operand0.get_type(), operand1.get_type());

            let (mut cmds, op0) = eval_operand(operand0, globals);
            let (tmp, op1) = eval_operand(operand1, globals);
            cmds.extend(tmp);

            match operand0.get_type() {
                Type::IntegerType { bits: 64 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 8);

                    cmds.push(assign(param(0, 0), op0[1].clone()));
                    cmds.push(assign(param(1, 0), op1[1].clone()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:and"),
                        }
                        .into(),
                    );
                    cmds.push(assign(dest[1].clone(), return_holder(0)));

                    cmds.push(assign(param(0, 0), op0[0].clone()));
                    cmds.push(assign(param(1, 0), op1[0].clone()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:and"),
                        }
                        .into(),
                    );

                    cmds.push(assign(dest[0].clone(), return_holder(0)));

                    cmds
                }
                Type::VectorType {
                    element_type,
                    num_elements: 4,
                } if matches!(&*element_type, Type::IntegerType { bits: 8 }) => {
                    // TODO: This is exactly the same as all the others but matches are hard
                    cmds.push(assign(param(0, 0), op0[0].clone()));
                    cmds.push(assign(param(1, 0), op1[0].clone()));

                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:and"),
                        }
                        .into(),
                    );

                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign(dest, return_holder(0)));

                    cmds
                }
                Type::IntegerType { bits: 8 } | Type::IntegerType { bits: 32 } => {
                    cmds.push(assign(param(0, 0), op0[0].clone()));
                    cmds.push(assign(param(1, 0), op1[0].clone()));

                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:and"),
                        }
                        .into(),
                    );

                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign(dest, return_holder(0)));

                    cmds
                }
                Type::IntegerType { bits: 1 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    let mut exec = Execute::new();
                    exec.with_subcmd(ExecuteSubCmd::Store {
                        is_success: true,
                        kind: ExecuteStoreKind::Score {
                            target: dest.into(),
                            objective: OBJECTIVE.into(),
                        },
                    });
                    exec.with_if(ExecuteCondition::Score {
                        target: op0[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((1..=1).into()),
                    });
                    exec.with_if(ExecuteCondition::Score {
                        target: op1[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((1..=1).into()),
                    });

                    cmds.push(exec.into());

                    cmds
                }
                _ => todo!("{:?}", operand0),
            }
        }
        Instruction::Xor(Xor {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            assert_eq!(operand0.get_type(), operand1.get_type());

            let (mut cmds, op0) = eval_operand(operand0, globals);
            let op0 = op0.into_iter().next().unwrap();

            let (tmp, op1) = eval_operand(operand1, globals);
            let op1 = op1.into_iter().next().unwrap();

            cmds.extend(tmp);

            match operand0.get_type() {
                Type::IntegerType { bits: 32 } => {
                    cmds.push(assign(param(0, 0), op0));
                    cmds.push(assign(param(1, 0), op1));

                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:xor"),
                        }
                        .into(),
                    );

                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign(dest, return_holder(0)));

                    cmds
                }
                Type::IntegerType { bits: 1 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(
                        ScoreSet {
                            target: dest.clone().into(),
                            target_obj: OBJECTIVE.into(),
                            score: 0,
                        }
                        .into(),
                    );

                    let mut lhs_1 = Execute::new();
                    lhs_1.with_if(ExecuteCondition::Score {
                        target: op0.clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((1..=1).into()),
                    });
                    lhs_1.with_if(ExecuteCondition::Score {
                        target: op1.clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=0).into()),
                    });
                    lhs_1.with_run(ScoreSet {
                        target: dest.clone().into(),
                        target_obj: OBJECTIVE.into(),
                        score: 1,
                    });

                    let mut rhs_1 = Execute::new();
                    rhs_1.with_if(ExecuteCondition::Score {
                        target: op0.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=0).into()),
                    });
                    rhs_1.with_if(ExecuteCondition::Score {
                        target: op1.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((1..=1).into()),
                    });
                    rhs_1.with_run(ScoreSet {
                        target: dest.into(),
                        target_obj: OBJECTIVE.into(),
                        score: 1,
                    });

                    cmds.push(lhs_1.into());
                    cmds.push(rhs_1.into());

                    cmds
                }
                t => todo!("{:?}", t),
            }
        }
        Instruction::Shl(Shl {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            if !matches!(operand0.get_type(), Type::IntegerType { bits: 32 }) {
                todo!("{:?}", operand0);
            }

            let (mut cmds, op0) = eval_operand(operand0, globals);
            let op0 = op0.into_iter().next().unwrap();

            let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                .into_iter()
                .next()
                .unwrap();

            if let MaybeConst::Const(c) = eval_maybe_const(operand1, globals) {
                let tmp = get_unique_holder();
                cmds.push(
                    ScoreSet {
                        target: tmp.clone().into(),
                        target_obj: OBJECTIVE.into(),
                        score: 1 << c,
                    }
                    .into(),
                );
                cmds.push(assign(dest.clone(), op0));
                cmds.push(
                    ScoreOp {
                        target: dest.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ScoreOpKind::MulAssign,
                        source: tmp.into(),
                        source_obj: OBJECTIVE.into(),
                    }
                    .into(),
                );
            } else {
                todo!("{:?}", operand1)
            }

            cmds
        }
        Instruction::LShr(LShr {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            let (mut cmds, op0) = eval_operand(operand0, globals);

            if let Operand::ConstantOperand(Constant::Int {
                bits: 64,
                value: 32,
            }) = operand1
            {
                if matches!(operand0.get_type(), Type::IntegerType { bits: 64 }) {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign(dest, op0[1].clone()));

                    cmds
                } else {
                    todo!()
                }
            } else if let Operand::ConstantOperand(Constant::Int { bits: 32, value: 1 }) = operand1
            {
                // FIXME: This implements an *arithmetic* right shift
                if matches!(operand0.get_type(), Type::IntegerType { bits: 32 }) {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign(dest.clone(), op0[0].clone()));

                    cmds.push(
                        ScoreOp {
                            target: dest.into(),
                            target_obj: OBJECTIVE.into(),
                            kind: ScoreOpKind::DivAssign,
                            source: ScoreHolder::new("%%2".into()).unwrap().into(),
                            source_obj: OBJECTIVE.into(),
                        }
                        .into(),
                    );
                } else {
                    todo!()
                }

                cmds
            } else {
                eprintln!("{:?}", operand0);
                eprintln!("{:?}", operand1);
                eprintln!("{:?}", dest);
                todo!()
            }
        }
        Instruction::PtrToInt(PtrToInt {
            operand,
            to_type: Type::IntegerType { bits: 32 },
            dest,
            ..
        }) => {
            if !matches!(operand.get_type(), Type::PointerType{ .. }) {
                todo!("{:?}", operand)
            }

            let (mut cmds, op) = eval_operand(operand, globals);
            let op = op.into_iter().next().unwrap();

            let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(assign(dest, op));

            cmds
        }
        Instruction::IntToPtr(IntToPtr {
            operand,
            to_type,
            dest,
            ..
        }) => {
            assert_eq!(operand.get_type(), Type::IntegerType { bits: 32 });

            if !matches!(to_type.get_type(), Type::PointerType{ .. }) {
                todo!("{:?}", operand)
            }

            let (mut cmds, op) = eval_operand(operand, globals);
            let op = op.into_iter().next().unwrap();

            let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(assign(dest, op));

            cmds
        }
        Instruction::ShuffleVector(ShuffleVector {
            operand0,
            operand1,
            dest,
            mask,
            ..
        }) => {
            let (mut cmds, op0) = eval_operand(operand0, globals);
            let (tmp, op1) = eval_operand(operand1, globals);
            cmds.extend(tmp);

            let element_type = if let Type::VectorType { element_type, .. } = operand0.get_type() {
                element_type
            } else {
                unreachable!()
            };

            if !matches!(&*element_type, Type::IntegerType { bits: 32 }) {
                todo!("{:?}", element_type)
            }

            let op0_len = if let Type::VectorType { num_elements, .. } = mask.get_type() {
                num_elements
            } else {
                unreachable!()
            };

            let mask_vals = if let Constant::Vector(v) = mask {
                &v[..]
            } else {
                unreachable!()
            };

            let dest_type = Type::VectorType {
                element_type,
                num_elements: mask_vals.len(),
            };

            let dest = ScoreHolder::from_local_name(dest.clone(), type_layout(&dest_type).size());
            assert_eq!(dest.len(), mask_vals.len());

            for (dest, mask_idx) in dest.into_iter().zip(mask_vals.iter()) {
                match mask_idx {
                    Constant::Undef(_) => {
                        // Should we mark it as `undef` for the interpreter?
                    }
                    Constant::Int { bits: 32, value } => {
                        let value = *value as usize;
                        let source = if value > op0_len {
                            &op1[value - op0_len]
                        } else {
                            &op0[value]
                        };
                        cmds.push(assign(dest, source.clone()));
                    }
                    _ => unreachable!(),
                }
            }

            cmds
        }
        Instruction::ExtractElement(ExtractElement {
            vector,
            index,
            dest,
            ..
        }) => {
            let element_type = if let Type::VectorType { element_type, .. } = vector.get_type() {
                element_type
            } else {
                unreachable!()
            };

            if !matches!(&*element_type, Type::IntegerType { bits: 32 }) {
                todo!("{:?}", vector)
            }

            let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                .into_iter()
                .next()
                .unwrap();

            let (mut cmds, vec) = eval_operand(vector, globals);

            match eval_maybe_const(index, globals) {
                MaybeConst::Const(c) => {
                    cmds.push(assign(dest, vec[c as usize].clone()));
                }
                MaybeConst::NonConst(_, _) => todo!("{:?}", index),
            }

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

pub fn eval_constant(
    con: &Constant,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> MaybeConst {
    match con {
        Constant::GlobalReference { name, .. } => {
            let temp = name.to_string();
            let holder = ScoreHolder::new_lossy(format!("%@{}", &temp[1..temp.len() - 1]));
            MaybeConst::NonConst(vec![], vec![holder])
        }
        Constant::PtrToInt(tmp) => {
            if let llvm_ir::constant::PtrToInt {
                operand: Constant::GlobalReference { name, .. },
                ..
            } = &**tmp
            {
                let addr = globals
                    .get(name)
                    .unwrap_or_else(|| panic!("failed to get {:?}", name))
                    .0;
                MaybeConst::Const(addr as i32)
            } else {
                todo!("{:?}", tmp)
            }
        }
        Constant::Int { bits: 1, value } => MaybeConst::Const(*value as i32),
        Constant::Int { bits: 8, value } => MaybeConst::Const(*value as i8 as i32),
        Constant::Int { bits: 32, value } => MaybeConst::Const(*value as i32),
        Constant::Int { bits: 64, value } => {
            // TODO: I mean it's *const* but not convenient...
            let num = get_unique_num();

            let lo_word = ScoreHolder::new(format!("%temp{}%0", num)).unwrap();
            let hi_word = ScoreHolder::new(format!("%temp{}%1", num)).unwrap();

            let cmds = vec![
                ScoreSet {
                    target: lo_word.clone().into(),
                    target_obj: OBJECTIVE.into(),
                    score: *value as i32,
                }
                .into(),
                ScoreSet {
                    target: hi_word.clone().into(),
                    target_obj: OBJECTIVE.into(),
                    score: (*value >> 32) as i32,
                }
                .into(),
            ];

            MaybeConst::NonConst(cmds, vec![lo_word, hi_word])
        }
        Constant::BitCast(bitcast) => eval_constant(&bitcast.operand, globals),
        Constant::Undef(ty) => {
            // TODO: This can literally be *anything* you want it to be

            let len = type_layout(ty).size();

            let num = get_unique_num();

            let (cmds, holders) = (0..((len + 3) / 4))
                .map(|idx| {
                    let holder = ScoreHolder::new(format!("%temp{}%{}", num, idx)).unwrap();
                    (
                        ScoreSet {
                            target: holder.clone().into(),
                            target_obj: OBJECTIVE.into(),
                            score: 0,
                        }
                        .into(),
                        holder,
                    )
                })
                .unzip();

            MaybeConst::NonConst(cmds, holders)
        }
        Constant::GetElementPtr(g) => MaybeConst::Const(getelementptr_const(&g, globals) as i32),
        Constant::Null(_) => MaybeConst::Const(0),
        Constant::AggregateZero(Type::VectorType {
            element_type,
            num_elements,
        }) => {
            let size = type_layout(&element_type).size() * num_elements;
            if size % 4 == 0 {
                let num = get_unique_num();

                let (cmds, holders) = (0..(size / 4))
                    .map(|idx| {
                        let holder = ScoreHolder::new(format!("%temp{}%{}", num, idx)).unwrap();
                        (
                            ScoreSet {
                                target: holder.clone().into(),
                                target_obj: OBJECTIVE.into(),
                                score: 0,
                            }
                            .into(),
                            holder,
                        )
                    })
                    .unzip();

                MaybeConst::NonConst(cmds, holders)
            } else {
                todo!("{:?} {}", element_type, num_elements)
            }
        }
        Constant::Vector(elems) => {
            if let [Constant::Int {
                bits: 8,
                value: val0,
            }, Constant::Int {
                bits: 8,
                value: val1,
            }, Constant::Int {
                bits: 8,
                value: val2,
            }, Constant::Int {
                bits: 8,
                value: val3,
            }] = elems[..]
            {
                MaybeConst::Const(i32::from_le_bytes([
                    val0 as u8, val1 as u8, val2 as u8, val3 as u8,
                ]))
            } else {
                todo!("{:?}", elems);
            }
        }
        _ => todo!("evaluate constant {:?}", con),
    }
}

pub fn eval_maybe_const(
    op: &Operand,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> MaybeConst {
    match op {
        Operand::LocalOperand { name, ty } => {
            let len = type_layout(ty).size();

            let holders = ScoreHolder::from_local_name(name.clone(), len);

            MaybeConst::NonConst(Vec::new(), holders)
        }
        Operand::ConstantOperand(con) => eval_constant(con, globals),
        _ => todo!("operand {:?}", op),
    }
}

pub fn eval_operand(
    op: &Operand,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> (Vec<Command>, Vec<ScoreHolder>) {
    eval_maybe_const(op, globals).force_eval()
}

lazy_static! {
    pub static ref TEMP_CNT: Mutex<u32> = Mutex::new(0);
    pub static ref FREE_PTR: Mutex<u32> = Mutex::new(4);
}

fn get_alloc(mut amount: u32) -> u32 {
    if amount % 4 != 0 {
        amount += 4 - (amount % 4);
    }

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

#[cfg(test)]
mod test {
    use super::*;
    use crate::interpreter::Interpreter;

    #[test]
    fn read_ptr_byte() {
        let func = McFunction {
            id: McFuncId::new("read_ptr_small"),
            cmds: read_ptr_small(ScoreHolder::new("dest".into()).unwrap(), false),
        };

        let mut interp = Interpreter::new(vec![func], "");
        let word = [0x12, 0xEA, 0x56, 0x78];
        interp
            .rust_scores
            .insert(ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp.memory[1] = i32::from_le_bytes(word);

        for (pt, expected) in word.iter().copied().enumerate() {
            interp.call_stack = vec![(0, 0)];
            interp.rust_scores.insert(ptr(), pt as i32 + 4);
            interp.run_to_end().unwrap();
            let result = *interp
                .rust_scores
                .get(&ScoreHolder::new("dest".into()).unwrap())
                .unwrap();
            assert_eq!(result, expected as i32);
        }
    }

    #[test]
    fn vector_layout() {
        let ty = Type::VectorType {
            element_type: Box::new(Type::IntegerType { bits: 8 }),
            num_elements: 4,
        };

        assert_eq!(type_layout(&ty), Layout::from_size_align(4, 4).unwrap());
    }

    #[test]
    fn struct_offset() {
        // TODO: More comprehensive test
        let element_types = vec![
            Type::IntegerType { bits: 32 },
            Type::IntegerType { bits: 32 },
        ];

        assert_eq!(offset_of(&element_types, false, 0), 0);
        assert_eq!(offset_of(&element_types, false, 1), 4);
        assert_eq!(offset_of(&element_types, true, 0), 0);
        assert_eq!(offset_of(&element_types, true, 1), 4);
    }
}
