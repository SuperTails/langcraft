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
use llvm_ir::constant::ICmp as ICmpConst;
use llvm_ir::constant::Select as SelectConst;
use llvm_ir::instruction::{
    Add, Alloca, And, BitCast, Call, ExtractElement, ExtractValue, GetElementPtr, ICmp,
    InsertElement, InsertValue, IntToPtr, LShr, Load, Mul, Or, Phi, PtrToInt, SDiv, SExt, SRem,
    Select, Shl, ShuffleVector, Store, Sub, Trunc, UDiv, URem, Xor, ZExt,
};
use llvm_ir::module::GlobalVariable;
use llvm_ir::terminator::{Br, CondBr, Ret, Switch, Unreachable};
use llvm_ir::types::Typed;
use llvm_ir::{
    Constant, Function, Instruction, IntPredicate, Module, Name, Operand, Terminator, Type,
};
use std::alloc::Layout;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::{TryFrom, TryInto};
use std::sync::Mutex;

// FIXME: Alignment for Alloca, functions, and global variables

pub const OBJECTIVE: &str = "rust";

pub const ROW_SIZE: usize = 32;

pub fn pos_to_func_idx(x: i32, z: i32) -> usize {
    usize::try_from(ROW_SIZE as i32 * (-2 - x) + z).unwrap()
}

pub fn func_idx_to_pos(f: usize) -> (i32, i32) {
    (-2 - (f / ROW_SIZE) as i32, (f % ROW_SIZE) as i32)
}

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

pub fn mark_todo() -> Command {
    Command::Comment("!INTERPRETER: TODO".into())
}

pub fn mark_unreachable() -> Command {
    Command::Comment("!INTERPRETER: UNREACHABLE".into())
}

pub fn mark_assertion(is_unless: bool, cond: &ExecuteCondition) -> Command {
    let mut text = "!INTERPRETER: ASSERT ".to_string();
    if is_unless {
        text.push_str("unless ");
    } else {
        text.push_str("if ");
    }
    text.push_str(&cond.to_string());
    Command::Comment(text)
}

// %ptr, %x, %y, %z, %param<X> are caller-saved registers
// all other registers are callee-saved
// %stackptr is... weird
// %temp<X> are... weird

// `intrinsic:setptr` sets the pointer to the value in `%ptr` for objective `rust`

/// This reads from %ptr, does a setptr, and then gets either a halfword or a byte
///
/// ... and clobbers %param0%0 and %param1%0 in the process
pub fn read_ptr_small(dest: ScoreHolder, is_halfword: bool) -> Vec<Command> {
    let mut cmds = Vec::new();

    if is_halfword {
        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:load_halfword"),
            }
            .into(),
        );

        cmds.push(assign(dest, param(0, 0)));

        cmds
    } else {
        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:load_byte"),
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

pub fn make_op_lit(lhs: ScoreHolder, kind: &str, score: i32) -> Command {
    match kind {
        "+=" => ScoreAdd {
            target: lhs.into(),
            target_obj: OBJECTIVE.into(),
            score,
        }
        .into(),
        "-=" => ScoreAdd {
            target: lhs.into(),
            target_obj: OBJECTIVE.into(),
            score: -score,
        }
        .into(),
        _ => {
            let rhs = ScoreHolder::new(format!("%%{}", score)).unwrap();
            make_op(lhs, kind, rhs)
        }
    }
}

pub fn make_op(lhs: ScoreHolder, op: &str, rhs: ScoreHolder) -> Command {
    let kind = match op {
        "+=" => ScoreOpKind::AddAssign,
        "-=" => ScoreOpKind::SubAssign,
        "*=" => ScoreOpKind::MulAssign,
        "/=" => ScoreOpKind::DivAssign,
        "%=" => ScoreOpKind::ModAssign,
        // TODO: Max, min, swap operators
        _ => panic!("{}", op),
    };

    ScoreOp {
        target: lhs.into(),
        target_obj: OBJECTIVE.into(),
        kind,
        source: rhs.into(),
        source_obj: OBJECTIVE.into(),
    }
    .into()
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

pub fn assign_lit(target: ScoreHolder, score: i32) -> Command {
    ScoreSet {
        target: target.into(),
        target_obj: OBJECTIVE.into(),
        score,
    }
    .into()
}

pub fn get_index(x: i32, y: i32, z: i32) -> Result<i32, InterpError> {
    if 0 <= x && x < 64 && 0 <= y && y < 16 && 0 <= z && z < 16 {
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
    assert!(address < 64 * 16 * 16);
    let z = address % 16;
    address /= 16;
    let y = address % 16;
    address /= 16;
    let x = address % 32;
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BuildOptions {
    pub log_trace: bool,
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

                    let (x, z) = func_idx_to_pos(idx);

                    let pos = format!("{} 1 {}", x, z);
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

                        let (x, z) = func_idx_to_pos(idx);

                        let pos = format!("{} 1 {}", x, z);
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

pub fn compile_module(module: &Module, options: &BuildOptions) -> Vec<McFunction> {
    let (mut init_cmds, globals) = compile_global_var_init(&module.global_vars, &module.functions);

    let main_return = get_alloc(4);

    init_cmds.push(set_memory(-1, main_return as i32));

    init_cmds.push(assign_lit(stackptr(), get_alloc(4) as i32));

    init_cmds.push(assign_lit(stackbaseptr(), 0));

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
            .filter(|e| !e.0.as_ref().starts_with("%return%") && !e.0.as_ref().contains("%%fixup"))
            .collect();

        for McFunction { id, .. } in mc_funcs.iter() {
            clobber_list.insert(
                id.name.clone(),
                clobbers.clone().into_iter().map(|c| c.0).collect(),
            );
        }

        let mut f = mc_funcs.into_iter();
        funcs.push(f.next().unwrap());
        after_blocks.extend(f);
    }

    funcs.extend(after_blocks);

    funcs.extend(crate::intrinsics::INTRINSICS.clone());

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

            /*let message = cir::TextBuilder::new()
            .append_text(format!("%stackptr at start of {} is ", func.id))
            .append_score(stackptr(), OBJECTIVE.into(), None)
            .build();*/

            let save_code = /*std::iter::once(
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
            .chain(*/
                clobber_list
                    .get(&func.id.name)
                    .unwrap()
                    .iter()
                    .cloned()
                    .chain(std::iter::once(stackbaseptr()))
                    .map(push)
                    .flatten()
                    .chain(std::iter::once(base_set));
            //);

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

    let mut build_cmds = funcs
        .iter()
        .enumerate()
        .map(|(idx, func)| {
            let (x, z) = func_idx_to_pos(idx);
            let pos = format!("{} 0 {}", x, z);
            let block = format!(
                "minecraft:command_block{{Command:\"{}\"}}",
                McFuncCall {
                    id: func.id.clone()
                }
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
            end: "-15 0 64".to_string(),
            block: "minecraft:air".to_string(),
        }
        .into(),
    );

    funcs[0].cmds.extend(build_cmds);

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

    if options.log_trace {
        for func in &mut funcs[1..] {
            func.cmds.insert(0, print_entry(&func.id));
        }
    }

    let (main_x, main_z) = func_idx_to_pos(main_idx);
    funcs.push(McFunction {
        id: McFuncId::new("run"),
        cmds: vec![
            McFuncCall {
                id: McFuncId::new("init"),
            }
            .into(),
            SetBlock {
                pos: format!("{} 1 {}", main_x, main_z),
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

    #[allow(clippy::reversed_empty_ranges)]
    funcs[0].cmds.splice(
        0..0,
        all_clobbers.iter().map(|c| assign_lit((*c).clone(), 1)),
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

    println!("Address: {:?}", address);
    println!("Indices: {:?}", indices);

    let result = if let Constant::GlobalReference { name, ty } = address {
        let mut offset = globals
            .get(&name)
            .unwrap_or_else(|| panic!("couldn't find global {:?}", name))
            .0;
        let mut ty = ty.clone();

        for index in &indices[1..] {
            let index = if let Constant::Int { bits: 32, value } = index {
                *value as i32
            } else {
                unreachable!()
            };

            match ty {
                Type::NamedStructType {
                    name: _name,
                    ty: inner_ty,
                } => {
                    let inner_ty = inner_ty.as_ref().unwrap();
                    let inner_ty = inner_ty.upgrade().unwrap().read().unwrap().clone();
                    if let Type::StructType {
                        element_types,
                        is_packed,
                    } = &inner_ty
                    {
                        ty = element_types[index as usize].clone();
                        offset += offset_of(element_types, *is_packed, index as u32) as u32;
                    } else {
                        todo!("{:?}", inner_ty)
                    }
                }
                Type::StructType {
                    element_types,
                    is_packed,
                } => {
                    ty = element_types[index as usize].clone();
                    offset += offset_of(&element_types, is_packed, index as u32) as u32;
                }
                Type::ArrayType {
                    element_type,
                    num_elements: _,
                } => {
                    let elem_size = type_layout(&element_type).pad_to_align().size();

                    ty = *element_type;
                    offset += elem_size as u32 * index as u32;
                }
                _ => todo!("{:?}", ty),
            }
        }

        println!("next type would be {:?}", ty);

        offset
    } else {
        todo!("{:?}", address)
    };

    println!("Result: {:?}", result);

    result
}

type GlobalVarList<'a> = HashMap<&'a Name, (u32, Option<Constant>)>;

fn compile_global_var_init<'a>(
    vars: &'a [GlobalVariable],
    funcs: &[Function],
) -> (Vec<Command>, GlobalVarList<'a>) {
    let mut globals = global_var_layout(vars);
    for (idx, func) in funcs.iter().enumerate() {
        let name = Box::leak(Box::new(Name::Name(func.name.clone())));
        globals.insert(name, (idx as u32 + 1, None));
    }

    let mut cmds = Vec::new();

    for var in vars {
        let (tmp, value) = one_global_var_init(var, &globals);
        cmds.extend(tmp);
        assert_eq!(globals.get_mut(&var.name).unwrap().1.replace(value), None);
    }

    // Currently used constants:
    // %%31BITSHIFT
    // %%4
    // %%SIXTEEN
    // %%256
    // %%2
    // %%1
    // %%65536
    // %%1024

    // TODO: This needs a better system
    static CONSTANTS: &[(&str, i32)] = &[
        ("%%31BITSHIFT", 1 << 31),
        ("%%ROW_SIZE", ROW_SIZE as i32),
        ("%%SIXTEEN", 16),
        ("%%-1", -1),
    ];

    for (name, value) in CONSTANTS {
        cmds.push(assign_lit(
            ScoreHolder::new(name.to_string()).unwrap(),
            *value,
        ));
    }

    for value in 0..31 {
        cmds.push(assign_lit(
            ScoreHolder::new(format!("%%{}", 1 << value)).unwrap(),
            1 << value,
        ));
    }

    (cmds, globals)
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

pub fn make_zeroed(ty: &Type) -> Constant {
    match ty {
        Type::NamedStructType {
            name: _,
            ty: struct_ty,
        } => {
            let struct_ty = struct_ty.as_ref().unwrap().upgrade().unwrap();
            let struct_ty = struct_ty.read().unwrap();
            make_zeroed(&struct_ty)
        }
        Type::StructType {
            element_types,
            is_packed,
        } => {
            let values = element_types.iter().map(|et| make_zeroed(et)).collect();
            Constant::Struct {
                name: None,
                values,
                is_packed: *is_packed,
            }
        }
        Type::ArrayType {
            element_type,
            num_elements,
        } => {
            let elements = std::iter::repeat(make_zeroed(element_type))
                .take(*num_elements)
                .collect();
            Constant::Array {
                element_type: (**element_type).clone(),
                elements,
            }
        }
        Type::IntegerType { bits } => Constant::Int {
            bits: *bits,
            value: 0,
        },
        _ => todo!("{:?}", ty),
    }
}

fn init_data(
    start_addr: i32,
    ty: &Type,
    mut value: Constant,
    globals: &GlobalVarList,
) -> BTreeMap<i32, u8> {
    if let Constant::AggregateZero(t) = value {
        value = make_zeroed(&t);
    }
    let value = value;

    match ty {
        Type::IntegerType { bits: 8 } => {
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals) {
                i8::try_from(c).unwrap() as u8
            } else {
                todo!("{:?}", value)
            };

            std::iter::once((start_addr, val)).collect()
        }
        Type::IntegerType { bits: 16 } => {
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals) {
                c
            } else {
                todo!("{:?}", value)
            };

            (val as u16)
                .to_le_bytes()
                .iter()
                .enumerate()
                .map(|(idx, byte)| (start_addr + idx as i32, *byte))
                .collect()
        }
        Type::IntegerType { bits: 32 } => {
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals) {
                c
            } else {
                todo!("{:?}", value)
            };

            val.to_le_bytes()
                .iter()
                .enumerate()
                .map(|(idx, byte)| (start_addr + idx as i32, *byte))
                .collect()
        }
        Type::IntegerType { bits: 64 } => {
            let val: u64 = if let Constant::Int { bits: 64, value } = value {
                value
            } else {
                todo!("{:?}", value)
            };

            val.to_le_bytes()
                .iter()
                .enumerate()
                .map(|(idx, byte)| (start_addr + idx as i32, *byte))
                .collect()
        }
        Type::PointerType {
            pointee_type: _,
            addr_space: _,
        } => {
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals) {
                c
            } else {
                todo!("{:?}", value)
            };

            val.to_le_bytes()
                .iter()
                .enumerate()
                .map(|(idx, byte)| (start_addr + idx as i32, *byte))
                .collect()
        }
        Type::ArrayType {
            element_type,
            num_elements,
        } => {
            let vals = match &value {
                Constant::Array {
                    element_type: et,
                    elements,
                } => {
                    assert_eq!(&**element_type, et);
                    elements
                }
                _ => todo!("{:?}", value),
            };

            assert_eq!(*num_elements, vals.len());

            vals.iter()
                .enumerate()
                .flat_map(|(idx, val)| {
                    let offset = offset_of_array(element_type, idx as u32);
                    let field_addr = start_addr + offset as i32;
                    init_data(field_addr, element_type, val.clone(), globals)
                })
                .collect()
        }
        Type::NamedStructType {
            name: _,
            ty: struct_ty,
        } => {
            let struct_ty = struct_ty.as_ref().unwrap().upgrade().unwrap();
            let struct_ty = struct_ty.read().unwrap();

            init_data(start_addr, &struct_ty, value, globals)
        }
        Type::StructType {
            element_types,
            is_packed,
        } => {
            let vals = match value {
                Constant::Struct {
                    name: _,
                    values,
                    is_packed: ip,
                } => {
                    assert_eq!(*is_packed, ip);
                    values
                }
                _ => todo!("data value {:?}", value),
            };

            assert_eq!(element_types.len(), vals.len());

            element_types
                .iter()
                .zip(vals.iter())
                .enumerate()
                .flat_map(|(idx, (field_ty, value))| {
                    let offset = offset_of(element_types, *is_packed, idx as u32);
                    let field_addr = start_addr + offset as i32;
                    init_data(field_addr, field_ty, value.clone(), globals)
                })
                .collect()
        }
        _ => todo!("data type {:?}", ty),
    }
}

fn one_global_var_init(v: &GlobalVariable, globals: &GlobalVarList) -> (Vec<Command>, Constant) {
    if matches!(v.name, Name::Number(_)) {
        todo!()
    }

    let start = globals.get(&v.name).unwrap().0;

    println!("evaluating {}", v.name);

    let temp = v.name.to_string();
    let target = ScoreHolder::new_lossy(format!("%@{}", &temp[1..temp.len() - 1]));

    match &v.ty {
        // I'm pretty sure it's *always* a pointer...
        Type::PointerType { pointee_type, .. } => {
            let bytes = init_data(
                start as i32,
                pointee_type,
                v.initializer.clone().unwrap(),
                globals,
            );

            let mut cmds = Vec::new();
            cmds.push(assign_lit(target, start as i32));

            if !bytes.is_empty() {
                let begin_addr = *bytes.iter().next().unwrap().0;
                let mut end_addr = bytes.iter().rev().next().unwrap().0 + 1;

                assert_eq!(begin_addr % 4, 0);

                if end_addr % 4 != 0 {
                    end_addr += 4 - (end_addr % 4)
                };

                let mut all_bytes = Vec::new();
                for a in begin_addr..end_addr {
                    all_bytes.push(bytes.get(&a).copied().unwrap_or(0));
                }

                assert_eq!(all_bytes.len() % 4, 0);

                let all_words = all_bytes
                    .chunks_exact(4)
                    .map(|word| i32::from_le_bytes(word.try_into().unwrap()));

                for (word_idx, word) in all_words.enumerate() {
                    cmds.push(set_memory(word, start as i32 + word_idx as i32 * 4));
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

fn compile_memset(
    arguments: &[(Operand, Vec<llvm_ir::function::ParameterAttribute>)],
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> Vec<Command> {
    if let [(dest, _), (value, _), (len, _), (volatile, _)] = &arguments[..] {
        let (mut cmds, dest1) = eval_operand(dest, globals);
        let (tmp, value1) = eval_operand(value, globals);
        cmds.extend(tmp);

        let len1 = if let Operand::ConstantOperand(Constant::Int { bits: 64, value }) = len {
            let len1 = get_unique_holder();
            cmds.push(assign_lit(len1.clone(), *value as i32));
            vec![len1]
        } else {
            let (tmp, len1) = eval_operand(len, globals);
            cmds.extend(tmp);
            len1
        };

        assert_eq!(dest1.len(), 1, "multiword pointer {:?}", dest);
        assert_eq!(value1.len(), 1, "multiword value {:?}", value);
        assert_eq!(len1.len(), 1, "multiword length {:?}", len);

        cmds.push(assign(param(0, 0), dest1[0].clone()));
        cmds.push(assign(param(1, 0), value1[0].clone()));
        cmds.push(assign(param(2, 0), len1[0].clone()));

        if !matches!(
            volatile,
            Operand::ConstantOperand(Constant::Int { bits: 1, value: 0 })
        ) {
            todo!("{:?}", volatile)
        }

        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:memset"),
            }
            .into(),
        );

        cmds
    } else {
        panic!("{:?}", arguments)
    }
}

fn compile_memcpy(
    arguments: &[(Operand, Vec<llvm_ir::function::ParameterAttribute>)],
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
) -> (Vec<Command>, Option<Vec<Command>>) {
    use llvm_ir::function::Attribute;
    let get_align = |attrs: &[Attribute]| -> Option<u64> {
        attrs
            .iter()
            .filter_map(|attr| {
                if let Attribute::EnumAttribute {
                    kind: 1,
                    value: Some(value),
                } = attr
                {
                    Some(value.get())
                } else {
                    None
                }
            })
            .next()
    };

    if let [(dest, dest_attr), (src, src_attr), (len, _), (volatile, _)] = &arguments[..] {
        let (mut cmds, src1) = eval_operand(src, globals);
        let (tmp, dest1) = eval_operand(dest, globals);
        cmds.extend(tmp);

        assert_eq!(src1.len(), 1, "multiword pointer {:?}", src);
        assert_eq!(dest1.len(), 1, "multiword pointer {:?}", dest);

        let src1 = src1.into_iter().next().unwrap();
        let dest1 = dest1.into_iter().next().unwrap();

        match (get_align(dest_attr), get_align(src_attr), len) {
            (_, _, Operand::ConstantOperand(Constant::Int { bits: 32, value }))
                if *value > 1024 =>
            {
                cmds.extend(push(ScoreHolder::new("%%fixup".to_string()).unwrap()));
                cmds.push(assign(param(0, 0), dest1));
                cmds.push(assign(param(1, 0), src1));
                cmds.push(assign_lit(param(2, 0), *value as i32));
                cmds.push(assign_lit(param(4, 0), 1));
                cmds.push(
                    McFuncCall {
                        id: McFuncId::new("intrinsic:memcpy%%fixup"),
                    }
                    .into(),
                );

                return (cmds, Some(Vec::new()));
            }
            (
                Some(d),
                Some(s),
                Operand::ConstantOperand(Constant::Int {
                    bits: 32,
                    value: len,
                }),
            ) if d % 4 == 0 && s % 4 == 0 => {
                let word_count = len / 4;
                let byte_count = len % 4;

                let tempsrc = get_unique_holder();
                let tempdst = get_unique_holder();

                cmds.push(Command::Comment(format!(
                    "Begin memcpy with src {} and dest {}",
                    src1, dest1
                )));
                cmds.push(assign(tempsrc.clone(), src1));
                cmds.push(assign(tempdst.clone(), dest1));
                cmds.push(assign_lit(param(4, 0), 0));

                let temp = get_unique_holder();

                for _ in 0..word_count {
                    cmds.push(assign(ptr(), tempsrc.clone()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:setptr"),
                        }
                        .into(),
                    );
                    cmds.push(read_ptr(temp.clone()));
                    cmds.push(make_op_lit(tempsrc.clone(), "+=", 4));

                    cmds.push(assign(ptr(), tempdst.clone()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:setptr"),
                        }
                        .into(),
                    );
                    cmds.push(write_ptr(temp.clone()));
                    cmds.push(make_op_lit(tempdst.clone(), "+=", 4));
                }

                for _ in 0..byte_count {
                    cmds.push(assign(ptr(), tempsrc.clone()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:load_byte"),
                        }
                        .into(),
                    );
                    cmds.push(make_op_lit(tempsrc.clone(), "+=", 1));

                    cmds.push(assign(param(2, 0), return_holder(0)));

                    cmds.push(assign(ptr(), tempdst.clone()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:store_byte"),
                        }
                        .into(),
                    );
                    cmds.push(make_op_lit(tempdst.clone(), "+=", 1));
                }

                cmds.push(Command::Comment("End memcpy".into()));
            }
            _ => {
                let (tmp, len1) = eval_operand(len, globals);
                cmds.extend(tmp);

                assert_eq!(len1.len(), 1, "multiword length {:?}", len);
                let len1 = len1.into_iter().next().unwrap();

                cmds.push(assign(param(0, 0), dest1));
                cmds.push(assign(param(1, 0), src1));
                cmds.push(assign(param(2, 0), len1));
                cmds.push(assign_lit(param(4, 0), 0));

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
            }
        };

        (cmds, None)
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
                before_cmds.push(assign_lit(param(idx, 0), score));
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

fn compile_xor(
    Xor {
        operand0,
        operand1,
        dest,
        debugloc: _,
    }: &Xor,
    globals: &GlobalVarList,
) -> Vec<Command> {
    assert_eq!(operand0.get_type(), operand1.get_type());

    let (mut cmds, op0) = eval_operand(operand0, globals);

    let (tmp, op1) = eval_operand(operand1, globals);

    cmds.extend(tmp);

    let layout = type_layout(&operand0.get_type());

    let dest = ScoreHolder::from_local_name(dest.clone(), layout.size());

    if matches!(operand0.get_type(), Type::IntegerType { bits: 1 }) {
        let dest = dest.into_iter().next().unwrap();

        let op0 = op0.into_iter().next().unwrap();
        let op1 = op1.into_iter().next().unwrap();

        cmds.push(assign_lit(dest.clone(), 0));

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
        lhs_1.with_run(assign_lit(dest.clone(), 1));

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
        rhs_1.with_run(assign_lit(dest, 1));

        cmds.push(lhs_1.into());
        cmds.push(rhs_1.into());

        cmds
    } else {
        for (dest, (op0, op1)) in dest.into_iter().zip(op0.into_iter().zip(op1.into_iter())) {
            cmds.push(assign(param(0, 0), op0));
            cmds.push(assign(param(1, 0), op1));

            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:xor"),
                }
                .into(),
            );

            cmds.push(assign(dest, return_holder(0)));
        }

        cmds
    }
}

fn compile_shl(
    Shl {
        operand0,
        operand1,
        dest,
        debugloc: _,
    }: &Shl,
    globals: &GlobalVarList,
) -> Vec<Command> {
    if matches!(operand0.get_type(), Type::IntegerType { bits: 64 })
        && matches!(
            operand1,
            Operand::ConstantOperand(Constant::Int {
                bits: 64,
                value: 32
            })
        )
    {
        let (mut cmds, op0) = eval_operand(operand0, globals);
        let dest = ScoreHolder::from_local_name(dest.clone(), 8);

        cmds.push(assign_lit(dest[0].clone(), 0));
        cmds.push(assign(dest[1].clone(), op0[0].clone()));

        cmds
    } else if matches!(operand0.get_type(), Type::IntegerType { bits: 64 }) {
        let (dest_lo, dest_hi) =
            if let [dest_lo, dest_hi] = &ScoreHolder::from_local_name(dest.clone(), 6)[..] {
                (dest_lo.clone(), dest_hi.clone())
            } else {
                unreachable!()
            };

        let shift = if let Operand::ConstantOperand(Constant::Int { bits: 64, value }) = operand1 {
            *value
        } else {
            todo!("{:?}", operand1)
        };

        let (mut cmds, op0) = eval_operand(operand0, globals);

        cmds.push(mark_assertion(
            false,
            &ExecuteCondition::Score {
                target: op0[1].clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ExecuteCondKind::Matches((0..=0).into()),
            },
        ));

        let mut intra_byte = |max_bound: i32, dest_lo: ScoreHolder, dest_hi: ScoreHolder| {
            let mul = 1 << shift;
            cmds.push(mark_assertion(
                false,
                &ExecuteCondition::Score {
                    target: op0[0].clone().into(),
                    target_obj: OBJECTIVE.into(),
                    kind: ExecuteCondKind::Matches((0..=max_bound).into()),
                },
            ));

            cmds.push(assign(dest_lo.clone(), op0[0].clone()));
            cmds.push(assign_lit(dest_hi, 0));
            let tmp = get_unique_holder();
            cmds.push(assign_lit(tmp.clone(), mul));
            cmds.push(
                ScoreOp {
                    target: dest_lo.into(),
                    target_obj: OBJECTIVE.into(),
                    kind: ScoreOpKind::MulAssign,
                    source: tmp.into(),
                    source_obj: OBJECTIVE.into(),
                }
                .into(),
            );
        };

        match shift {
            8 => intra_byte(0x00_FF_FF_FF, dest_lo, dest_hi),
            16 => intra_byte(0x00_00_FF_FF, dest_lo, dest_hi),
            24 => intra_byte(0x00_00_00_FF, dest_lo, dest_hi),
            32 => todo!(),
            _ => todo!("{:?}", operand1),
        };

        cmds
    } else if matches!(operand0.get_type(), Type::IntegerType { bits: 48 }) {
        let (dest_lo, dest_hi) =
            if let [dest_lo, dest_hi] = &ScoreHolder::from_local_name(dest.clone(), 6)[..] {
                (dest_lo.clone(), dest_hi.clone())
            } else {
                unreachable!()
            };

        let shift = if let Operand::ConstantOperand(Constant::Int { bits: 48, value }) = operand1 {
            *value
        } else {
            todo!("{:?}", operand1)
        };

        let (mut cmds, op0) = eval_operand(operand0, globals);

        cmds.push(mark_assertion(
            false,
            &ExecuteCondition::Score {
                target: op0[1].clone().into(),
                target_obj: OBJECTIVE.into(),
                kind: ExecuteCondKind::Matches((0..=0).into()),
            },
        ));

        let mut intra_byte = |max_bound: i32, dest_lo: ScoreHolder, dest_hi: ScoreHolder| {
            let mul = 1 << shift;
            cmds.push(mark_assertion(
                false,
                &ExecuteCondition::Score {
                    target: op0[0].clone().into(),
                    target_obj: OBJECTIVE.into(),
                    kind: ExecuteCondKind::Matches((0..=max_bound).into()),
                },
            ));

            cmds.push(assign(dest_lo.clone(), op0[0].clone()));
            cmds.push(assign_lit(dest_hi, 0));
            let tmp = get_unique_holder();
            cmds.push(assign_lit(tmp.clone(), mul));
            cmds.push(
                ScoreOp {
                    target: dest_lo.into(),
                    target_obj: OBJECTIVE.into(),
                    kind: ScoreOpKind::MulAssign,
                    source: tmp.into(),
                    source_obj: OBJECTIVE.into(),
                }
                .into(),
            );
        };

        match shift {
            8 => intra_byte(0x00_FF_FF_FF, dest_lo, dest_hi),
            16 => intra_byte(0x00_00_FF_FF, dest_lo, dest_hi),
            24 => intra_byte(0x00_00_00_FF, dest_lo, dest_hi),
            32 => {
                cmds.push(mark_assertion(
                    false,
                    &ExecuteCondition::Score {
                        target: op0[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=0xFF).into()),
                    },
                ));

                cmds.push(assign_lit(dest_lo, 0));
                cmds.push(assign(dest_hi, op0[0].clone()));
            }
            _ => todo!("{:?}", operand1),
        };

        cmds
    } else {
        let bits = match operand0.get_type() {
            Type::IntegerType { bits: 32 } => 32,
            Type::IntegerType { bits: 24 } => 24,
            Type::IntegerType { bits: 16 } => 16,
            Type::IntegerType { bits: 8 } => 8,
            _ => todo!("{:?}, shift: {:?}", operand0, operand1),
        };

        let (mut cmds, op0) = eval_operand(operand0, globals);
        let op0 = op0.into_iter().next().unwrap();

        let dest = ScoreHolder::from_local_name(dest.clone(), 4)
            .into_iter()
            .next()
            .unwrap();

        match eval_maybe_const(operand1, globals) {
            MaybeConst::Const(c) => {
                cmds.push(assign(dest.clone(), op0));
                cmds.push(make_op_lit(dest.clone(), "*=", 1 << c));

                if !matches!(operand0.get_type(), Type::IntegerType { bits: 32 }) {
                    let max_val = match operand0.get_type() {
                        Type::IntegerType { bits: 8 } => 255,
                        Type::IntegerType { bits: 16 } => 65535,
                        Type::IntegerType { bits: 24 } => 16777216,
                        Type::IntegerType { bits: 32 } => unreachable!(),
                        ty => todo!("{:?}", ty),
                    };

                    cmds.push(mark_assertion(
                        false,
                        &ExecuteCondition::Score {
                            target: dest.into(),
                            target_obj: OBJECTIVE.into(),
                            kind: ExecuteCondKind::Matches((..=max_val).into()),
                        },
                    ))
                }
            }
            MaybeConst::NonConst(tmp, op1) => {
                let op1 = op1.into_iter().next().unwrap();

                cmds.extend(tmp);
                cmds.push(assign(param(0, 0), op0));
                cmds.push(assign(param(1, 0), op1));
                cmds.push(
                    McFuncCall {
                        id: McFuncId::new("intrinsic:shl"),
                    }
                    .into(),
                );
                cmds.push(assign(dest.clone(), param(0, 0)));
                if bits < 32 {
                    cmds.push(make_op_lit(dest, "%=", 1 << bits));
                }
            }
        }

        cmds
    }
}

fn compile_lshr(
    LShr {
        operand0,
        operand1,
        dest,
        debugloc: _,
    }: &LShr,
    globals: &GlobalVarList,
) -> Vec<Command> {
    let (mut cmds, op0) = eval_operand(operand0, globals);

    if let Operand::ConstantOperand(Constant::Int { bits: 64, value }) = operand1 {
        if matches!(operand0.get_type(), Type::IntegerType { bits: 64 }) {
            cmds.extend(lshr_64_bit_const(
                op0[0].clone(),
                op0[1].clone(),
                *value as i32,
                dest.clone(),
            ));

            cmds
        } else {
            unreachable!()
        }
    } else {
        if let Type::IntegerType { bits } = operand0.get_type() {
            if bits > 32 {
                todo!("{:?}, {:?}", operand0, operand1);
            }
        } else {
            todo!("{:?}", operand0);
        }

        let dest = ScoreHolder::from_local_name(dest.clone(), 4)
            .into_iter()
            .next()
            .unwrap();

        let (tmp, op1) = eval_operand(operand1, globals);
        let op1 = op1.into_iter().next().unwrap();

        cmds.extend(tmp);
        cmds.push(assign(param(0, 0), op0[0].clone()));
        cmds.push(assign(param(1, 0), op1));
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

                let (mut cmds, op) = eval_operand(&arguments[0].0, globals);

                cmds.push(mark_assertion(
                    false,
                    &ExecuteCondition::Score {
                        target: op[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((1..=1).into()),
                    },
                ));

                (cmds, None)
            }
            "insert_asm" => {
                assert_eq!(arguments.len(), 3);

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

                let (mut cmds, arg) = eval_operand(&arguments[2].0, globals);
                let arg = arg.into_iter().next().unwrap();

                let interpolated = text
                    .replace("$0", &arg.to_string())
                    .replace("$obj", OBJECTIVE);

                let cmd = interpolated.parse().unwrap();

                cmds.push(cmd);

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
                cmds.push(assign_lit(dest.clone(), b' ' as i32));

                let mut valid_chars = (b'A'..=b'Z').collect::<Vec<_>>();
                valid_chars.extend(b'0'..=b'9');
                valid_chars.push(b'[');
                valid_chars.push(b']');
                valid_chars.push(b'(');
                valid_chars.push(b')');
                valid_chars.push(b'{');
                valid_chars.push(b'}');
                valid_chars.push(b'=');
                valid_chars.push(b'%');
                valid_chars.push(b'+');
                valid_chars.push(b'<');

                for c in valid_chars {
                    let is_white =
                        c == b'H' || c == b'Q' || c == b'S' || c == b')' || c == b'(' || c == b'=';

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
                    cmd.with_run(assign_lit(dest.clone(), c as i32));
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
                cmds.push(assign_lit(dest.clone(), 0));

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
                    cmd.with_run(assign_lit(dest.clone(), *block as i32));
                    cmds.push(cmd.into());
                }

                (cmds, None)
            }
            "llvm.dbg.label" => (vec![], None),
            "llvm.dbg.declare" => (vec![], None),
            "llvm.dbg.value" => (vec![], None),
            "llvm.ctlz.i32" => {
                let dest = dest.unwrap().into_iter().next().unwrap();
                let mut cmds = Vec::new();

                match eval_maybe_const(&arguments[0].0, globals) {
                    MaybeConst::Const(c) => {
                        cmds.push(assign_lit(dest, c.leading_zeros() as i32));
                    }
                    MaybeConst::NonConst(tmp, op) => {
                        let op = op.into_iter().next().unwrap();
                        cmds.extend(tmp);
                        cmds.push(assign(param(0, 0), op));
                        cmds.push(
                            McFuncCall {
                                id: McFuncId::new("intrinsic:llvm_ctlz_i32"),
                            }
                            .into(),
                        );
                        cmds.push(assign(dest, return_holder(0)));
                    }
                }

                (cmds, None)
            }
            "llvm.usub.with.overflow.i32" => {
                let mut dest_words = dest.unwrap().into_iter();
                let dest_value = dest_words.next().unwrap();
                let dest_flag = dest_words.next().unwrap();

                let mut cmds = Vec::new();
                cmds.push(assign_lit(dest_flag.clone(), 0));

                cmds.extend(setup_arguments(arguments, globals));

                cmds.push(make_op_lit(param(1, 0), "*=", -1));

                cmds.extend(add_32_with_carry(
                    param(0, 0),
                    param(1, 0),
                    dest_value,
                    assign_lit(dest_flag, 1),
                ));

                (cmds, None)
            }
            "llvm.fshr.i32" => {
                let dest = dest.unwrap().into_iter().next().unwrap();
                let mut cmds = setup_arguments(arguments, globals);
                cmds.push(
                    McFuncCall {
                        id: McFuncId::new("intrinsic:llvm_fshr_i32"),
                    }
                    .into(),
                );
                cmds.push(assign(dest, return_holder(0)));
                (cmds, None)
            }
            "llvm.memset.p0i8.i32" | "llvm.memset.p0i8.i64" => {
                assert_eq!(dest, None);
                (compile_memset(arguments, globals), None)
            }
            "llvm.memcpy.p0i8.p0i8.i32" => {
                assert_eq!(dest, None);
                compile_memcpy(arguments, globals)
            }
            "llvm.usub.with.overflow.i8" => {
                let dest = dest.unwrap().into_iter().next().unwrap();

                if let [(lhs, _), (rhs, _)] = &arguments[..] {
                    let (mut cmds, l) = eval_operand(lhs, globals);
                    let (tmp, r) = eval_operand(rhs, globals);
                    cmds.extend(tmp);

                    let l = l.into_iter().next().unwrap();
                    let r = r.into_iter().next().unwrap();

                    let tmp = get_unique_holder();
                    cmds.push(assign(tmp.clone(), r));
                    cmds.push(make_op_lit(tmp.clone(), "*=", -1));
                    // Zero out high bits
                    cmds.push(make_op_lit(tmp.clone(), "+=", 0x1_00));

                    cmds.push(assign(dest.clone(), l));
                    cmds.push(make_op(dest, "+=", tmp));
                    (cmds, None)
                } else {
                    unreachable!()
                }
            }
            "llvm.uadd.with.overflow.i8" => {
                let dest = dest.unwrap().into_iter().next().unwrap();

                if let [(lhs, _), (rhs, _)] = &arguments[..] {
                    let (mut cmds, l) = eval_operand(lhs, globals);
                    let (tmp, r) = eval_operand(rhs, globals);
                    cmds.extend(tmp);

                    let l = l.into_iter().next().unwrap();
                    let r = r.into_iter().next().unwrap();

                    cmds.push(assign(dest.clone(), l));
                    cmds.push(make_op(dest, "+=", r));
                    (cmds, None)
                } else {
                    unreachable!()
                }
            }
            "llvm.lifetime.start.p0i8" => {
                assert_eq!(dest, None);
                (vec![], None)
            }
            "llvm.lifetime.end.p0i8" => {
                assert_eq!(dest, None);
                (vec![], None)
            }
            "bcmp" => {
                assert_eq!(arguments.len(), 3);

                let dest = dest.as_ref().expect("bcmp should return a value");

                assert_eq!(dest.len(), 1, "wrong length for dest");
                let dest = dest[0].clone();

                let mut cmds = Vec::new();

                cmds.extend(setup_arguments(arguments, globals));

                cmds.push(
                    McFuncCall {
                        id: McFuncId::new("intrinsic:bcmp"),
                    }
                    .into(),
                );

                cmds.push(assign(dest, return_holder(0)));

                (cmds, None)
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

            // We don't actually want to use this, so we basically just `assert!(false)`
            before_cmds.push(mark_assertion(
                false,
                &ExecuteCondition::Score {
                    target: ScoreHolder::new("%%2".into()).unwrap().into(),
                    target_obj: OBJECTIVE.into(),
                    kind: ExecuteCondKind::Matches((0..=0).into()),
                },
            ));

            let temp_z = get_unique_holder();
            before_cmds.push(assign(temp_z.clone(), func_ptr.clone()));
            // FIXME: These should use ROW_SIZE
            before_cmds.push(make_op_lit(temp_z.clone(), "%=", 32));

            let temp_x = get_unique_holder();
            before_cmds.push(assign(temp_x.clone(), func_ptr));
            before_cmds.push(make_op_lit(temp_x.clone(), "/=", 32));
            before_cmds.push(make_op_lit(temp_x.clone(), "*=", -1));

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
                target: temp_z.into(),
                target_obj: OBJECTIVE.into(),
            });

            before_cmds.push(set_z.into());

            let mut set_x = Execute::new();
            set_x.with_as(
                cir::Selector {
                    var: cir::SelectorVariable::AllEntities,
                    args: vec![cir::SelectorArg("tag=ptr".into())],
                }
                .into(),
            );
            set_x.with_subcmd(ExecuteSubCmd::Store {
                is_success: false,
                kind: ExecuteStoreKind::Data {
                    target: DataTarget::Entity(
                        cir::Selector {
                            var: cir::SelectorVariable::ThisEntity,
                            args: Vec::new(),
                        }
                        .into(),
                    ),
                    path: "Pos[0]".into(),
                    ty: "double".into(),
                    scale: 1.0,
                },
            });
            set_x.with_run(ScoreGet {
                target: temp_x.into(),
                target_obj: OBJECTIVE.into(),
            });

            before_cmds.push(set_x.into());

            let mut set_block = Execute::new();
            set_block.with_at(
                cir::Selector {
                    var: cir::SelectorVariable::AllEntities,
                    args: vec![cir::SelectorArg("tag=ptr".into())],
                }
                .into(),
            );
            set_block.with_run(SetBlock {
                pos: "~-2 1 ~".into(),
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

pub fn compile_terminator(
    parent: &Function,
    term: &Terminator,
    globals: &GlobalVarList,
) -> Vec<Command> {
    let mut cmds = Vec::new();

    match &term {
        Terminator::Ret(Ret {
            return_operand: None,
            ..
        }) => {
            cmds.push(Command::Comment("return".to_string()));

            cmds.push(
                McFuncCall {
                    id: McFuncId::new("%%loadregs"),
                }
                .into(),
            );

            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:pop_and_branch"),
                }
                .into(),
            );

            cmds
        }
        Terminator::Ret(Ret {
            return_operand: Some(operand),
            ..
        }) => {
            cmds.push(Command::Comment(format!("return operand {:?}", operand)));

            let (tmp, source) = eval_operand(operand, globals);

            cmds.extend(tmp);

            for (idx, word) in source.into_iter().enumerate() {
                cmds.push(assign(return_holder(idx), word));
            }

            cmds.push(
                McFuncCall {
                    id: McFuncId::new("%%loadregs"),
                }
                .into(),
            );

            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:pop_and_branch"),
                }
                .into(),
            );

            cmds
        }
        Terminator::Br(Br { dest, .. }) => {
            let mut id = McFuncId::new_block(&parent.name, dest.clone());

            id.name.push_str("%%fixup");

            cmds.push(McFuncCall { id }.into());

            cmds
        }
        Terminator::CondBr(CondBr {
            condition,
            true_dest,
            false_dest,
            ..
        }) => {
            let (tmp, cond) = eval_operand(condition, globals);
            cmds.extend(tmp);

            assert_eq!(cond.len(), 1);
            let cond = cond[0].clone();

            let mut true_dest = McFuncId::new_block(&parent.name, true_dest.clone());
            let mut false_dest = McFuncId::new_block(&parent.name, false_dest.clone());

            true_dest.name.push_str("%%fixup");
            false_dest.name.push_str("%%fixup");

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

            cmds.push(true_cmd.into());
            cmds.push(false_cmd.into());

            cmds
        }
        Terminator::Switch(Switch {
            operand,
            dests,
            default_dest,
            ..
        }) => {
            let (tmp, operand) = eval_operand(operand, globals);
            cmds.extend(tmp);

            if operand.len() != 1 {
                todo!("multibyte operand in switch {:?}", operand);
            }

            let operand = operand[0].clone();

            let default_tracker = get_unique_holder();

            cmds.push(assign_lit(default_tracker.clone(), 0));

            for (dest_value, dest_name) in dests.iter() {
                let dest_value = if let Constant::Int { value, .. } = dest_value {
                    *value as i32
                } else {
                    todo!("{:?}", dest_value)
                };

                let mut dest_id = McFuncId::new_block(&parent.name, dest_name.clone());

                dest_id.name.push_str("%%fixup");

                let mut branch_cmd = Execute::new();
                branch_cmd.with_if(ExecuteCondition::Score {
                    target: Target::Uuid(operand.clone()),
                    target_obj: OBJECTIVE.to_string(),
                    kind: ExecuteCondKind::Matches(cir::McRange::Between(dest_value..=dest_value)),
                });

                let mut add_cmd = branch_cmd.clone();

                add_cmd.with_run(assign_lit(default_tracker.clone(), 1));
                branch_cmd.with_run(McFuncCall { id: dest_id });

                cmds.push(add_cmd.into());
                cmds.push(branch_cmd.into());
            }

            let mut default_dest = McFuncId::new_block(&parent.name, default_dest.clone());

            default_dest.name.push_str("%%fixup");

            let mut default_cmd = Execute::new();
            default_cmd.with_if(ExecuteCondition::Score {
                target: default_tracker.into(),
                target_obj: OBJECTIVE.to_string(),
                kind: ExecuteCondKind::Matches(cir::McRange::Between(0..=0)),
            });
            default_cmd.with_run(McFuncCall { id: default_dest });

            cmds.push(default_cmd.into());

            cmds
        }
        Terminator::Unreachable(Unreachable { .. }) => {
            cmds.push(mark_unreachable());
            cmds.push(
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
            );

            cmds
        }
        Terminator::Resume(_) => {
            cmds.push(mark_todo());

            let message = cir::TextBuilder::new()
                .append_text("OH NO EXCEPTION HANDLING TOOD".into())
                .build();

            cmds.push(
                Tellraw {
                    target: cir::Selector {
                        var: cir::SelectorVariable::AllPlayers,
                        args: Vec::new(),
                    }
                    .into(),
                    message,
                }
                .into(),
            );

            cmds
        }
        term => todo!("terminator {:?}", term),
    }
}

pub fn compile_function(
    func: &Function,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
    options: &BuildOptions,
) -> (Vec<McFunction>, HashMap<ScoreHolder, cir::HolderUse>) {
    if func.is_var_arg {
        todo!("functions with variadic arguments");
    }

    if func.basic_blocks.is_empty() {
        todo!("functions with no basic blocks");
    }

    println!("Function {}, {}", func.name, func.basic_blocks.len());

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

            this.cmds.push(assign_lit(
                ScoreHolder::new("%phi".to_string()).unwrap(),
                idx as i32,
            ));

            this.cmds
                .extend(compile_terminator(&func, &block.term, globals));

            result.push(this);

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

            result
        })
        .collect::<Vec<_>>();

    let mut clobbers = HashMap::new();
    for f in funcs.iter() {
        for c in f.cmds.iter() {
            cir::merge_uses(&mut clobbers, &c.holder_uses());
        }
    }

    let clobbers = clobbers
        .into_iter()
        .map(|(c, u)| ((*c).clone(), u))
        .collect();

    (funcs, clobbers)
}

pub fn lshr_64_bit_const(
    op0_lo: ScoreHolder,
    op0_hi: ScoreHolder,
    amount: i32,
    dest: Name,
) -> Vec<Command> {
    let mut dest = ScoreHolder::from_local_name(dest, 8).into_iter();
    let dest_lo = dest.next().unwrap();
    let dest_hi = dest.next().unwrap();

    let mut cmds = Vec::new();

    if amount >= 32 {
        cmds.push(assign_lit(dest_hi, 0));

        if amount > 32 {
            cmds.push(assign(param(0, 0), op0_hi));
            cmds.push(assign_lit(param(1, 0), amount - 32));
            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:lshr"),
                }
                .into(),
            );
            cmds.push(assign(dest_lo, param(0, 0)));
        } else {
            cmds.push(assign(dest_lo, op0_lo));
        }

        cmds
    } else {
        let temp = get_unique_holder();

        // temp = (hi & mask) << (32 - amount)
        cmds.push(assign(temp.clone(), op0_hi.clone()));
        cmds.push(make_op_lit(temp.clone(), "%=", 1 << amount));
        cmds.push(make_op_lit(temp.clone(), "*=", 1 << (32 - amount)));

        // hi' = hi lshr amount
        cmds.push(assign(param(0, 0), op0_hi));
        cmds.push(assign_lit(param(1, 0), amount));
        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:lshr"),
            }
            .into(),
        );
        cmds.push(assign(dest_hi, param(0, 0)));

        // lo' = (lo lshr amount) + temp
        cmds.push(assign(param(0, 0), op0_lo));
        cmds.push(assign_lit(param(1, 0), amount));
        cmds.push(
            McFuncCall {
                id: McFuncId::new("intrinsic:lshr"),
            }
            .into(),
        );
        cmds.push(assign(dest_lo.clone(), param(0, 0)));
        cmds.push(make_op(dest_lo, "+=", temp));

        cmds
    }
}

pub fn mul_64_bit(
    op0_lo: ScoreHolder,
    op0_hi: ScoreHolder,
    op1_lo: ScoreHolder,
    op1_hi: ScoreHolder,
    dest: Name,
) -> Vec<Command> {
    // x_l*y_l + (x_h*y_l + x_l*y_h)*2^32

    let mut dest = ScoreHolder::from_local_name(dest, 8).into_iter();
    let dest_lo = dest.next().unwrap();
    let dest_hi = dest.next().unwrap();

    let mut cmds = Vec::new();

    cmds.push(assign(param(0, 0), op0_lo));
    cmds.push(assign(param(1, 0), op1_lo));
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:mul_32_to_64"),
        }
        .into(),
    );
    cmds.push(assign(dest_lo, return_holder(0)));
    cmds.push(assign(dest_hi.clone(), return_holder(1)));

    cmds.push(make_op(param(0, 0), "*=", op1_hi));
    cmds.push(make_op(param(1, 0), "*=", op0_hi));

    cmds.push(make_op(dest_hi.clone(), "+=", param(0, 0)));
    cmds.push(make_op(dest_hi, "+=", param(1, 0)));

    cmds
}

// 64-bit addition:
// To detect a carry between 32-bit signed integers `a` and `b`:
// let added = a.wrapping_add(b);
// if a < 0 && b < 0 { result = true }
// if a < 0 && b >= 0 && added >= 0 { result = true }
// if a >= 0 && b < 0 && added >= 0 { result = true }

pub fn add_32_with_carry(
    op0: ScoreHolder,
    op1: ScoreHolder,
    dest: ScoreHolder,
    on_carry: Command,
) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(assign(dest.clone(), op0.clone()));
    cmds.push(make_op(dest.clone(), "+=", op1.clone()));

    let mut both_neg = Execute::new();
    both_neg.with_if(ExecuteCondition::Score {
        target: op0.clone().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    both_neg.with_if(ExecuteCondition::Score {
        target: op1.clone().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    both_neg.with_run(on_carry.clone());
    cmds.push(both_neg.into());

    let mut op0_neg = Execute::new();
    op0_neg.with_if(ExecuteCondition::Score {
        target: op0.clone().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    op0_neg.with_if(ExecuteCondition::Score {
        target: op1.clone().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    op0_neg.with_if(ExecuteCondition::Score {
        target: dest.clone().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    op0_neg.with_run(on_carry.clone());
    cmds.push(op0_neg.into());

    let mut op1_neg = Execute::new();
    op1_neg.with_if(ExecuteCondition::Score {
        target: op0.into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    op1_neg.with_if(ExecuteCondition::Score {
        target: op1.into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((..=-1).into()),
    });
    op1_neg.with_if(ExecuteCondition::Score {
        target: dest.into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((0..).into()),
    });
    op1_neg.with_run(on_carry);
    cmds.push(op1_neg.into());

    cmds
}

pub fn add_64_bit(
    op0_lo: ScoreHolder,
    op0_hi: ScoreHolder,
    op1_lo: ScoreHolder,
    op1_hi: ScoreHolder,
    dest: Name,
) -> Vec<Command> {
    let mut dest_iter = ScoreHolder::from_local_name(dest, 8).into_iter();
    let dest_lo = dest_iter.next().unwrap();
    let dest_hi = dest_iter.next().unwrap();

    let mut cmds = Vec::new();

    cmds.push(assign(dest_hi.clone(), op0_hi));
    cmds.push(make_op(dest_hi.clone(), "+=", op1_hi));

    cmds.extend(add_32_with_carry(
        op0_lo,
        op1_lo,
        dest_lo,
        make_op_lit(dest_hi, "+=", 1),
    ));

    cmds
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

    if matches!(operand0.get_type(), Type::IntegerType { bits: 64 }) {
        let op0_lo = source0[0].clone();
        let op0_hi = source0[1].clone();

        let op1_lo = source1[0].clone();
        let op1_hi = source1[1].clone();

        match kind {
            ScoreOpKind::AddAssign => {
                cmds.extend(add_64_bit(op0_lo, op0_hi, op1_lo, op1_hi, dest.clone()));
            }
            ScoreOpKind::MulAssign => {
                cmds.extend(mul_64_bit(op0_lo, op0_hi, op1_lo, op1_hi, dest.clone()));
            }
            _ => todo!("{:?}", kind),
        }

        cmds
    } else {
        let dest =
            ScoreHolder::from_local_name(dest.clone(), type_layout(&operand0.get_type()).size());

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
            assert_eq!(source0.len(), 1, "{:?}", kind);
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

pub fn offset_of_array(element_type: &Type, field: u32) -> usize {
    let mut offset = 0;
    let mut result = Layout::from_size_align(0, 1).unwrap();
    for _ in 0..field + 1 {
        let (r, o) = result.extend(type_layout(element_type)).unwrap();
        offset = o;
        result = r;
    }
    offset
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
        Type::IntegerType { bits: 24 } => Layout::from_size_align(3, 4).unwrap(),
        Type::IntegerType { bits: 32 } => Layout::from_size_align(4, 4).unwrap(),
        Type::IntegerType { bits: 48 } => Layout::from_size_align(6, 4).unwrap(),
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
    cmds.push(assign_lit(dest.clone(), invert as i32));

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
    check1.with_run(assign_lit(dest.clone(), !invert as i32));
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
    check2.with_run(assign_lit(dest.clone(), !invert as i32));
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
    check3.with_run(assign_lit(dest.clone(), !invert as i32));
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
        eq_check.with_run(assign_lit(dest, !invert as i32));
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
        cmds.push(make_op_lit(holder.clone(), "*=", 256));
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
        cmds.push(make_op_lit(holder.clone(), "/=", 256));
    }

    cmds
}

/// Zeros out the lowest `bytes` bytes of `holder`
/// Clobbers %param0%0 and %param1%0
fn zero_low_bytes(holder: ScoreHolder, bytes: u32) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(Command::Comment(format!(
        "zero {} lowest bytes of {}",
        bytes, holder
    )));

    // Zero out the lower bits
    cmds.push(assign(param(0, 0), holder.clone()));
    cmds.push(assign_lit(param(1, 0), bytes as i32 * 8));
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:lshr"),
        }
        .into(),
    );
    cmds.push(assign(holder.clone(), param(0, 0)));

    for _ in 0..bytes {
        cmds.push(make_op_lit(holder.clone(), "*=", 256));
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

    cmds.extend(zero_low_bytes(top_bits.clone(), bytes));

    cmds.push(make_op(holder, "-=", top_bits));

    cmds
}

fn compile_getelementptr(
    GetElementPtr {
        address,
        indices,
        dest,
        in_bounds: _,
        debugloc: _,
    }: &GetElementPtr,
    globals: &GlobalVarList,
) -> Vec<Command> {
    let dest = ScoreHolder::from_local_name(dest.clone(), 4);
    let dest = dest[0].clone();

    let mut offset: i32 = 0;
    let mut ty = address.get_type();

    let mut cmds = Vec::new();

    assert!(matches!(ty, Type::PointerType { .. }));

    for index in indices {
        match ty {
            Type::PointerType { pointee_type, .. } => {
                let pointee_size = type_layout(&pointee_type).pad_to_align().size();

                ty = *pointee_type;

                match eval_maybe_const(index, globals) {
                    MaybeConst::Const(c) => offset += pointee_size as i32 * c,
                    MaybeConst::NonConst(a, b) => {
                        assert_eq!(b.len(), 1);
                        let b = b.into_iter().next().unwrap();

                        cmds.extend(a);
                        for _ in 0..pointee_size {
                            cmds.push(make_op(dest.clone(), "+=", b.clone()));
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

                offset += offset_of(&element_types, is_packed, index as u32) as i32;

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
                    offset += offset_of(element_types, *is_packed, index as u32) as i32;

                    ty = element_types[index as usize].clone();
                } else {
                    todo!("{:?}", &*struct_ty);
                }
            }
            Type::ArrayType { element_type, .. } => {
                let elem_size = type_layout(&element_type).pad_to_align().size();

                match eval_maybe_const(index, globals) {
                    MaybeConst::Const(c) => {
                        offset += c * elem_size as i32;
                    }
                    MaybeConst::NonConst(a, b) => {
                        assert_eq!(b.len(), 1);
                        let b = b.into_iter().next().unwrap();

                        cmds.extend(a);
                        for _ in 0..elem_size {
                            cmds.push(make_op(dest.clone(), "+=", b.clone()));
                        }
                    }
                }

                ty = *element_type;
            }
            _ => todo!("{:?}", ty),
        }
    }

    let mut start_cmds = match eval_maybe_const(address, globals) {
        MaybeConst::Const(addr) => vec![assign_lit(dest.clone(), addr + offset as i32)],
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

pub fn compile_normal_icmp(
    target: ScoreHolder,
    source: ScoreHolder,
    predicate: &IntPredicate,
    dest: ScoreHolder,
) -> Vec<Command> {
    let mut cmds = Vec::new();

    /* for IntPredicate::Eq
    cmds.push(assign_lit(dest.clone(), 0));

    let mut exec = Execute::new();
    for (t, s) in target.into_iter().zip(source.into_iter()) {
        exec.with_if(ExecuteCondition::Score {
            target: t.into(),
            target_obj: OBJECTIVE.into(),
            kind: ExecuteCondKind::Relation {
                source: s.into(),
                source_obj: OBJECTIVE.into(),
                relation: cir::Relation::Eq,
            }
        });
    }
    exec.with_run(assign_lit(dest, 1));

    cmds
    */

    let signed_cmp =
        |rel, normal| compile_signed_cmp(target.clone(), source.clone(), dest.clone(), rel, normal);

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

pub fn compile_instr(
    instr: &Instruction,
    parent: &Function,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
    _options: &BuildOptions,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let result = match instr {
        // We use an empty stack
        Instruction::Alloca(alloca) => compile_alloca(alloca),
        Instruction::GetElementPtr(gep) => compile_getelementptr(gep, globals),
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
        Instruction::Store(Store {
            address,
            value,
            alignment,
            ..
        }) => {
            let (mut cmds, addr) = eval_operand(address, globals);

            assert_eq!(addr.len(), 1, "multiword addr {:?}", address);

            let addr = addr[0].clone();

            let value_size = type_layout(&value.get_type()).size();
            if value_size % 4 == 0 && alignment % 4 == 0 {
                // If we're directly storing a constant,
                // we can skip writing to a temporary value
                let write_cmds = match eval_maybe_const(value, globals) {
                    MaybeConst::Const(value) => vec![write_ptr_const(value)],
                    MaybeConst::NonConst(eval_cmds, ids) => {
                        cmds.extend(eval_cmds);

                        ids.into_iter().map(write_ptr).collect()
                    }
                };

                let tmp = get_unique_holder();
                cmds.push(assign(tmp.clone(), addr.clone()));
                cmds.push(make_op_lit(tmp.clone(), "%=", 4));
                cmds.push(mark_assertion(
                    false,
                    &ExecuteCondition::Score {
                        target: tmp.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=0).into()),
                    },
                ));

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
            } else if value_size == 2 {
                let (eval_cmds, value) = eval_operand(value, globals);
                cmds.extend(eval_cmds);
                let value = value.into_iter().next().unwrap();

                cmds.push(assign(ptr(), addr));

                if alignment % 2 == 0 {
                    cmds.push(assign(param(2, 0), value));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:store_halfword"),
                        }
                        .into(),
                    )
                } else {
                    cmds.push(assign(param(0, 0), value));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:store_halfword_unaligned"),
                        }
                        .into(),
                    )
                }
            } else if value_size % 4 == 0 {
                /*if let Operand::ConstantOperand(Constant::Int { bits: 64, value }) = value {
                for (idx, byte) in value.to_le_bytes().iter().copied().enumerate() {
                    cmds.push(assign(ptr(), addr.clone()));
                    cmds.push(ScoreAdd {
                        target: ptr().into(),
                        target_obj: OBJECTIVE.into(),
                        score: idx as i32,
                    }.into());
                    cmds.push(ScoreSet {
                        target: param(2, 0).into(),
                        target_obj: OBJECTIVE.into(),
                        score: byte as i32,
                    }.into());
                    cmds.push(McFuncCall {
                        id: McFuncId::new("intrinsic:store_byte"),
                    }.into());
                }
                */
                let (tmp, val) = eval_operand(value, globals);
                cmds.extend(tmp);

                for (word_idx, val) in val.into_iter().enumerate() {
                    cmds.push(assign(ptr(), addr.clone()));
                    cmds.push(
                        ScoreAdd {
                            target: ptr().into(),
                            target_obj: OBJECTIVE.into(),
                            score: 4 * word_idx as i32,
                        }
                        .into(),
                    );
                    cmds.push(assign(param(0, 0), val));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:store_word_unaligned"),
                        }
                        .into(),
                    );
                }
            } else if value_size < 4 {
                let (tmp, val) = eval_operand(value, globals);
                cmds.extend(tmp);

                let val = val.into_iter().next().unwrap();

                /*
                scoreboard players operation %param0%0 rust = %%temp0_swu rust
                scoreboard players set %param1%0 rust 8
                function intrinsic:lshr
                scoreboard players operation %param2%0 rust = %param0%0 rust
                scoreboard players operation %param2%0 rust %= %%256 rust
                function intrinsic:store_byte
                scoreboard players add %ptr rust 1*/

                cmds.push(assign(ptr(), addr));

                for byte_idx in 0..value_size {
                    cmds.push(assign(param(0, 0), val.clone()));
                    cmds.push(assign_lit(param(1, 0), 8 * byte_idx as i32));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:lshr"),
                        }
                        .into(),
                    );
                    cmds.push(assign(param(2, 0), param(0, 0)));
                    cmds.push(make_op_lit(param(2, 0), "%=", 256));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:store_byte"),
                        }
                        .into(),
                    );
                    cmds.push(make_op_lit(ptr(), "+=", 1));
                }
            } else {
                todo!("{:?} {}", value, alignment)
            }

            cmds
        }
        Instruction::Load(Load {
            dest,
            address,
            alignment,
            ..
        }) => {
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

            if pointee_layout.size() % 4 == 0 && pointee_layout.align() == 4 && alignment % 4 == 0 {
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
            } else if pointee_layout.size() % 4 == 0 && *alignment == 1 {
                for (word_idx, dest_word) in dest.into_iter().enumerate() {
                    cmds.push(assign(ptr(), addr.clone()));
                    cmds.push(
                        ScoreAdd {
                            target: ptr().into(),
                            target_obj: OBJECTIVE.into(),
                            score: word_idx as i32 * 4,
                        }
                        .into(),
                    );
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:load_word_unaligned"),
                        }
                        .into(),
                    );
                    cmds.push(assign(dest_word, return_holder(0)));
                }
            } else if pointee_layout.size() == 1 {
                cmds.push(assign(ptr(), addr));
                cmds.extend(read_ptr_small(dest[0].clone(), false));
            } else if pointee_layout.size() == 2 {
                cmds.push(assign(ptr(), addr));

                if alignment % 2 == 0 {
                    cmds.extend(read_ptr_small(dest[0].clone(), true));
                } else {
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:load_halfword_unaligned"),
                        }
                        .into(),
                    );
                    cmds.push(assign(dest[0].clone(), return_holder(0)));
                }
            } else if pointee_layout.size() < 4 && alignment % 4 == 0 {
                cmds.push(assign(ptr(), addr));
                cmds.push(read_ptr(dest[0].clone()));
                if pointee_layout.size() == 3 {
                    cmds.push(make_op_lit(dest[0].clone(), "%=", 16777216));
                } else {
                    todo!("{:?}", pointee_layout)
                }
            } else {
                todo!(
                    "{:?} with layout {:?} and pointer alignment {}",
                    pointee_type,
                    pointee_layout,
                    alignment
                )
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
        Instruction::SDiv(SDiv {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::DivAssign, globals),
        Instruction::SRem(SRem {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::ModAssign, globals),
        Instruction::UDiv(UDiv {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            let (mut cmds, source0) = eval_operand(operand0, globals);
            let (tmp, source1) = eval_operand(operand1, globals);
            cmds.extend(tmp.into_iter());

            // FIXME: THIS DOES AN SREM
            for s in source0.iter().cloned() {
                cmds.push(mark_assertion(
                    true,
                    &ExecuteCondition::Score {
                        target: s.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((..=-1).into()),
                    },
                ));
            }

            for s in source1.iter().cloned() {
                cmds.push(mark_assertion(
                    true,
                    &ExecuteCondition::Score {
                        target: s.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((..=-1).into()),
                    },
                ));
            }

            let dest = ScoreHolder::from_local_name(
                dest.clone(),
                type_layout(&operand0.get_type()).size(),
            );

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
                        kind: ScoreOpKind::DivAssign,
                        source: Target::Uuid(source1),
                        source_obj: OBJECTIVE.to_string(),
                    }
                    .into(),
                );
            }
            cmds
        }
        Instruction::URem(URem {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            let (mut cmds, source0) = eval_operand(operand0, globals);
            let (tmp, source1) = eval_operand(operand1, globals);
            cmds.extend(tmp.into_iter());

            // FIXME: THIS DOES AN SREM
            for s in source0.iter().cloned() {
                cmds.push(mark_assertion(
                    true,
                    &ExecuteCondition::Score {
                        target: s.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((..=-1).into()),
                    },
                ));
            }

            for s in source1.iter().cloned() {
                cmds.push(mark_assertion(
                    true,
                    &ExecuteCondition::Score {
                        target: s.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((..=-1).into()),
                    },
                ));
            }

            let dest = ScoreHolder::from_local_name(
                dest.clone(),
                type_layout(&operand0.get_type()).size(),
            );

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
                        kind: ScoreOpKind::ModAssign,
                        source: Target::Uuid(source1),
                        source_obj: OBJECTIVE.to_string(),
                    }
                    .into(),
                );
            }
            cmds
        }
        Instruction::ICmp(ICmp {
            predicate: pred @ IntPredicate::EQ,
            operand0,
            operand1,
            dest,
            ..
        })
        | Instruction::ICmp(ICmp {
            predicate: pred @ IntPredicate::NE,
            operand0,
            operand1,
            dest,
            ..
        }) if operand0.get_type() == Type::IntegerType { bits: 64 } => {
            let is_eq = pred == &IntPredicate::EQ;

            // TODO: When operand1 is a constant, we can optimize the direct comparison into a `matches`
            let (mut cmds, op0) = eval_operand(operand0, globals);
            let (tmp_cmds, op1) = eval_operand(operand1, globals);
            cmds.extend(tmp_cmds);

            let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(assign_lit(dest.clone(), !is_eq as i32));

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
            exec.with_run(assign_lit(dest, is_eq as i32));

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

            match operand0.get_type() {
                Type::VectorType {
                    element_type,
                    num_elements,
                } if matches!(*element_type, Type::IntegerType { bits: 32 }) => {
                    if num_elements > 4 {
                        todo!()
                    }

                    let dest = ScoreHolder::from_local_name(dest.clone(), num_elements)
                        .into_iter()
                        .next()
                        .unwrap();

                    let dest_byte = get_unique_holder();
                    for (idx, (t, s)) in target.into_iter().zip(source.into_iter()).enumerate() {
                        cmds.extend(compile_normal_icmp(t, s, predicate, dest_byte.clone()));
                        cmds.push(make_op_lit(dest_byte.clone(), "*=", 1 << (8 * idx)));
                        cmds.push(make_op(dest.clone(), "+=", dest_byte.clone()));
                    }

                    cmds
                }
                Type::VectorType {
                    element_type,
                    num_elements: 4,
                } if matches!(*element_type, Type::IntegerType { bits: 8 }) => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                        .into_iter()
                        .next()
                        .unwrap();
                    let target = target.into_iter().next().unwrap();
                    let source = source.into_iter().next().unwrap();

                    let target_byte = get_unique_holder();
                    let source_byte = get_unique_holder();
                    let dest_byte = get_unique_holder();
                    for byte_idx in 0..4 {
                        // FIXME: These should be ASRs, not just a divide!!

                        cmds.push(assign(target_byte.clone(), target.clone()));
                        cmds.push(make_op_lit(
                            target_byte.clone(),
                            "*=",
                            1 << (8 * (3 - byte_idx)),
                        ));
                        cmds.push(make_op_lit(target_byte.clone(), "/=", 1 << (8 * byte_idx)));

                        cmds.push(assign(source_byte.clone(), source.clone()));
                        cmds.push(make_op_lit(
                            source_byte.clone(),
                            "*=",
                            1 << (8 * (3 - byte_idx)),
                        ));
                        cmds.push(make_op_lit(source_byte.clone(), "/=", 1 << (8 * byte_idx)));

                        cmds.extend(compile_normal_icmp(
                            target_byte.clone(),
                            source_byte.clone(),
                            predicate,
                            dest_byte.clone(),
                        ));
                        cmds.push(make_op_lit(dest_byte.clone(), "*=", 1 << (8 * byte_idx)));
                        cmds.push(make_op(dest.clone(), "+=", dest_byte.clone()));
                    }

                    cmds
                }
                ty @ Type::VectorType { .. } => todo!("{:?}", ty),
                _ => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                        .into_iter()
                        .next()
                        .unwrap();

                    if target.len() != 1 || source.len() != 1 {
                        todo!()
                    }

                    let target = target.into_iter().next().unwrap();
                    let source = source.into_iter().next().unwrap();

                    cmds.extend(compile_normal_icmp(target, source, predicate, dest));

                    cmds
                }
            }
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
        Instruction::Trunc(Trunc {
            operand,
            to_type,
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals);

            let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(assign(dest.clone(), op[0].clone()));

            let result_size = match to_type {
                Type::IntegerType { bits: 24 } => 3,
                Type::IntegerType { bits: 16 } => 2,
                Type::IntegerType { bits: 8 } => 1,
                _ => todo!("{:?}", to_type),
            };
            cmds.extend(truncate_to(dest, result_size));

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

                    // FIXME: THIS DOES NOT WORK
                    // Shift over to the relevant byte
                    for _ in 0..offset {
                        cmds.push(make_op_lit(dest.clone(), "/=", 256));
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
                cmds.push(mark_assertion(
                    true,
                    &ExecuteCondition::Score {
                        target: dest[insert_idx].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((0..=255).into()),
                    },
                ));

                if index == 0 {
                    cmds.extend(zero_low_bytes(dest[insert_idx].clone(), 1));
                    cmds.push(make_op(dest[insert_idx].clone(), "+=", elem));
                } else if index + 1 == element_types.len() as u32 {
                    let trunc_len = offset % 4;
                    cmds.extend(truncate_to(dest[insert_idx].clone(), trunc_len as u32));
                    cmds.extend(shift_left_bytes(elem.clone(), trunc_len as u32));
                    cmds.push(make_op(dest[insert_idx].clone(), "+=", elem));
                } else {
                    todo!()
                }
            } else {
                todo!();
            }

            cmds
        }
        Instruction::SExt(SExt {
            operand,
            to_type,
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals);

            if matches!(to_type, Type::IntegerType { bits: 32 }) {
                let op = op.into_iter().next().unwrap();

                let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                    .into_iter()
                    .next()
                    .unwrap();

                if matches!(operand.get_type(), Type::IntegerType { bits: 1 }) {
                    let cond = ExecuteCondition::Score {
                        target: op.into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((1..=1).into()),
                    };

                    let mut on_one = Execute::new();
                    on_one.with_if(cond.clone());
                    on_one.with_run(assign_lit(dest.clone(), 0xFFFF_FFFF_u32 as i32));
                    cmds.push(on_one.into());

                    let mut on_zero = Execute::new();
                    on_zero.with_unless(cond);
                    on_zero.with_run(assign_lit(dest, 0));
                    cmds.push(on_zero.into());

                    cmds
                } else {
                    let (range, to_add) =
                        if matches!(operand.get_type(), Type::IntegerType { bits: 8 }) {
                            (128..=255, -256)
                        } else if matches!(operand.get_type(), Type::IntegerType { bits: 16 }) {
                            (32768..=65535, -65536)
                        } else {
                            todo!("{:?}", operand.get_type());
                        };

                    cmds.push(assign(dest.clone(), op));
                    let mut exec = Execute::new();
                    exec.with_if(ExecuteCondition::Score {
                        target: dest.clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches(range.into()),
                    });
                    exec.with_run(ScoreAdd {
                        target: dest.into(),
                        target_obj: OBJECTIVE.into(),
                        score: to_add,
                    });
                    cmds.push(exec.into());

                    cmds
                }
            } else if matches!(to_type, Type::IntegerType { bits: 64 }) {
                let dest = ScoreHolder::from_local_name(dest.clone(), 8);

                if matches!(operand.get_type(), Type::IntegerType { bits: 32 }) {
                    let op = op.into_iter().next().unwrap();
                    cmds.push(assign(dest[0].clone(), op));
                    cmds.push(assign_lit(dest[1].clone(), 0));

                    let mut exec = Execute::new();
                    exec.with_if(ExecuteCondition::Score {
                        target: dest[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((..=-1).into()),
                    });
                    exec.with_run(assign_lit(dest[1].clone(), u32::MAX as i32));
                    cmds.push(exec.into());

                    cmds
                } else {
                    todo!("{:?}", operand)
                }
            } else {
                todo!("sign extend to {:?}", to_type)
            }
        }
        Instruction::ZExt(ZExt {
            operand,
            to_type,
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals);

            let to_size = type_layout(to_type).size();

            let dst = ScoreHolder::from_local_name(dest.clone(), to_size);

            if op.len() == 1 {
                cmds.push(assign(dst[0].clone(), op[0].clone()));
                for dst in dst[1..].iter().cloned() {
                    cmds.push(assign_lit(dst, 0));
                }
            } else if op.len() == 2 {
                cmds.push(assign(dst[0].clone(), op[0].clone()));
                cmds.push(assign(dst[1].clone(), op[1].clone()));
                for dst in dst[2..].iter().cloned() {
                    cmds.push(assign_lit(dst, 0));
                }
            } else {
                todo!("{:?} -> {:?}", operand, to_type)
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

            let (tmp, op1) = eval_operand(operand1, globals);

            cmds.extend(tmp);

            match operand0.get_type() {
                ty if type_layout(&ty).size() <= 4 => {
                    let op0 = op0.into_iter().next().unwrap();
                    let op1 = op1.into_iter().next().unwrap();

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
                    let op0 = op0.into_iter().next().unwrap();
                    let op1 = op1.into_iter().next().unwrap();

                    let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                        .into_iter()
                        .next()
                        .unwrap();

                    cmds.push(assign_lit(dest.clone(), 1));

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
                    exec.with_run(assign_lit(dest, 0));
                    cmds.push(exec.into());

                    cmds
                }
                Type::IntegerType { bits: 64 } | Type::IntegerType { bits: 48 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 6);

                    for (dest_word, (op0, op1)) in
                        dest.into_iter().zip(op0.into_iter().zip(op1.into_iter()))
                    {
                        cmds.push(assign(param(0, 0), op0));
                        cmds.push(assign(param(1, 0), op1));

                        cmds.push(
                            McFuncCall {
                                id: McFuncId::new("intrinsic:or"),
                            }
                            .into(),
                        );

                        cmds.push(assign(dest_word, return_holder(0)));
                    }

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

            let layout = type_layout(&operand0.get_type());

            let dest = ScoreHolder::from_local_name(dest.clone(), layout.size());

            match operand0.get_type() {
                _ if layout.size() % 4 == 0 => {
                    for (dest, (op0, op1)) in
                        dest.into_iter().zip(op0.into_iter().zip(op1.into_iter()))
                    {
                        cmds.push(assign(param(0, 0), op0));
                        cmds.push(assign(param(1, 0), op1));
                        cmds.push(
                            McFuncCall {
                                id: McFuncId::new("intrinsic:and"),
                            }
                            .into(),
                        );
                        cmds.push(assign(dest, return_holder(0)));
                    }

                    cmds
                }
                // TODO: This is exactly the same as the above
                Type::IntegerType { bits: 8 } | Type::IntegerType { bits: 16 } => {
                    cmds.push(assign(param(0, 0), op0[0].clone()));
                    cmds.push(assign(param(1, 0), op1[0].clone()));

                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:and"),
                        }
                        .into(),
                    );

                    let dest = dest.into_iter().next().unwrap();

                    cmds.push(assign(dest, return_holder(0)));

                    cmds
                }
                Type::IntegerType { bits: 1 } => {
                    let dest = dest.into_iter().next().unwrap();

                    cmds.push(assign_lit(dest.clone(), 0));

                    let mut exec = Execute::new();
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
                    exec.with_run(assign_lit(dest, 1));

                    cmds.push(exec.into());

                    cmds
                }
                _ => todo!("{:?}", operand0),
            }
        }
        Instruction::Xor(xor) => compile_xor(xor, globals),
        Instruction::Shl(shl) => compile_shl(shl, globals),
        Instruction::LShr(lshr) => compile_lshr(lshr, globals),
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

            let op0_len = if let Type::VectorType { num_elements, .. } = mask.get_type() {
                num_elements
            } else {
                unreachable!()
            };

            let mask_vals = match mask {
                Constant::AggregateZero(Type::VectorType {
                    element_type: _,
                    num_elements,
                }) => vec![Constant::Int { bits: 32, value: 0 }; *num_elements],
                Constant::Vector(v) => v.clone(),
                _ => unreachable!("mask: {:?}", mask),
            };

            let dest_type = Type::VectorType {
                element_type: element_type.clone(),
                num_elements: mask_vals.len(),
            };

            let dest = ScoreHolder::from_local_name(dest.clone(), type_layout(&dest_type).size());

            match &*element_type {
                Type::IntegerType { bits: 32 } => {
                    for (dest, mask_idx) in dest.into_iter().zip(mask_vals.into_iter()) {
                        match mask_idx {
                            Constant::Undef(_) => {
                                // Should we mark it as `undef` for the interpreter?
                            }
                            Constant::Int { bits: 32, value } => {
                                let value = value as usize;

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
                }
                // This does deal with signs, do not use with 8 bits!
                Type::IntegerType { bits: 1 } if mask_vals.len() == 4 => {
                    if mask_vals.len() != 4 {
                        todo!()
                    }

                    let dest = dest.into_iter().next().unwrap();

                    assign_lit(dest.clone(), 0);

                    let dest_byte = get_unique_holder();
                    for (dest_byte_idx, mask_idx) in mask_vals.into_iter().enumerate() {
                        match mask_idx {
                            Constant::Undef(_) => {}
                            Constant::Int { bits: 32, value } => {
                                let value = value as usize;

                                if op0_len != 4 {
                                    todo!()
                                }

                                let (source, byte_idx) = if value > op0_len {
                                    let value = value - op0_len;
                                    (&op1[value / 4], value % 4)
                                } else {
                                    (&op0[value / 4], value % 4)
                                };

                                cmds.push(assign(dest_byte.clone(), source.clone()));
                                cmds.push(make_op_lit(dest_byte.clone(), "/=", byte_idx as i32));
                                cmds.push(make_op_lit(dest_byte.clone(), "%=", 256));
                                cmds.push(make_op_lit(
                                    dest_byte.clone(),
                                    "*=",
                                    1 << (8 * dest_byte_idx),
                                ));
                                cmds.push(make_op(dest.clone(), "+=", dest_byte.clone()));
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                _ => todo!("{:?}", element_type),
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

            let dest =
                ScoreHolder::from_local_name(dest.clone(), type_layout(&element_type).size());

            let (mut cmds, vec) = eval_operand(vector, globals);

            match eval_maybe_const(index, globals) {
                MaybeConst::Const(c) => {
                    let c = c as usize;
                    match &*element_type {
                        Type::IntegerType { bits: 32 } => {
                            let dest = dest.into_iter().next().unwrap();

                            cmds.push(assign(dest, vec[c].clone()));
                        }
                        Type::IntegerType { bits: 1 } => {
                            let dest = dest.into_iter().next().unwrap();

                            cmds.push(assign(dest.clone(), vec[c / 4].clone()));
                            cmds.push(make_op_lit(dest, "/=", 1 << (8 * (c % 4))));
                        }
                        ty => todo!("{:?}", ty),
                    }
                }
                MaybeConst::NonConst(_, _) => todo!("{:?}", index),
            }

            cmds
        }
        Instruction::InsertElement(InsertElement {
            vector,
            element,
            index,
            dest,
            debugloc: _,
        }) => {
            let element_type = if let Type::VectorType { element_type, .. } = vector.get_type() {
                element_type
            } else {
                unreachable!()
            };

            if !matches!(&*element_type, Type::IntegerType { bits: 32 }) {
                todo!("{:?}", vector)
            }

            let dest =
                ScoreHolder::from_local_name(dest.clone(), type_layout(&vector.get_type()).size());

            let (mut cmds, vec) = eval_operand(vector, globals);

            let (tmp, elem) = eval_operand(element, globals);
            cmds.extend(tmp);

            assert_eq!(elem.len(), 1);
            let elem = elem.into_iter().next().unwrap();

            match eval_maybe_const(index, globals) {
                MaybeConst::Const(c) => {
                    for (word_idx, (src_word, dest_word)) in
                        vec.into_iter().zip(dest.into_iter()).enumerate()
                    {
                        if word_idx == c as usize {
                            cmds.push(assign(dest_word, elem.clone()))
                        } else {
                            cmds.push(assign(dest_word, src_word));
                        }
                    }
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
                (vec![assign_lit(target.clone(), score)], vec![target])
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
            let addr = globals
                .get(name)
                .unwrap_or_else(|| panic!("failed to get {:?}", name))
                .0;
            MaybeConst::Const(addr as i32)
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
        Constant::Int { bits: 16, value } => MaybeConst::Const(*value as i16 as i32),
        // FIXME: This should be sign extended???
        Constant::Int { bits: 24, value } => MaybeConst::Const(*value as i32),
        Constant::Int { bits: 32, value } => MaybeConst::Const(*value as i32),
        Constant::Int { bits: 48, value } | Constant::Int { bits: 64, value } => {
            // TODO: I mean it's *const* but not convenient...
            // See also: <i32 x 2>
            let num = get_unique_num();

            let lo_word = ScoreHolder::new(format!("%temp{}%0", num)).unwrap();
            let hi_word = ScoreHolder::new(format!("%temp{}%1", num)).unwrap();

            let cmds = vec![
                assign_lit(lo_word.clone(), *value as i32),
                assign_lit(hi_word.clone(), (*value >> 32) as i32),
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
                    (assign_lit(holder.clone(), 0), holder)
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
                        (assign_lit(holder.clone(), 0), holder)
                    })
                    .unzip();

                MaybeConst::NonConst(cmds, holders)
            } else {
                todo!("{:?} {}", element_type, num_elements)
            }
        }
        Constant::Vector(elems) => {
            // TODO: This is ugly, please fix all of this

            let as_8 = elems
                .iter()
                .map(|e| {
                    if let Constant::Int { bits: 8, value } = e {
                        Some(*value as u8)
                    } else {
                        None
                    }
                })
                .collect::<Option<Vec<u8>>>();

            let as_32 = elems
                .iter()
                .map(|e| {
                    if let Constant::Int { bits: 32, value } = e {
                        Some(*value as i32)
                    } else {
                        None
                    }
                })
                .collect::<Option<Vec<i32>>>();

            let as_64 = elems
                .iter()
                .map(|e| {
                    if let Constant::Int { bits: 64, value } = e {
                        Some(*value)
                    } else {
                        None
                    }
                })
                .collect::<Option<Vec<u64>>>();

            let as_64 = as_64.map(|vec| {
                vec.into_iter()
                    .flat_map(|v| {
                        std::iter::once(v as i32).chain(std::iter::once((v >> 32) as i32))
                    })
                    .collect::<Vec<i32>>()
            });

            if let Some(as_8) = as_8 {
                if let [val0, val1, val2, val3] = as_8[..] {
                    MaybeConst::Const(i32::from_le_bytes([
                        val0 as u8, val1 as u8, val2 as u8, val3 as u8,
                    ]))
                } else {
                    todo!()
                }
            } else if let Some(as_32) = as_32 {
                let num = get_unique_num();

                let (cmds, holders) = as_32
                    .into_iter()
                    .enumerate()
                    .map(|(word_idx, word)| {
                        let holder =
                            ScoreHolder::new(format!("%temp{}%{}", num, word_idx)).unwrap();
                        let cmd = assign_lit(holder.clone(), word);
                        (cmd, holder)
                    })
                    .unzip();

                MaybeConst::NonConst(cmds, holders)
            } else if let Some(as_64) = as_64 {
                let num = get_unique_num();

                let (cmds, holders) = as_64
                    .into_iter()
                    .enumerate()
                    .map(|(word_idx, word)| {
                        let holder =
                            ScoreHolder::new(format!("%temp{}%{}", num, word_idx)).unwrap();
                        let cmd = assign_lit(holder.clone(), word);
                        (cmd, holder)
                    })
                    .unzip();

                MaybeConst::NonConst(cmds, holders)
            } else {
                todo!("{:?}", elems);
            }
        }
        Constant::ICmp(icmp) => {
            let ICmpConst {
                predicate,
                operand0,
                operand1,
            } = &**icmp;
            if let MaybeConst::Const(op0) = eval_constant(operand0, globals) {
                if let MaybeConst::Const(op1) = eval_constant(operand1, globals) {
                    let result = match predicate {
                        IntPredicate::NE => op0 != op1,
                        _ => todo!("{:?}", predicate),
                    };
                    MaybeConst::Const(result as i32)
                } else {
                    todo!("{:?}", operand1)
                }
            } else {
                todo!("{:?}", operand0)
            }
        }
        Constant::Select(s) => {
            let SelectConst {
                condition,
                true_value,
                false_value,
            } = &**s;
            let condition = match condition {
                Constant::ICmp(i) => {
                    let ICmpConst {
                        predicate,
                        operand0,
                        operand1,
                    } = &**i;
                    if let Constant::BitCast(bc) = operand0 {
                        if let BitCastConst {
                            operand: Constant::GlobalReference { name, .. },
                            ..
                        } = &**bc
                        {
                            let value = globals.get(name).unwrap().0;
                            if let Constant::Null(_) = operand1 {
                                #[allow(clippy::absurd_extreme_comparisons)]
                                match predicate {
                                    IntPredicate::ULE => (value as u32) <= 0,
                                    IntPredicate::NE => (value as u32) != 0,
                                    _ => todo!("{:?}", predicate),
                                }
                            } else {
                                todo!("{:?}", operand1)
                            }
                        } else {
                            todo!("{:?}", bc)
                        }
                    } else {
                        todo!("{:?}", operand0)
                    }
                }
                _ => todo!("{:?}", condition),
            };
            if condition {
                eval_constant(true_value, globals)
            } else {
                eval_constant(false_value, globals)
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

    #[test]
    #[ignore]
    fn test_u64_shift_const() {
        todo!()
    }

    #[test]
    #[ignore]
    fn test_u64_add() {
        todo!()
    }

    #[test]
    #[ignore]
    fn test_u64_mul() {
        todo!()
    }

    #[test]
    #[ignore]
    fn test_zero_low_bytes() {
        todo!()
    }

    #[test]
    #[ignore]
    fn unaligned_u64_store_const() {
        todo!()
    }

    #[test]
    #[ignore]
    fn unaligned_u64_store() {
        todo!()
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
