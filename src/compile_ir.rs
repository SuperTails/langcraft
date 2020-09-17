use crate::analysis::{AbstractBlock, BlockEdge, BlockEnd};
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
use llvm_ir::constant::ConstantRef;
use llvm_ir::instruction::{
    Add, Alloca, And, AShr, BitCast, Call, ExtractElement, ExtractValue, GetElementPtr, ICmp,
    InsertElement, InsertValue, IntToPtr, LShr, Load, Mul, Or, Phi, PtrToInt, SDiv, SExt, SRem,
    Select, Shl, ShuffleVector, Store, Sub, Trunc, UDiv, URem, Xor, ZExt,
};
use llvm_ir::function::ParameterAttribute;
use llvm_ir::module::GlobalVariable;
use llvm_ir::terminator::{Br, CondBr, Ret, Switch, Unreachable};
use llvm_ir::types::{Typed, TypeRef, Types, NamedStructDef};
use llvm_ir::{
    Constant, Function, Instruction, IntPredicate, Module, Name, Operand, Terminator, Type,
};
use std::alloc::Layout;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::{TryFrom, TryInto};
use std::sync::Mutex;

// FIXME: Alignment for Alloca, functions, and global variables

// FIXME:
// Doing it twice just so that it's ***really*** visible FIXME:
// VALUES THAT ARE NOT A MULTIPLE OF WORD SIZE STORED IN REGISTERS
// ARE NOT GUARANTEED TO HAVE THEIR HIGH BITS ZEROED OR SIGN-EXTENDED
// SO YOU CANNOT JUST ADD A "u8" INTO A "u32" AND EXPECT IT TO WORK

// FIXME: This all should set `gamerule maxCommandChainLength` to an appropriate value


fn as_struct_ty(n: &NamedStructDef) -> Option<(&[TypeRef], bool)> {
    named_as_type(n).and_then(|d| {
        if let Type::StructType { element_types, is_packed } = &**d {
            Some((&element_types[..], *is_packed))
        } else {
            unreachable!()
        }
    })
}

fn named_as_type(n: &NamedStructDef) -> Option<&TypeRef> {
    match n {
        NamedStructDef::Defined(d) => Some(d),
        NamedStructDef::Opaque => None,
    }
}

fn as_const_64(operand: &Operand) -> Option<u64> {
    as_const_int(64, operand)
}

fn as_const_32(operand: &Operand) -> Option<u32> {
    as_const_int(32, operand).map(|n| n as u32)
}

fn as_const_int(bits: u32, operand: &Operand) -> Option<u64> {
    if let Operand::ConstantOperand(c) = operand {
        if let Constant::Int { bits: b, value } = &**c {
            if *b == bits {
                Some(*value)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    }
}

pub const OBJECTIVE: &str = "rust";

pub const ROW_SIZE: usize = 32;

pub fn pos_to_func_idx(x: i32, z: i32) -> usize {
    usize::try_from(ROW_SIZE as i32 * (-2 - x) + z).unwrap()
}

pub fn func_idx_to_pos(f: usize) -> (i32, i32) {
    (-2 - (f / ROW_SIZE) as i32, (f % ROW_SIZE) as i32)
}

pub fn temp_fn_ptr() -> ScoreHolder {
    ScoreHolder::new("%%tempfuncptr".to_string()).unwrap()
}

pub fn cmd_count() -> ScoreHolder {
    ScoreHolder::new("%%cmdcount".to_string()).unwrap()
}

pub fn cmd_limit() -> ScoreHolder {
    ScoreHolder::new("%%CMD_LIMIT".to_string()).unwrap()
}

pub const COND_STACK_BYTES: usize = 500;

pub fn condtempholder() -> ScoreHolder {
    ScoreHolder::new("%%condtempholder".to_string()).unwrap()
}

pub fn condstackptr() -> ScoreHolder {
    ScoreHolder::new("%condstackptr".to_string()).unwrap()
}

pub fn push_cond_stack(target: ScoreHolder) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(assign(ptr(), condstackptr()));
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(write_ptr(target));
    cmds.push(
        ScoreAdd {
            target: condstackptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: 4,
        }
        .into(),
    );

    cmds
}

pub fn pop_cond_stack(target: ScoreHolder) -> Vec<Command> {
    let mut cmds = Vec::new();

    cmds.push(
        ScoreAdd {
            target: condstackptr().into(),
            target_obj: OBJECTIVE.to_string(),
            score: -4,
        }
        .into(),
    );
    cmds.push(assign(ptr(), condstackptr()));
    cmds.push(
        McFuncCall {
            id: McFuncId::new("intrinsic:setptr"),
        }
        .into(),
    );
    cmds.push(read_ptr(target));

    cmds
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

pub fn mark_assertion_matches<R: Into<cir::McRange>>(inverted: bool, score_holder: ScoreHolder, range: R) -> Command {
    mark_assertion(inverted, &ExecuteCondition::Score {
        target: score_holder.into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches(range.into()),
    })
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

pub fn invert(target: ScoreHolder) -> Vec<Command> {
    vec![
        make_op_lit(target.clone(), "+=", 1),
        make_op_lit(target, "*=", -1),
    ]
}

pub fn get_index(x: i32, y: i32, z: i32) -> Result<i32, InterpError> {
    if 0 <= x && x < 128 && 0 <= y && y < 16 && 0 <= z && z < 16 {
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
    assert!(address < 128 * 16 * 16);
    let z = address % 16;
    address /= 16;
    let y = address % 16;
    address /= 16;
    let x = address % 128;
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
    /// Insert a print command at the beginning of each LLVM basic block 
    pub trace_bbs: bool,
}


pub fn save_regs<T>(regs: T) -> Vec<Command>
where
    T: IntoIterator<Item = ScoreHolder>
{
    let base_set = assign(stackbaseptr(), stackptr());

    regs
        .into_iter()
        .chain(std::iter::once(stackbaseptr()))
        .filter(|reg| {
            reg != &stackptr() &&
            reg != &ptr() &&
            reg != &condstackptr() &&
            reg != &condtempholder() &&
            //reg != &stackbaseptr() &&
            reg != &ScoreHolder::new("%phi".into()).unwrap() &&
            !reg.as_ref().contains("%%fixup") &&
            !reg.as_ref().starts_with("%return%")
        })
        .map(push)
        .flatten()
        .chain(std::iter::once(base_set))
        .collect()
}

pub fn load_regs<T, U>(regs: T) -> Vec<Command>
where
    T: IntoIterator<Item=ScoreHolder, IntoIter=U>,
    U: DoubleEndedIterator + Iterator<Item=ScoreHolder>,
{
    let base_read = assign(stackptr(), stackbaseptr());

    std::iter::once(base_read).chain(
        regs
            .into_iter()
            .filter(|reg| {
                reg != &stackptr() &&
                reg != &ptr() &&
                reg != &condstackptr() &&
                reg != &condtempholder() &&
                //reg != &stackbaseptr() &&
                reg != &ScoreHolder::new("%phi".into()).unwrap() &&
                !reg.as_ref().contains("%%fixup") &&
                !reg.as_ref().starts_with("%return%")
            })
            .chain(std::iter::once(stackbaseptr()))
            .rev()
            .map(pop)
            .flatten()
    ).collect()
}

type AbstractCompileOutput = (Vec<AbstractBlock>, HashMap<String, BTreeSet<ScoreHolder>>, HashMap<String, McFuncId>);

fn compile_module_abstract(module: &Module, options: &BuildOptions, globals: &GlobalVarList) -> AbstractCompileOutput {
    let mut clobber_list = HashMap::<String, BTreeSet<ScoreHolder>>::new();

    let mut funcs = Vec::new();

    let mut after_blocks = Vec::new();

    let mut func_starts = HashMap::<String, McFuncId>::new();

    for (parent, (mc_funcs, clobbers)) in module
        .functions
        .iter()
        .map(|f| (f, compile_function(f, &globals, &module.types, options)))
    {
        for AbstractBlock { body: McFunction { id, .. }, .. } in mc_funcs.iter() {
            clobber_list.insert(
                id.name.clone(),
                clobbers.clone().into_iter().map(|c| c.0).collect(),
            );
        }

        func_starts.insert(parent.name.clone(), mc_funcs[0].body.id.clone());

        let mut f = mc_funcs.into_iter();
        funcs.push(f.next().unwrap());
        after_blocks.extend(f);
    }

    funcs.extend(after_blocks);

    /*println!("funcs:");
    for func in funcs.iter() {
        println!("{}", func.0.body.id);
        match &func.0.term {
            BlockEnd::Inlined(_) => {
                unreachable!()
            }
            BlockEnd::DynCall => {
                println!("\tdynamic");
            }
            BlockEnd::StaticCall(c) => {
                println!("\tstatic {}", c)
            }
            BlockEnd::Normal(n) => {
                println!("\tnormal: {:?}", n)
            }
        }
    }*/

    for intr in crate::intrinsics::INTRINSICS.iter() {
        assert_eq!(func_starts.insert(intr.id.to_string(), intr.id.clone()), None);
    }

    (funcs, clobber_list, func_starts)
}

pub fn create_return_func() -> McFunction {
    let mut cmds = Vec::new();

    cmds.extend(pop(temp_fn_ptr()));

    cmds.push(McFuncCall { id: McFuncId::new("rust:__langcraft_call") }.into());

    McFunction {
        id: McFuncId::new("rust:__langcraft_return"),
        cmds,
    }
}

pub fn create_call_func(others: &[McFunction]) -> McFunction {
    let mut cmds = Vec::new();

    // TODO: Make this a build option
    /*let mut t = cir::TextBuilder::new();
    t.append_text("func ptr: ".to_string());
    t.append_score(temp_fn_ptr(), OBJECTIVE.into(), None);

    cmds.push(
        Tellraw {
            target: cir::Selector {
                var: cir::SelectorVariable::AllPlayers,
                args: Vec::new(),
            }
            .into(),
            message: t.build(),
        }
        .into()
    );*/

    let mut cmd = Execute::new();       
    cmd.with_if(ExecuteCondition::Score {
        target: temp_fn_ptr().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((-1..=-1).into()),
    });
    cmd.with_run(SetBlock {
        pos: "~ ~ ~".into(),
        block: "minecraft:air".into(),
        kind: cir::SetBlockKind::Replace,
    });
    cmds.push(cmd.into());

    // This could be improved with a binary search, but eh, whatever
    for (idx, dest) in others.iter().enumerate() {
        let mut cmd = Execute::new();       
        cmd.with_if(ExecuteCondition::Score {
            target: temp_fn_ptr().into(),
            target_obj: OBJECTIVE.into(),
            kind: ExecuteCondKind::Matches((idx as i32..=idx as i32).into()),
        });
        cmd.with_run(Data {
            target: DataTarget::Block("~ ~ ~".to_string()),
            kind: cir::DataKind::Modify {
                path: "Command".to_string(),
                kind: cir::DataModifyKind::Set,
                source: cir::DataModifySource::ValueString(McFuncCall { id: dest.id.clone() }.to_string()),
            },
        });
        cmds.push(cmd.into());
    }

    let mut on_invalid_1 = Execute::new();
    on_invalid_1.with_unless(ExecuteCondition::Score {
        target: temp_fn_ptr().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((-1..=others.len() as i32 - 1).into()),
    });
    on_invalid_1.with_run(SetBlock {
        pos: "~ ~ ~".into(),
        block: "minecraft:air".into(),
        kind: cir::SetBlockKind::Replace,
    });
    cmds.push(on_invalid_1.into());

    let mut on_invalid_2 = Execute::new();
    on_invalid_2.with_unless(ExecuteCondition::Score {
        target: temp_fn_ptr().into(),
        target_obj: OBJECTIVE.into(),
        kind: ExecuteCondKind::Matches((-1..=others.len() as i32 - 1).into()),
    });
    on_invalid_2.with_run(SetBlock {
        pos: "-2 0 0".into(),
        block: "minecraft:air".into(),
        kind: cir::SetBlockKind::Replace,
    });
    cmds.push(on_invalid_2.into());

    McFunction {
        id: McFuncId::new("rust:__langcraft_call"),
        cmds,
    }
}

lazy_static! {
    pub static ref ON_TICK: McFunction = McFunction::from_str(
        McFuncId::new("__langcraft_on_tick"),
        include_str!("__langcraft_on_tick.mcfunction")
    ).unwrap();
}

pub fn compile_module(module: &Module, options: &BuildOptions) -> Vec<McFunction> {
    // Steps in compiling a module:
    // 1. Lay out global variables
    // 2. Convert LLVM functions to abstract blocks
    // 3. Extend abstract blocks with "chains"
    // 4. Reify call graph to MC functions
    // 5. Do relocations
    // 6. Add global variable init commands
    
    // Step 1: Lay out global variables
    let mut alloc = StaticAllocator(4);
    let mut globals = global_var_layout(&module.global_vars, &module.functions, &mut alloc, &module.types);

    // Step 2: Convert LLVM functions to abstract blocks
    let (funcs, clobber_list, func_starts) = compile_module_abstract(module, options, &globals);

    for func in funcs.iter() {
        if let Some(dest) = func.get_dest(&func_starts) {
            println!("{} -> {}", func.body.id, dest);
        }
    }

    // Step 4: Reify call graph to MC functions
    let mut funcs = funcs
        .into_iter()
        .map(|block| {
            reify_block(block, &clobber_list, &func_starts, &globals, &module.types)
        })
        .collect::<Vec<_>>();

    funcs.extend(crate::intrinsics::INTRINSICS.clone());

    // Step 5: Do relocations
    let mut funcs = do_relocation(funcs, &func_starts, &mut globals);

    println!("\nIndices:");
    for (idx, f) in funcs.iter().enumerate() {
        println!("{:>2}: {}", idx, f.id);
    }

    // Step 6: Add global variable init commands
    let mut init_cmds = compile_global_var_init(&module.global_vars, &mut globals, &module.types);
    init_cmds.push(assign_lit(condstackptr(), alloc.reserve(COND_STACK_BYTES as u32) as i32));
    let main_return = alloc.reserve(4);
    init_cmds.push(set_memory(-1, main_return as i32));
    init_cmds.push(assign_lit(stackptr(), alloc.reserve(4) as i32));
    init_cmds.push(assign_lit(stackbaseptr(), 0));
    init_cmds.extend(make_build_cmds(func_starts.get("main").unwrap()));

    let mut all_clobbers = BTreeSet::new();
    for c in clobber_list.values() {
        all_clobbers.extend(c);
    }

    #[allow(clippy::reversed_empty_ranges)]
    init_cmds.splice(
        0..0,
        all_clobbers.iter().map(|c| assign_lit((*c).clone(), 1)),
    );

    funcs.push(McFunction {
        id: McFuncId::new("init"),
        cmds: init_cmds,
    });

    funcs.push(ON_TICK.clone());

    //if let Some(main_id) = func_starts.get("main") {
        //let main_idx = funcs.iter().position(|f| &f.id == main_id).unwrap();
        //let (main_x, main_z) = func_idx_to_pos(main_idx);
    funcs.push(McFunction {
        id: McFuncId::new("run"),
        cmds: vec![
            McFuncCall {
                id: McFuncId::new("init"),
            }
            .into(),
            SetBlock {
                pos: "-2 1 0".to_string(),
                block: "minecraft:redstone_block".to_string(),
                kind: SetBlockKind::Replace,
            }
            .into(),
        ],
    });

    //} else {
    //    todo!("support programs without an entry point")
    //}

    funcs
}

/// Finalizes the locations of the generated functions
/// and applies any necessary fixups
fn do_relocation<T>(funcs: T, func_starts: &HashMap<String, McFuncId>, globals: &mut GlobalVarList) -> Vec<McFunction>
    where T: IntoIterator<Item=McFunction>
{
    let mut funcs = funcs.into_iter().collect::<Vec<_>>();

    for (func, func_id) in func_starts.iter() {
        let idx = funcs.iter().position(|f| &f.id == func_id).unwrap();

        let name = Name::Name(Box::new(func.clone()));

        #[allow(clippy::map_entry)]
        if globals.contains_key(&name) {
            globals.get_mut(&name).unwrap().0 = idx as u32;
        } else {
            assert!(func.contains("intrinsic"));
            let name = Box::leak(Box::new(name));
            globals.insert(name, (idx as u32, None));
        }
    }

    funcs.push(create_call_func(&funcs));
    funcs.push(create_return_func());

    apply_fixups(&mut funcs, &func_starts);

    funcs
}

fn apply_branch_fixups(funcs: &mut [McFunction]) {
    for func_idx in 0..funcs.len() {
        let mut cmd_idx = 0;
        while cmd_idx < funcs[func_idx].cmds.len() {
            if let Command::FuncCall(McFuncCall { id }) = &mut funcs[func_idx].cmds[cmd_idx] {
                if id.name.ends_with("%%fixup_branch") {
                    // It doesn't matter what we replace it with
                    // because the whole command gets removed
                    let mut id = std::mem::replace(id, McFuncId::new(""));
                    id.name.truncate(id.name.len() - "%%fixup_branch".len());

                    let idx = funcs.iter().position(|f| f.id == id).unwrap();
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

                    cmd_idx += 1;
                }
            } else if let Command::Execute(Execute {
                run: Some(func_call),
                ..
            }) = &mut funcs[func_idx].cmds[cmd_idx]
            {
                if let Command::FuncCall(McFuncCall { id }) = &mut **func_call {
                    if id.name.ends_with("%%fixup_branch") {
                        let mut id = std::mem::replace(id, McFuncId::new(""));
                        id.name.truncate(id.name.len() - "%%fixup_branch".len());

                        let idx = funcs.iter().position(|f| f.id == id).unwrap();
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
                } /*else if target.as_ref().ends_with("%%fixup_func_ref") {
                        // This is a reference to a function
                        let func_name = target.as_ref()[..target.as_ref().len() - "%%fixup_func_ref".len()].to_owned();
                        let func_id = func_starts.get(&func_name).unwrap();
                        //let func_idx = funcs.iter().enumerate().find(|(_, f)| &f.id == func_id).unwrap().0;
                        todo!("{} {}", func_name, func_call)
                }*/
            }

            cmd_idx += 1;
        }
    }
}

fn apply_func_ref_fixups(funcs: &mut [McFunction], func_starts: &HashMap<String, McFuncId>) {
    let get_value = |holder: &ScoreHolder, funcs: &[McFunction]| -> usize {
        assert!(holder.as_ref().ends_with("%%fixup_func_ref"));
        let func_name = &holder.as_ref()[..holder.as_ref().len() - "%%fixup_func_ref".len()];
        let func_id = func_starts.get(func_name).unwrap();
        funcs.iter().position(|f| &f.id == func_id).unwrap()
    };

    for func_idx in 0..funcs.len() {
        let mut cmd_idx = 0;
        while cmd_idx < funcs[func_idx].cmds.len() {
            if funcs[func_idx].cmds[cmd_idx].to_string().contains("%%fixup_func_ref") {
                if funcs[func_idx].cmds[cmd_idx].to_string().starts_with("execute at @e[tag=ptr] store result block ~ ~ ~ RecordItem.tag.Memory int 1 run") {
                    if let Command::Execute(Execute { run: Some(run), .. }) = &funcs[func_idx].cmds[cmd_idx] {
                        if let Command::ScoreGet(ScoreGet { target: Target::Uuid(target), target_obj }) = &**run {
                            assert_eq!(target_obj, OBJECTIVE);

                            let value = get_value(target, funcs);

                            let mut cmd = Execute::new();
                            cmd.with_at(Target::Selector(cir::Selector {
                                var: cir::SelectorVariable::AllEntities,
                                args: vec![cir::SelectorArg("tag=ptr".into())],
                            }));
                            cmd.with_run(Data {
                                target: DataTarget::Block("~ ~ ~".into()),
                                kind: DataKind::Modify {
                                    kind: cir::DataModifyKind::Set,
                                    path: "RecordItem.tag.Memory".into(),
                                    source: cir::DataModifySource::Value(value as i32)
                                }
                            });

                            funcs[func_idx].cmds[cmd_idx] = cmd.into();
                        } else {
                            todo!("{}", run)
                        }
                    } else {
                        unreachable!()
                    }
                } else if let Command::ScoreOp(ScoreOp { target, target_obj, kind, source: Target::Uuid(source), source_obj }) = &funcs[func_idx].cmds[cmd_idx] {
                    if source.as_ref().ends_with("%%fixup_func_ref") {
                        assert_eq!(source_obj, OBJECTIVE);
                        if *kind != ScoreOpKind::Assign {
                            todo!("{:?}", kind)
                        }

                        let target = target.clone();
                        let target_obj = target_obj.clone();

                        let value = get_value(source, funcs);

                        funcs[func_idx].cmds[cmd_idx] = ScoreSet {
                            target,
                            target_obj,
                            score: value as i32,
                        }.into();
                    } else {
                        todo!()
                    }
                } else {
                    todo!("{}", funcs[func_idx].cmds[cmd_idx])
                }
            }

            cmd_idx += 1;
        }
    }
}

fn apply_return_fixups(funcs: &mut [McFunction]) {
    for func_idx in 0..funcs.len() {
        let mut cmd_idx = 0;
        while cmd_idx < funcs[func_idx].cmds.len() {
            if let Command::Execute(Execute {
                run: Some(func_call),
                ..
            }) = &mut funcs[func_idx].cmds[cmd_idx] {
                if let Command::ScoreGet(ScoreGet {
                    target: Target::Uuid(target),
                    ..
                }) = &mut **func_call
                {
                    if target.as_ref().ends_with("%%fixup_return_addr") {
                        let return_id = &target.as_ref()[..target.as_ref().len() - "%%fixup_return_addr".len()];
                        let mut return_id = return_id.parse::<McFuncId>().unwrap();
                        return_id.sub += 1;

                        let idx = funcs
                            .iter()
                            .position(|f| f.id == return_id)
                            .unwrap_or_else(|| panic!("could not find {:?}", return_id));

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

fn apply_call_fixups(funcs: &mut [McFunction], func_starts: &HashMap<String, McFuncId>) {
    for func_idx in 0..funcs.len() {
        let mut cmd_idx = 0;
        while cmd_idx < funcs[func_idx].cmds.len() {
            if let Command::Comment(c) = &mut funcs[func_idx].cmds[cmd_idx] {
                // TODO: `strip_prefix` is nightly but it's exactly what I'm doing
                if c.starts_with("!FIXUPCALL ") {
                    let name = c["!FIXUPCALL ".len()..].to_owned();

                    let call_id = func_starts.get(&name).unwrap_or_else(|| panic!("failed to get {}", name));
                    let idx = funcs.iter().position(|f| &f.id == call_id).unwrap();
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
                        .insert(cmd_idx, Command::Comment(name.to_string()));
                }
            }

            cmd_idx += 1;
        }
    }

}

// This doesn't change what the function clobbers
fn apply_fixups(funcs: &mut [McFunction], func_starts: &HashMap<String, McFuncId>) {
    apply_branch_fixups(funcs);
    apply_return_fixups(funcs);
    apply_func_ref_fixups(funcs, func_starts);
    apply_call_fixups(funcs, func_starts);
    apply_cmd_count_fixups(funcs, func_starts);

    // Make sure we didn't miss anything
    for func in funcs.iter() {
        for cmd in func.cmds.iter() {
            let as_string = cmd.to_string();
            if as_string.contains("%%fixup") || as_string.contains("!FIXUP") {
                todo!("{}", cmd)
            }
        }
    }
}

fn apply_cmd_count_fixups(funcs: &mut [McFunction], func_starts: &HashMap<String, McFuncId>) {
    let list = funcs.iter().map(|f| (f.id.clone(), f)).collect();

    let counts = funcs.iter().map(|f| crate::analysis::estimate_total_count(&list, func_starts, f)).collect::<Vec<_>>();
    for (func_idx, count) in (0..funcs.len()).zip(counts.into_iter()) {
        for cmd_idx in 0..funcs[func_idx].cmds.len() {
            if let Command::Comment(c) = &funcs[func_idx].cmds[cmd_idx] {
                if c == "%%fixup_update_cmds" {
                    funcs[func_idx].cmds[cmd_idx] = make_op_lit(cmd_count(), "+=", count.unwrap() as i32);
                }
            }
        }
    }
}

fn make_build_cmds(main_id: &McFuncId) -> Vec<Command> {
    vec![
        cir::Fill {
            start: "-2 0 0".to_string(),
            end: "-2 1 2".to_string(),
            block: "minecraft:air".to_string(),
        }
        .into(),
        SetBlock {
            pos: "-2 0 0".to_string(),
            block: "minecraft:command_block[facing=south]{Command:\"function rust:__langcraft_on_tick\"}".to_string(),
            kind: cir::SetBlockKind::Replace,
        }
        .into(),
        SetBlock {
            pos: "-2 1 1".to_string(),
            block: format!("minecraft:chain_command_block[conditional=true,facing=south]{{UpdateLastExecution:0b,auto:1b,Command:\"{}\"}}", McFuncCall { id: main_id.clone() }),
            kind: cir::SetBlockKind::Replace,
        }
        .into(),
        SetBlock {
            pos: "-2 0 2".to_string(),
            block: "minecraft:chain_command_block[conditional=true,facing=north]{UpdateLastExecution:0b,auto:1b}".into(),
            kind: cir::SetBlockKind::Replace,
        }
        .into(),
    ]
}

/*fn make_build_cmds(funcs: &[McFunction]) -> Vec<Command> {
    let mut build_cmds = vec![
        cir::Fill {
            start: "-2 0 0".to_string(),
            end: "-15 0 64".to_string(),
            block: "minecraft:air".to_string(),
        }
        .into(),
    ];

    build_cmds.extend(funcs
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
        }));

    build_cmds
}*/



fn getelementptr_const(
    GetElementPtrConst {
        address,
        indices,
        in_bounds,
    }: &GetElementPtrConst,
    globals: &GlobalVarList,
    tys: &Types,
) -> u32 {
    if !in_bounds {
        todo!("not inbounds constant getelementptr")
    }

    println!("Address: {:?}", address);
    println!("Indices: {:?}", indices);

    let result = if let Constant::GlobalReference { name, ty } = &**address {
        let mut offset = globals
            .get(&name)
            .unwrap_or_else(|| panic!("couldn't find global {:?}", name))
            .0;
        let mut ty = ty.clone();

        for index in &indices[1..] {
            let index = if let Constant::Int { bits: 32, value } = &**index {
                *value as i32
            } else {
                unreachable!()
            };

            match (*ty).clone() {
                Type::NamedStructType {
                    name,
                } => {
                    let (element_types, is_packed) = as_struct_ty(tys.named_struct_def(&name).unwrap()).unwrap();
                    ty = element_types[index as usize].clone();
                    offset += offset_of(element_types, is_packed, index as u32, tys) as u32;
                }
                Type::StructType {
                    element_types,
                    is_packed,
                } => {
                    ty = element_types[index as usize].clone();
                    offset += offset_of(&element_types, is_packed, index as u32, tys) as u32;
                }
                Type::ArrayType {
                    element_type,
                    num_elements: _,
                } => {
                    let elem_size = type_layout(&element_type, tys).pad_to_align().size();

                    ty = element_type;
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
    globals: &mut GlobalVarList,
    tys: &Types,
) -> Vec<Command> {
    let mut cmds = Vec::new();

    for var in vars {
        cmds.extend(one_global_var_init(var, &globals, tys));
    }

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

    cmds.push(assign_lit(cmd_count(), 0));

    cmds
}

fn global_var_layout<'a>(v: &'a [GlobalVariable], funcs: &[Function], alloc: &mut StaticAllocator, tys: &Types) -> GlobalVarList<'a> {
    let mut result = HashMap::new();
    for v in v.iter() {
        let pointee_type = if let Type::PointerType { pointee_type, .. } = &v.ty.as_ref() {
            pointee_type
        } else {
            unreachable!()
        };

        let start = alloc.reserve(type_layout(pointee_type, tys).size() as u32);
        result.insert(&v.name, (start, Some((**v.initializer.as_ref().unwrap()).clone())));
    }

    for func in funcs.iter() {
        let name = Box::leak(Box::new(Name::Name(Box::new(func.name.clone()))));
        result.insert(name, (u32::MAX, None));
    }

    result
}

pub fn make_zeroed(ty: &Type, tys: &Types) -> Constant {
    match ty {
        Type::NamedStructType {
            name,
        } => {
            let struct_ty = tys.named_struct_def(name).unwrap();
            make_zeroed(named_as_type(struct_ty).unwrap(), tys)
        }
        Type::StructType {
            element_types,
            is_packed,
        } => {
            let values = element_types.iter().map(|et| ConstantRef::new(make_zeroed(et, tys))).collect();
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
            let elements = std::iter::repeat(ConstantRef::new(make_zeroed(element_type, tys)))
                .take(*num_elements)
                .collect();
            Constant::Array {
                element_type: (*element_type).clone(),
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
    tys: &Types,
) -> BTreeMap<i32, u8> {
    if let Constant::AggregateZero(t) = value {
        value = make_zeroed(&t, tys);
    }
    let value = value;

    match ty {
        Type::IntegerType { bits: 8 } => {
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals, tys) {
                i8::try_from(c).unwrap() as u8
            } else {
                todo!("{:?}", value)
            };

            std::iter::once((start_addr, val)).collect()
        }
        Type::IntegerType { bits: 16 } => {
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals, tys) {
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
            let val = if let MaybeConst::Const(c) = eval_constant(&value, globals, tys) {
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
            let val = match eval_constant(&value, globals, tys) {
                MaybeConst::Const(c) => c,
                _ => todo!("{:?}", value),
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
                    assert_eq!(&**element_type, &**et);
                    elements
                }
                _ => todo!("{:?}", value),
            };

            assert_eq!(*num_elements, vals.len());

            vals.iter()
                .enumerate()
                .flat_map(|(idx, val)| {
                    let offset = offset_of_array(element_type, idx as u32, tys);
                    let field_addr = start_addr + offset as i32;
                    init_data(field_addr, element_type, (**val).clone(), globals, tys)
                })
                .collect()
        }
        Type::NamedStructType {
            name,
        } => {
            let struct_ty = tys.named_struct_def(name).unwrap();

            init_data(start_addr, named_as_type(struct_ty).unwrap(), value, globals, tys)
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
                    let offset = offset_of(element_types, *is_packed, idx as u32, tys);
                    let field_addr = start_addr + offset as i32;
                    init_data(field_addr, field_ty, (**value).clone(), globals, tys)
                })
                .collect()
        }
        _ => todo!("data type {:?}", ty),
    }
}

fn one_global_var_init(v: &GlobalVariable, globals: &GlobalVarList, tys: &Types) -> Vec<Command> {
    if matches!(v.name, Name::Number(_)) {
        todo!()
    }

    let start = globals.get(&v.name).unwrap().0;

    println!("evaluating {}", v.name);

    let temp = v.name.to_string();
    let target = ScoreHolder::new_lossy(format!("%@{}", &temp[1..temp.len() - 1]));

    match &*v.ty {
        // I'm pretty sure it's *always* a pointer...
        Type::PointerType { pointee_type, .. } => {
            let bytes = init_data(
                start as i32,
                pointee_type,
                (*v.initializer.clone().unwrap()).clone(),
                globals,
                tys,
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

            cmds
        }
        _ => todo!("{:?}", v.ty),
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
    arguments: &[(Operand, Vec<ParameterAttribute>)],
    globals: &GlobalVarList,
    tys: &Types,
) -> Vec<Command> {
    if let [(dest, _), (value, _), (len, _), (volatile, _)] = &arguments[..] {
        let volatile = as_const_int(1, volatile).unwrap() != 0;

        if volatile {
            todo!()
        }

        let (mut cmds, dest1) = eval_operand(dest, globals, tys);
        let (tmp, value1) = eval_operand(value, globals, tys);
        cmds.extend(tmp);

        let len1 = if let Some(value) = as_const_64(len) {
            let len1 = get_unique_holder();
            cmds.push(assign_lit(len1.clone(), value as i32));
            vec![len1]
        } else {
            let (tmp, len1) = eval_operand(len, globals, tys);
            cmds.extend(tmp);
            len1
        };

        assert_eq!(dest1.len(), 1, "multiword pointer {:?}", dest);
        assert_eq!(value1.len(), 1, "multiword value {:?}", value);
        assert_eq!(len1.len(), 1, "multiword length {:?}", len);

        cmds.push(assign(param(0, 0), dest1[0].clone()));
        cmds.push(assign(param(1, 0), value1[0].clone()));
        cmds.push(assign(param(2, 0), len1[0].clone()));

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
    arguments: &[(Operand, Vec<ParameterAttribute>)],
    globals: &GlobalVarList,
    tys: &Types,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let get_align = |attrs: &[ParameterAttribute]| -> Option<u64> {
        attrs
            .iter()
            .filter_map(|attr| {
                if let ParameterAttribute::Alignment(value) = attr {
                    Some(*value)
                } else {
                    None
                }
            })
            .next()
    };

    if let [(dest, dest_attr), (src, src_attr), (len, _), (volatile, _)] = &arguments[..] {
        let volatile = as_const_int(1, volatile).unwrap() != 0;

        if volatile {
            todo!()
        }

        let (mut cmds, src1) = eval_operand(src, globals, tys);
        let (tmp, dest1) = eval_operand(dest, globals, tys);
        cmds.extend(tmp);

        assert_eq!(src1.len(), 1, "multiword pointer {:?}", src);
        assert_eq!(dest1.len(), 1, "multiword pointer {:?}", dest);

        let src1 = src1.into_iter().next().unwrap();
        let dest1 = dest1.into_iter().next().unwrap();

        let dest_align = get_align(dest_attr);
        let src_align = get_align(src_attr);

        if let Some(value) = as_const_32(len).filter(|v| *v > 1024) {
            cmds.extend(push(ScoreHolder::new("%%fixup_return_addr".to_string()).unwrap()));
            cmds.push(assign(param(0, 0), dest1));
            cmds.push(assign(param(1, 0), src1));
            cmds.push(assign_lit(param(2, 0), value as i32));
            cmds.push(assign_lit(param(4, 0), 1));
            cmds.push(Command::Comment("!FIXUPCALL intrinsic:memcpy".into()));

            return (cmds, Some(Vec::new()));
        } else if let Some(((d, s), len)) = dest_align.zip(src_align).zip(as_const_32(len)).filter(|((d, s), _)| d % 4 == 0 && s % 4 == 0) {
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
        } else {
            let (tmp, len1) = eval_operand(len, globals, tys);
            cmds.extend(tmp);

            assert_eq!(len1.len(), 1, "multiword length {:?}", len);
            let len1 = len1.into_iter().next().unwrap();

            cmds.push(assign(param(0, 0), dest1));
            cmds.push(assign(param(1, 0), src1));
            cmds.push(assign(param(2, 0), len1));
            cmds.push(assign_lit(param(4, 0), 0));

            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:memcpy"),
                }
                .into(),
            );
        }

        (cmds, None)
    } else {
        panic!("{:?}", arguments);
    }
}

fn setup_arguments(
    arguments: &[(Operand, Vec<ParameterAttribute>)],
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
    tys: &Types,
) -> Vec<Command> {
    let mut before_cmds = Vec::new();

    // Set arguments
    for (idx, (arg, _attrs)) in arguments.iter().enumerate() {
        match eval_maybe_const(arg, globals, tys) {
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
    tys: &Types,
) -> Vec<Command> {
    assert_eq!(operand0.get_type(tys), operand1.get_type(tys));

    let layout = type_layout(&operand0.get_type(tys), tys);

    if matches!(&*operand0.get_type(tys), Type::IntegerType { bits: 1 }) {
        let (mut cmds, op0) = eval_operand(operand0, globals, tys);

        let (tmp, op1) = eval_operand(operand1, globals, tys);

        cmds.extend(tmp);

        let dest = ScoreHolder::from_local_name(dest.clone(), layout.size());
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
        compile_bitwise_word(operand0, operand1, dest.clone(), BitOp::Xor, globals, tys)
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
    tys: &Types,
) -> Vec<Command> {
    let operand1_is_32 = as_const_64(operand1) == Some(32);

    if matches!(&*operand0.get_type(tys), Type::IntegerType { bits: 64 }) && operand1_is_32 {
        let (mut cmds, op0) = eval_operand(operand0, globals, tys);
        let dest = ScoreHolder::from_local_name(dest.clone(), 8);

        cmds.push(assign_lit(dest[0].clone(), 0));
        cmds.push(assign(dest[1].clone(), op0[0].clone()));

        cmds
    } else if matches!(&*operand0.get_type(tys), Type::IntegerType { bits: 64 }) {
        let (dest_lo, dest_hi) =
            if let [dest_lo, dest_hi] = &ScoreHolder::from_local_name(dest.clone(), 6)[..] {
                (dest_lo.clone(), dest_hi.clone())
            } else {
                unreachable!()
            };

        let (mut cmds, op0) = eval_operand(operand0, globals, tys);

        if let Some(shift) = as_const_64(operand1) {
            let mut intra_byte = |max_bound: i32, dest_lo: ScoreHolder, dest_hi: ScoreHolder| {
                let mul = 1 << shift;
                cmds.push(mark_assertion_matches(false, op0[1].clone(), 0..=max_bound));

                cmds.push(assign(dest_lo.clone(), op0[0].clone()));
                cmds.push(assign(dest_hi.clone(), op0[1].clone()));
                let tmp = get_unique_holder();
                cmds.push(assign_lit(tmp.clone(), mul));
                cmds.push(make_op(dest_lo, "*=", tmp.clone()));
                cmds.push(make_op(dest_hi.clone(), "*=", tmp.clone()));
                
                let tmp2 = get_unique_holder();
                
                cmds.push(assign(tmp2.clone(),op0[0].clone()));
                cmds.push(assign_lit(tmp.clone(),1 << (32 - shift)));
                cmds.push(make_op(tmp2.clone(), "/=", tmp));
                cmds.push(make_op(dest_hi, "+=", tmp2));
            };
            
            if shift < 32 {
                intra_byte(((4294967295 as i64) >> shift) as i32, dest_lo, dest_hi);
            } else {
                let tmp = get_unique_holder();

                cmds.push(assign_lit(dest_lo, 0));
                cmds.push(assign_lit(tmp.clone(), 1 << (shift - 32)));
                cmds.push(assign(dest_hi.clone(), op0[0].clone()));
                cmds.push(make_op(dest_hi, "*=", tmp));
            }
        } else {
            match eval_maybe_const(operand1,globals,tys) {
                MaybeConst::NonConst(tmp, op1) => {
                    let mut op0i = op0.into_iter();
                    let mut op1i = op1.into_iter();
                    let (dest_lo, dest_hi) = 
                        if let [dest_lo, dest_hi] = &ScoreHolder::from_local_name(dest.clone(), 8)[..] {
                            (dest_lo.clone(), dest_hi.clone())
                        } else {
                            unreachable!()
                        };

                    cmds.extend(tmp);
                    cmds.push(assign(param(0, 0), op0i.next().unwrap()));
                    cmds.push(assign(param(0, 1), op0i.next().unwrap()));
                    cmds.push(assign(param(1, 0), op1i.next().unwrap()));
                    cmds.push(assign(param(1, 1), op1i.next().unwrap()));
                    cmds.push(
                        McFuncCall {
                            id: McFuncId::new("intrinsic:shl64"),
                        }
                        .into(),
                    );
                    cmds.push(assign(dest_lo, param(0, 0)));
                    cmds.push(assign(dest_hi, param(0, 1)));
                }
                _ => todo!("Add handling for {:?}", operand1)
            }
        }

        cmds
    } else if matches!(&*operand0.get_type(tys), Type::IntegerType { bits: 48 }) {
        let (dest_lo, dest_hi) =
            if let [dest_lo, dest_hi] = &ScoreHolder::from_local_name(dest.clone(), 6)[..] {
                (dest_lo.clone(), dest_hi.clone())
            } else {
                unreachable!()
            };

        let shift = as_const_int(48, operand1).unwrap();

        let (mut cmds, op0) = eval_operand(operand0, globals, tys);

        cmds.push(mark_assertion_matches(false, op0[1].clone(), 0..=0));

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
                cmds.push(mark_assertion_matches(false, op0[0].clone(), 0..=0xFF));

                cmds.push(assign_lit(dest_lo, 0));
                cmds.push(assign(dest_hi, op0[0].clone()));
            }
            _ => todo!("{:?}", operand1),
        };

        cmds
    } else {
        let op0_type = operand0.get_type(tys);

        let bits = match &*op0_type {
            Type::IntegerType { bits: 32 } => 32,
            Type::IntegerType { bits: 24 } => 24,
            Type::IntegerType { bits: 16 } => 16,
            Type::IntegerType { bits: 8 } => 8,
            _ => todo!("{:?}, shift: {:?}", operand0, operand1),
        };

        let (mut cmds, op0) = eval_operand(operand0, globals, tys);
        let op0 = op0.into_iter().next().unwrap();

        let dest = ScoreHolder::from_local_name(dest.clone(), 4)
            .into_iter()
            .next()
            .unwrap();

        match eval_maybe_const(operand1, globals, tys) {
            MaybeConst::Const(c) => {
                cmds.push(assign(dest.clone(), op0));
                cmds.push(make_op_lit(dest.clone(), "*=", 1 << c));

                if !matches!(&*op0_type, Type::IntegerType { bits: 32 }) {
                    let max_val = match &*op0_type {
                        Type::IntegerType { bits: 8 } => 255,
                        Type::IntegerType { bits: 16 } => 65535,
                        Type::IntegerType { bits: 24 } => 16777216,
                        ty => todo!("{:?}", ty),
                    };

                    cmds.push(mark_assertion_matches(false, dest, ..=max_val));
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
        debugloc,
    }: &LShr,
    globals: &GlobalVarList,
    tys: &Types,
) -> Vec<Command> {
    let (mut cmds, op0) = eval_operand(operand0, globals, tys);

    let op0_type = operand0.get_type(tys);

    if let Some(value) = as_const_64(operand1) {
        assert!(matches!(&*op0_type, Type::IntegerType { bits: 64 }));

        cmds.extend(lshr_64_bit_const(
            op0[0].clone(),
            op0[1].clone(),
            value as i32,
            dest.clone(),
        ));

        cmds
    } else {
        if let Type::IntegerType { bits } = &*op0_type {
            // this error was moved down
        } else {
            todo!("{:?}", operand0);
        }

        let (tmp, op1) = eval_operand(operand1, globals, tys);
        let op1 = op1.into_iter().next().unwrap();

        if op0.len() == 1 {
            let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                .into_iter()
                .next()
                .unwrap();

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
        } else if op0.len() == 2 {
            let mut dest = ScoreHolder::from_local_name(dest.clone(), 8).into_iter();

            cmds.extend(tmp);
            cmds.push(assign(param(0, 0), op0[0].clone()));
            cmds.push(assign(param(0, 1), op0[1].clone()));
            cmds.push(assign(param(1, 0), op1));
            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:lshr64"),
                }
                .into(),
            );
            cmds.push(assign(dest.next().unwrap(), param(0, 0)));
            cmds.push(assign(dest.next().unwrap(), param(0, 1)));
        } else {
            dumploc(debugloc);
            todo!("Logical Shift Right with {} bits",op0.len());
        }

        cmds
    }
}

fn compile_ashr(
    AShr {
        operand0,
        operand1,
        dest,
        debugloc,
    }: &AShr,
    globals: &GlobalVarList,
    tys: &Types,
) -> Vec<Command> {
    let (mut cmds, op0) = eval_operand(operand0, globals, tys);

    let op0_type = operand0.get_type(tys);

    if let Some(value) = as_const_64(operand1) {
        dumploc(debugloc);
        todo!("[FATAL] 64-bit arithmetic shift right is not supported.");
    } else {
        if let Type::IntegerType { bits } = &*op0_type {
            // this error was moved down
        } else {
            dumploc(debugloc);
            todo!("[FATAL] Arithmetic shift right is only implemented for integers.");
        }

        let (tmp, op1) = eval_operand(operand1, globals, tys);
        let op1 = op1.into_iter().next().unwrap();

        if op0.len() == 1 {
            let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                .into_iter()
                .next()
                .unwrap();

            cmds.extend(tmp);
            cmds.push(assign(param(0, 0), op0[0].clone()));
            cmds.push(assign(param(1, 0), op1));
            cmds.push(
                McFuncCall {
                    id: McFuncId::new("intrinsic:ashr"),
                }
                .into(),
            );
            cmds.push(assign(dest, param(0, 0)));
        } else {
            dumploc(debugloc);
            todo!("[FATAL] Arithmetic Shift Right with {} bits is unimplemented", op0.len());
        }

        cmds
    }
}

fn compile_call(
    Call {
        function,
        arguments,
        dest,
        debugloc,
        ..
    }: &Call,
    globals: &GlobalVarList,
    tys: &Types,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let function = match function {
        Either::Left(asm) => todo!("inline assembly {:?}", asm),
        Either::Right(operand) => operand,
    };

    let static_call = if let Operand::ConstantOperand(c) = function {
        if let Constant::GlobalReference { name: Name::Name(name), ty } = &**c {
            if let Type::FuncType { result_type, is_var_arg: false, .. } = &**ty {
                Some((name, result_type))
            } else {
                None
            }
        } else if let Constant::BitCast (llvm_ir::constant::BitCast { operand, to_type }) = &**c {
            let (ref_name, ref_ty) = if let Constant::GlobalReference { name: Name::Name(name), ty } = &**operand {
                (name, ty)
            } else {
                dumploc(debugloc);
                panic!("[FATAL] Cannot handle this callee. Are you trying to double-cast?");
            };

            if let Type::FuncType { result_type: _, is_var_arg: false, .. } = &**ref_ty {

            } else {
                dumploc(debugloc);
                panic!("[FATAL] Cannot directly cast a data pointer to a function.");
            }

            let to_inner_ty = if let Type::PointerType { pointee_type, addr_space: _ } = &**to_type {
                pointee_type
            } else {
                dumploc(debugloc);
                panic!("[FATAL] Cannot call a non-pointer");
            };

            if let Type::FuncType { result_type, is_var_arg: false, .. } = &**to_inner_ty {
                Some((ref_name, result_type))
            } else {
                dumploc(debugloc);
                panic!("[FATAL] Cannot call a data pointer");
            }
        } else {
            None
        }
    } else {
        None
    };

    let local_op = if let Operand::LocalOperand { name: _, ty } = function {
        if let Type::PointerType { pointee_type, addr_space: _ } = &**ty {
            Some(pointee_type)
        } else {
            None
        }
    } else {
        None
    };

    if let Some((name, result_type)) = static_call {
        let dest_size = type_layout(result_type, tys).size();
        let dest = dest
            .clone()
            .map(|d| ScoreHolder::from_local_name(d, dest_size));

        match name.as_str() {
            "llvm.assume" => {
                assert_eq!(arguments.len(), 1);
                assert!(dest.is_none());
                println!("Assumption {:?}", arguments[0]);

                let (mut cmds, op) = eval_operand(&arguments[0].0, globals, tys);

                cmds.push(mark_assertion_matches(false, op[0].clone(), 1..=1));

                (cmds, None)
            }
            "insert_asm" => {
                assert_eq!(arguments.len(), 3);

                let ptr = arguments[0].clone();
                let len = arguments[1].clone();

                let len = as_const_32(&len.0).unwrap();


                // TODO: this is so so terribly awful

                let addr = if let Operand::ConstantOperand(c) = &ptr.0 {
                    c
                } else {
                    todo!("{:?}", ptr)
                };

                let addr = if let Constant::GetElementPtr(g) = &**addr {
                    let GetElementPtrConst {
                        address,
                        indices,
                        in_bounds: _in_bounds,
                    } = &*g;

                    let addr = if let Constant::GlobalReference { name, .. } = &**address {
                        name
                    } else {
                        todo!("{:?}", address)
                    };

                    let indices_ok =
                        indices.len() == 3 &&
                        *indices[0] == Constant::Int { bits: 32, value: 0 } &&
                        *indices[1] == Constant::Int { bits: 32, value: 0 } &&
                        *indices[2] == Constant::Int { bits: 32, value: 0 };

                    if !indices_ok {
                        todo!("{:?}", indices)
                    }

                    addr
                } else {
                    todo!("{:?}", addr)
                };

                let data = &globals.get(addr).unwrap().1;

                let data = if let Constant::Struct {
                    values,
                    is_packed: true,
                    ..
                } = data.as_ref().unwrap()
                {
                    if let [c] = &values[..]
                    {
                        if let Constant::Array {
                            element_type,
                            elements,
                        } = &**c {
                            if element_type == &tys.i8() {
                                elements
                            } else {
                                todo!("{:?}", element_type)
                            }
                        } else {
                            todo!("{:?}", c)
                        }
                    } else {
                        todo!("{:?}", values)
                    }
                } else {
                    todo!("{:?}", data)
                };

                let data = data[..len as usize]
                    .iter()
                    .map(|d| {
                        if let Constant::Int { bits: 8, value } = &**d {
                            *value as u8
                        } else {
                            unreachable!()
                        }
                    })
                    .collect::<Vec<u8>>();

                let text = std::str::from_utf8(&data).unwrap();

                let (mut cmds, arg) = eval_operand(&arguments[2].0, globals, tys);
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

                let len = as_const_32(&len.0).unwrap();

                // TODO: this is so so terribly awful
                let addr = if let Operand::ConstantOperand(c) = &ptr.0 {
                    c
                } else {
                    todo!("{:?}", ptr)
                };

                let addr = if let Constant::GetElementPtr(g) = &**addr {
                    let GetElementPtrConst {
                        address,
                        indices,
                        in_bounds: _in_bounds,
                    } = &*g;

                    let addr = if let Constant::GlobalReference { name, .. } = &**address {
                        name
                    } else {
                        todo!("{:?}", address)
                    };

                    let indices_ok =
                        indices.len() == 3 &&
                        *indices[0] == Constant::Int { bits: 32, value: 0 } &&
                        *indices[1] == Constant::Int { bits: 32, value: 0 } &&
                        *indices[2] == Constant::Int { bits: 32, value: 0 };

                    if !indices_ok {
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
                    if let [c] = &values[..]
                    {
                        if let Constant::Array {
                            element_type,
                            elements,
                        } = &**c {
                            if element_type == &tys.i8() {
                                elements
                            } else {
                                todo!("{:?}", element_type)
                            }
                        } else {
                            todo!("{:?}", c)
                        }
                    } else {
                        todo!("{:?}", values)
                    }
                } else {
                    todo!("{:?}", data)
                };

                let data = data[..len as usize]
                    .iter()
                    .map(|d| {
                        if let Constant::Int { bits: 8, value } = &**d {
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

                let (mut cmds, name) = eval_operand(&arguments[0].0, globals, tys);

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

                let mut cmds = vec![Command::Comment("call to turtle_set".to_string())];

                match eval_maybe_const(&arguments[0].0, globals, tys) {
                    MaybeConst::Const(mc_block) => {
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
                        cmds.push(cmd.into())
                    }
                    MaybeConst::NonConst(tmp, mc_block) => {
                        cmds.extend(tmp);

                        let mc_block = mc_block.into_iter().next().unwrap();

                        // TODO: Check for an invalid argument

                        for possible in MC_BLOCKS.iter().copied() {
                            let mut cmd = Execute::new();
                            cmd.with_if(ExecuteCondition::Score {
                                target: mc_block.clone().into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((possible as i32..=possible as i32).into()),
                            });
                            cmd.with_at(Target::Selector(cir::Selector {
                                var: cir::SelectorVariable::AllEntities,
                                args: vec![cir::SelectorArg("tag=turtle".to_string())],
                            }));
                            cmd.with_run(SetBlock {
                                block: possible.to_string(),
                                pos: "~ ~ ~".to_string(),
                                kind: SetBlockKind::Replace,
                            });
                            cmds.push(cmd.into())
                        }
                    }
                }

                (cmds, None)
            }
            "turtle_check" => {
                assert_eq!(arguments.len(), 1);

                let dest = dest.as_ref().expect("turtle_check should return a value");
                assert_eq!(dest.len(), 1);
                let dest = dest[0].clone();

                let mc_block =
                    if let MaybeConst::Const(c) = eval_maybe_const(&arguments[0].0, globals, tys) {
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

                match eval_maybe_const(&arguments[0].0, globals, tys) {
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

                cmds.extend(setup_arguments(arguments, globals, tys));

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
                let mut cmds = setup_arguments(arguments, globals, tys);
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
                (compile_memset(arguments, globals, tys), None)
            }
            "llvm.memcpy.p0i8.p0i8.i32" => {
                assert_eq!(dest, None);
                compile_memcpy(arguments, globals, tys)
            }
            "llvm.usub.with.overflow.i8" => {
                let dest = dest.unwrap().into_iter().next().unwrap();

                if let [(lhs, _), (rhs, _)] = &arguments[..] {
                    let (mut cmds, l) = eval_operand(lhs, globals, tys);
                    let (tmp, r) = eval_operand(rhs, globals, tys);
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
                    let (mut cmds, l) = eval_operand(lhs, globals, tys);
                    let (tmp, r) = eval_operand(rhs, globals, tys);
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

                cmds.extend(setup_arguments(arguments, globals, tys));

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
                let mut before_cmds = Vec::new();

                before_cmds.push(Command::Comment(format!("Calling {}", name)));

                // Push return address
                before_cmds.extend(push(ScoreHolder::new("%%fixup_return_addr".to_string()).unwrap()));

                before_cmds.extend(setup_arguments(arguments, globals, tys));

                // Branch to function
                before_cmds.push(Command::Comment(format!("!FIXUPCALL {}", name)));

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
    } else if let Some(pointee_type) = local_op {
        let (mut before_cmds, func_ptr) = eval_operand(function, globals, tys);
        assert_eq!(func_ptr.len(), 1);
        let func_ptr = func_ptr.into_iter().next().unwrap();

        if let Type::FuncType {
            result_type,
            is_var_arg: false,
            ..
        } = &**pointee_type
        {
            let dest_size = type_layout(result_type, tys).size();
            let dest = dest
                .clone()
                .map(|d| ScoreHolder::from_local_name(d, dest_size));

            // Push return address
            before_cmds.extend(push(ScoreHolder::new("%%fixup_return_addr".to_string()).unwrap()));

            before_cmds.extend(setup_arguments(arguments, globals, tys));

            before_cmds.push(Command::Comment(format!("!DYNCALL {}", func_ptr)));

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

pub(crate) fn compile_terminator(
    parent: &Function,
    term: &Terminator,
    clobbers: BTreeSet<ScoreHolder>,
    globals: &GlobalVarList,
    tys: &Types,
) -> (Vec<Command>, Either<Vec<(BlockEdge, McFuncId)>, McFuncId>) {
    let mut cmds = Vec::new();

    match &term {
        Terminator::Ret(Ret {
            return_operand: None,
            ..
        }) => {
            cmds.push(Command::Comment("return".to_string()));

            cmds.extend(load_regs(clobbers));

            (cmds, Either::Right(McFuncId::new("rust:__langcraft_return")))
        }
        Terminator::Ret(Ret {
            return_operand: Some(operand),
            ..
        }) => {
            cmds.push(Command::Comment(format!("return operand {:?}", operand)));

            let (tmp, source) = eval_operand(operand, globals, tys);

            cmds.extend(tmp);

            for (idx, word) in source.into_iter().enumerate() {
                cmds.push(assign(return_holder(idx), word));
            }

            cmds.extend(load_regs(clobbers));

            (cmds, Either::Right(McFuncId::new("rust:__langcraft_return")))
        }
        Terminator::Br(Br { dest, .. }) => {
            (Vec::new(), Either::Left(vec![(BlockEdge::None, McFuncId::new_block(&parent.name, dest.clone()))]))
        }
        Terminator::CondBr(CondBr {
            condition,
            true_dest,
            false_dest,
            ..
        }) => {
            let (tmp, cond) = eval_operand(condition, globals, tys);
            cmds.extend(tmp);

            assert_eq!(cond.len(), 1);
            let cond = cond[0].clone();

            let true_dest = McFuncId::new_block(&parent.name, true_dest.clone());
            let false_dest = McFuncId::new_block(&parent.name, false_dest.clone());

            (cmds, Either::Left(vec![
                (BlockEdge::Cond { value: cond.clone(), inverted: false }, true_dest),
                (BlockEdge::Cond { value: cond,         inverted: true  }, false_dest),
            ]))

            /*let mut true_cmd = Execute::new();
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

            cmds*/
        }
        Terminator::Switch(Switch {
            operand,
            dests,
            default_dest,
            ..
        }) => {
            let (tmp, op) = eval_operand(operand, globals, tys);
            cmds.extend(tmp);

            let operand = match &*operand.get_type(tys) {
                Type::IntegerType { bits: 32 } => {
                    op.into_iter().next().unwrap()
                }
                Type::IntegerType { bits: 16 } => {
                    let op = op.into_iter().next().unwrap();

                    let tmp = get_unique_holder();
                    
                    cmds.push(assign(tmp.clone(), op));
                    cmds.push(make_op_lit(tmp.clone(), "%=", 65536));

                    tmp
                }
                Type::IntegerType { bits: 8 } => {
                    let op = op.into_iter().next().unwrap();

                    let tmp = get_unique_holder();
                    
                    cmds.push(assign(tmp.clone(), op));
                    cmds.push(make_op_lit(tmp.clone(), "%=", 256));

                    tmp
                }
                o => todo!("{:?}", o)
            };


            let mut edges = dests.iter().map(|(dest_value, dest_name)| {
                let dest_id = McFuncId::new_block(&parent.name, dest_name.clone());

                let edge = match &*dest_value.get_type(tys) {
                    Type::IntegerType { bits: 32 } |
                    Type::IntegerType { bits: 16 } |
                    Type::IntegerType { bits: 8 } => {
                        if let MaybeConst::Const(expected) = eval_constant(dest_value, globals, tys) {
                            BlockEdge::SwitchCond { 
                                value: operand.clone(),
                                expected,
                            }
                        } else {
                            unreachable!()
                        }
                    }
                    _ => todo!()
                };

                (edge, dest_id)
            }).collect::<Vec<_>>();

            let default_dest_id = McFuncId::new_block(&parent.name, default_dest.clone());

            let not_expected = dests.iter().map(|(dest_value, _)| {
                match &*dest_value.get_type(tys) {
                    Type::IntegerType { bits: 32 } |
                    Type::IntegerType { bits: 16 } | 
                    Type::IntegerType { bits: 8 } => {
                        if let MaybeConst::Const(ne) = eval_constant(dest_value, globals, tys) {
                            ne
                        } else {
                            unreachable!()
                        }
                    }
                    _ => todo!()
                }
            }).collect();

            let default_edge = BlockEdge::SwitchDefault {
                value: operand,
                not_expected,
            };

            edges.push((default_edge, default_dest_id));

            (cmds, Either::Left(edges))
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

            (cmds, Either::Left(Vec::new()))
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

            (cmds, Either::Left(Vec::new()))
        }
        term => todo!("terminator {:?}", term),
    }
}

#[allow(clippy::reversed_empty_ranges)]
fn reify_block(AbstractBlock { needs_prolog, mut body, term, parent }: AbstractBlock, clobber_list: &HashMap<String, BTreeSet<ScoreHolder>>, func_starts: &HashMap<String, McFuncId>, globals: &GlobalVarList, tys: &Types) -> McFunction {
    let mut clobbers = clobber_list.get(&body.id.name).unwrap().clone();

    for arg in parent.parameters.iter() {
        let arg_size = type_layout(&arg.ty, tys).size();
        clobbers.extend(ScoreHolder::from_local_name(arg.name.clone(), arg_size).iter().cloned());
    }

    if needs_prolog {
        let mut prolog = save_regs(clobbers.clone());

        for (idx, arg) in parent.parameters.iter().enumerate() {
            let arg_size = type_layout(&arg.ty, tys).size();

            for (arg_word, arg_holder) in
                ScoreHolder::from_local_name(arg.name.clone(), arg_size)
                    .into_iter()
                    .enumerate()
            {
                prolog.push(assign(arg_holder, param(idx, arg_word)));
            }
        }

        body.cmds.splice(0..0, prolog);

        // FIXME: When `trace-bbs` is enabled this puts them at the correct place
        // body.cmds.splice(1..1, prolog);
    }

    body.cmds.extend(compile_block_end(&term.unwrap(), body.cmds.len(), &parent, clobbers, &func_starts, globals, tys));

    body
}

static RESUME_BLOCK_POS: &str = "-2 1 1";
static ACTIVATE_BLOCK_POS: &str = "-2 1 0";

fn compile_block_end(block_end: &BlockEnd, body_cmds: usize, parent: &Function, clobbers: BTreeSet<ScoreHolder>, func_starts: &HashMap<String, McFuncId>, globals: &GlobalVarList, tys: &Types) -> Vec<Command> {
    let mut cmds = Vec::new();

    let dests: Either<Vec<(BlockEdge, McFuncId)>, McFuncId> = match block_end {
        BlockEnd::StaticCall(func_name) => {
            if func_name == "!FIXUPCALL intrinsic:memcpy" {
                todo!()
            } else {
                assert!(func_name.starts_with("!FIXUPCALL "));
                let func_name = &func_name["!FIXUPCALL ".len()..];
                let func_id = func_starts.get(func_name).unwrap().clone();
                Either::Left(vec![(BlockEdge::None, func_id)])
            }
        }
        BlockEnd::DynCall(func_ptr) => {
            cmds.push(assign(temp_fn_ptr(), func_ptr.clone()));
            Either::Right(McFuncId::new("rust:__langcraft_call"))

            /*
            // FIXME: This is identical to the one at the end of `compile_call`
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

            body.cmds.push(set_block.into());
            */
        }
        BlockEnd::Normal(t) => {
            let (tmp, dests) = compile_terminator(&parent, &t, clobbers, globals, tys);
            cmds.extend(tmp);
            dests
        }
    };

    if let Either::Left(l) = &dests {
        if l.is_empty() {
            // We have somehow reached an unreachable block

            cmds.push(SetBlock {
                pos: "~ ~ ~".into(),
                block: "minecraft:air".into(),
                kind: cir::SetBlockKind::Replace,
            }.into());
            cmds.push(SetBlock {
                pos: "-2 0 0".into(),
                block: "minecraft:air".into(),
                kind: cir::SetBlockKind::Replace,
            }.into());

            return cmds;
        }
    }

    // FIXME: Actually figure out how many commands are added by this method correctly
    // Update command count
    cmds.push(make_op_lit(cmd_count(), "+=", body_cmds as i32 + 10));

    // All commands used when under the threshold share the same prefix
    let under_thresh_base = {
        let mut tmp = Execute::new();
        tmp.with_if(ExecuteCondition::Score {
            target: cmd_count().into(),
            target_obj: OBJECTIVE.into(),
            kind: ExecuteCondKind::Relation {
                relation: cir::Relation::LessThan,
                source: cmd_limit().into(),
                source_obj: OBJECTIVE.into(),
            }
        });
        tmp
    };

    // If command count < threshold:
    //  modify the next command block
    //  set the next pointer to the current command block

    match &dests {
        Either::Left(dests) => {
            for (edge, dest) in dests.iter().cloned() {
                let mut modify_next = under_thresh_base.clone();
                modify_next.with_at(cir::Selector {
                    var: cir::SelectorVariable::AllEntities,
                    args: vec![cir::SelectorArg("tag=next".into())],
                }.into());
                for (cond, is_unless) in edge.into_conds() {
                    modify_next.with_subcmd(ExecuteSubCmd::Condition { is_unless, cond });
                }
                modify_next.with_run(Data {
                    target: DataTarget::Block("~ ~ ~".to_string()),
                    kind: cir::DataKind::Modify {
                        path: "Command".to_string(),
                        kind: cir::DataModifyKind::Set,
                        source: cir::DataModifySource::ValueString(McFuncCall { id: dest }.to_string()),
                    },
                });

                cmds.push(modify_next.into());
            }
        }
        Either::Right(id) => {
            let mut modify_next = under_thresh_base.clone();
            modify_next.with_at(cir::Selector {
                var: cir::SelectorVariable::AllEntities,
                args: vec![cir::SelectorArg("tag=next".into())],
            }.into());
            modify_next.with_run(McFuncCall { id: id.clone() });
            cmds.push(modify_next.into());
        }
    }
    let mut set_next_ptr = under_thresh_base;
    set_next_ptr.with_run(cir::Teleport {
        target: cir::Selector {
            var: cir::SelectorVariable::AllEntities,
            args: vec![cir::SelectorArg("tag=next".into())],
        }.into(),
        pos: "~ ~ ~".to_string(),
    });
    cmds.push(set_next_ptr.into());

    // If command count >= threshold:
    //  set resume command
    //  clear the next command block
    //  activate starting command block
    //  reset command count

    // All commands used when over the threshold share the same prefix
    let over_thresh_base = {
        let mut tmp = Execute::new();
        tmp.with_if(ExecuteCondition::Score {
            target: cmd_count().into(),
            target_obj: OBJECTIVE.into(),
            kind: ExecuteCondKind::Relation {
                relation: cir::Relation::GreaterThanEq,
                source: cmd_limit().into(),
                source_obj: OBJECTIVE.into(),
            }
        });
        tmp
    };

    match &dests {
        Either::Left(dests) => {
            for (edge, dest) in dests.iter().cloned() {
                let mut modify_resume = over_thresh_base.clone();
                for (cond, is_unless) in edge.into_conds() {
                    modify_resume.with_subcmd(ExecuteSubCmd::Condition { is_unless, cond });
                }
                modify_resume.with_run(Data {
                    target: DataTarget::Block(RESUME_BLOCK_POS.into()),
                    kind: cir::DataKind::Modify {
                        path: "Command".to_string(),
                        kind: cir::DataModifyKind::Set,
                        source: cir::DataModifySource::ValueString(McFuncCall { id: dest }.to_string()),
                    },
                });

                cmds.push(modify_resume.into());
            }
        }
        Either::Right(id) => {
            let mut modify_resume = over_thresh_base.clone();
            modify_resume.with_positioned(RESUME_BLOCK_POS.into());
            modify_resume.with_run(McFuncCall { id: id.clone() });
            cmds.push(modify_resume.into());
        }
    }

    let mut clear_next = over_thresh_base.clone();
    clear_next.with_at(cir::Selector {
        var: cir::SelectorVariable::AllEntities,
        args: vec![cir::SelectorArg("tag=next".into())],
    }.into());
    clear_next.with_run(SetBlock {
        pos: "~ ~ ~".into(),
        block: "minecraft:air".into(),
        kind: cir::SetBlockKind::Replace,
    });
    cmds.push(clear_next.into());

    let mut activate_next = over_thresh_base.clone();
    activate_next.with_run(SetBlock {
        block: "minecraft:redstone_block".to_string(),
        pos: ACTIVATE_BLOCK_POS.into(),
        kind: SetBlockKind::Replace,
    });
    cmds.push(activate_next.into());

    let mut reset_count = over_thresh_base;
    reset_count.with_run(assign_lit(cmd_count(), 0));
    cmds.push(reset_count.into());

    cmds
}

fn compile_function(
    func: &Function,
    globals: &GlobalVarList,
    tys: &Types,
    options: &BuildOptions,
) -> (Vec<AbstractBlock>, HashMap<ScoreHolder, cir::HolderUse>) {
    if func.is_var_arg {
        todo!("functions with variadic arguments");
    }

    if func.basic_blocks.is_empty() {
        todo!("functions with no basic blocks");
    }

    println!("Function {}, {}", func.name, func.basic_blocks.len());

    let mut funcs = func
        .basic_blocks
        .iter()
        .enumerate()
        .flat_map(|(idx, block)| {
            let mut result = Vec::new();

            let mut sub = 0;

            let make_new_func = |sub| {
                let id = McFuncId::new_sub(func.name.clone(), block.name.clone(), sub);
                let cmds = if options.trace_bbs {
                    vec![print_entry(&id)]
                } else {
                    vec![]
                };
                McFunction {
                    id,
                    cmds,
                }
            };

            let mut this = make_new_func(sub);
            sub += 1;

            if idx == 0 {
            }

            for instr in block.instrs.iter() {
                let (mut before, after) = compile_instr(instr, func, globals, tys, options);

                if let Some(after) = after {
                    let term = match before.pop().unwrap() {
                        Command::Comment(c) if c.starts_with("!FIXUPCALL") => {
                            BlockEnd::StaticCall(c)
                        }
                        Command::Comment(c) if c.starts_with("!DYNCALL ") => {
                            let holder = ScoreHolder::new(c["!DYNCALL ".len()..].to_string()).unwrap();
                            BlockEnd::DynCall(holder)
                        }
                        b => todo!("{:?}", b),
                    };

                    this.cmds.extend(before);

                    result.push(AbstractBlock {
                        parent: func.clone(),
                        needs_prolog: idx == 0 && sub == 1,
                        body: std::mem::replace(&mut this, make_new_func(sub)),
                        term: Some(term),
                    });
                    sub += 1;

                    this.cmds.extend(after);
                } else {
                    this.cmds.extend(before);
                }
            }

            this.cmds.push(assign_lit(
                ScoreHolder::new("%phi".to_string()).unwrap(),
                idx as i32,
            ));

            result.push(AbstractBlock {
                parent: func.clone(),
                needs_prolog: idx == 0 && sub == 1,
                body: this,
                term: Some(BlockEnd::Normal(block.term.clone())),
            });

            /*for sub_block in result.iter_mut() {
                sub_block.body.cmds.insert(
                    0,
                    SetBlock {
                        pos: "~ ~1 ~".to_string(),
                        block: "minecraft:air".to_string(),
                        kind: SetBlockKind::Replace,
                    }
                    .into(),
                );
            }*/

            result
        })
        .collect::<Vec<_>>();

    /*for (idx, func) in funcs.iter().enumerate() {
        println!("Body command count for {}: {:?}", func.body.id, estimate_body_cmds(&funcs, idx));
    }*/

    for func in funcs.iter_mut() {
        for cmd in func.body.cmds.iter_mut() {
            if let Command::Execute(Execute {
                run: Some(func_call),
                ..
            }) = cmd {
                if let Command::ScoreGet(ScoreGet {
                    target: Target::Uuid(target),
                    ..
                }) = &mut **func_call
                {
                    if target.as_ref() == "%%fixup_return_addr" {
                        // FIXME: Also ewwww aaaaaa help
                        *target = ScoreHolder::new_unchecked(format!("{}%%fixup_return_addr", func.body.id));
                    }
                }
            }
        }
    }

    let mut clobbers = HashMap::new();
    for f in funcs.iter() {
        for c in f.body.cmds.iter() {
            cir::merge_uses(&mut clobbers, &c.holder_uses());
        }
    }

    let clobbers = clobbers
        .into_iter()
        .map(|(c, u)| ((*c).clone(), u))
        .collect::<HashMap<_, _>>();

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
            cmds.push(assign(dest_lo, op0_hi));
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
    tys: &Types,
) -> Vec<Command> {
    let (mut cmds, source0) = eval_operand(operand0, globals, tys);
    let (tmp, source1) = eval_operand(operand1, globals, tys);
    cmds.extend(tmp.into_iter());

    let op0_type = operand0.get_type(tys);

    if matches!(&*op0_type, Type::IntegerType { bits: 64 }) {
        let op0_lo = source0[0].clone();
        let op0_hi = source0[1].clone();

        let op1_lo = source1[0].clone();
        let op1_hi = source1[1].clone();

        match kind {
            ScoreOpKind::AddAssign => {
                cmds.extend(add_64_bit(op0_lo, op0_hi, op1_lo, op1_hi, dest.clone()));
            }
            ScoreOpKind::SubAssign => {
                let op1_inv_lo = get_unique_holder();
                let op1_inv_hi = get_unique_holder();

                cmds.push(assign(op1_inv_lo.clone(), op1_lo));
                cmds.push(assign(op1_inv_hi.clone(), op1_hi));

                cmds.extend(invert(op1_inv_lo.clone()));
                cmds.extend(invert(op1_inv_hi.clone()));

                let op1_neg_name = Name::from(format!("%%temp_neg_{}", get_unique_num()));

                cmds.extend(add_64_bit(
                    op1_inv_lo, 
                    op1_inv_hi,
                    ScoreHolder::new("%%1".into()).unwrap(),
                    ScoreHolder::new("%%0".into()).unwrap(),
                    op1_neg_name.clone()
                ));

                let mut tmp = ScoreHolder::from_local_name(op1_neg_name, 8).into_iter();
                let op1_neg_lo = tmp.next().unwrap();
                let op1_neg_hi = tmp.next().unwrap();

                cmds.extend(add_64_bit(op0_lo, op0_hi, op1_neg_lo, op1_neg_hi, dest.clone()));
            }
            ScoreOpKind::MulAssign => {
                cmds.extend(mul_64_bit(op0_lo, op0_hi, op1_lo, op1_hi, dest.clone()));
            }
            _ => todo!("{:?}", kind),
        }

        cmds
    } else {
        let dest =
            ScoreHolder::from_local_name(dest.clone(), type_layout(&*op0_type, tys).size());

        if let Type::VectorType {
            element_type,
            num_elements,
        } = &*op0_type
        {
            if !matches!(&**element_type, Type::IntegerType { bits: 32 }) {
                todo!("{:?}", element_type)
            }

            assert_eq!(source0.len(), *num_elements);
            assert_eq!(source1.len(), *num_elements);
            assert_eq!(dest.len(), *num_elements);
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

pub fn offset_of_array(element_type: &Type, field: u32, tys: &Types) -> usize {
    let mut offset = 0;
    let mut result = Layout::from_size_align(0, 1).unwrap();
    for _ in 0..field + 1 {
        let (r, o) = result.extend(type_layout(element_type, tys)).unwrap();
        offset = o;
        result = r;
    }
    offset
}

pub fn offset_of(element_types: &[TypeRef], is_packed: bool, field: u32, tys: &Types) -> usize {
    if is_packed {
        element_types[0..field as usize]
            .iter()
            .map(|t| type_layout(t, tys).size())
            .sum::<usize>()
    } else {
        let mut offset = 0;
        let mut result = Layout::from_size_align(0, 1).unwrap();
        for elem in &element_types[0..field as usize + 1] {
            let (r, o) = result.extend(type_layout(elem, tys)).unwrap();
            offset = o;
            result = r;
        }
        offset
    }
}

pub fn type_layout(ty: &Type, tys: &Types) -> Layout {
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
                    element_types.iter().map(|e| type_layout(e, tys).size()).sum(),
                    1,
                )
                .unwrap()
            } else if element_types.is_empty() {
                Layout::from_size_align(0, 1).unwrap()
            } else {
                let mut result = type_layout(&element_types[0], tys);
                for elem in &element_types[1..] {
                    result = result.extend(type_layout(elem, tys)).unwrap().0;
                }
                result
            }
        }
        Type::NamedStructType { name } => {
            let ty = tys.named_struct_def(name).unwrap();

            type_layout(named_as_type(ty).unwrap(), tys)
        }
        Type::VectorType {
            element_type,
            num_elements,
        } => {
            let mut result = type_layout(element_type, tys);
            for _ in 0..num_elements - 1 {
                result = result.extend(type_layout(element_type, tys)).unwrap().0;
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
                let mut result = type_layout(element_type, tys);
                for _ in 0..num_elements - 1 {
                    result = result.extend(type_layout(element_type, tys)).unwrap().0;
                }
                result
            }
        }
        Type::PointerType { .. } => Layout::from_size_align(4, 4).unwrap(),
        Type::VoidType => Layout::from_size_align(0, 4).unwrap(),
        _ => todo!("size of type {:?}", ty),
    }
}

/*
 * Tells the user where the program experienced the error (if included)
 */
pub fn dumploc(debugloc: &Option<llvm_ir::DebugLoc>) {
    if let Some(llvm_ir::DebugLoc {line,col,filename,directory}) = debugloc { // check if the debug location is actually attached
        eprint!("at {}:{}",filename,line); // this is the base information to give to the user
        
        // some compilers can give the column number so print that out if it's included
        if let Some(column) = col {
            eprint!(":{}",column);
        }
        
        if let Some(dir) = directory {
            eprint!(" in {}/{}",dir,filename);
        }
        
        // conclude with a colon and newline
        eprintln!(":");
    }
}

pub fn compile_alloca(
    Alloca {
        allocated_type,
        num_elements,
        dest,
        debugloc,
        ..
    }: &Alloca,
    tys: &Types,
) -> Vec<Command> {
    let type_size = type_layout(allocated_type, tys)
        .align_to(4)
        .unwrap()
        .pad_to_align()
        .size();

    let dest = ScoreHolder::from_local_name(dest.clone(), 4);
    assert_eq!(dest.len(), 1);
    let dest = dest[0].clone();

    let mut cmds = Vec::new();

    cmds.push(assign(dest, stackptr()));
    if let Some(num_elements) = as_const_32(num_elements) {
        cmds.push(make_op_lit(stackptr(), "+=", type_size as i32 * num_elements as i32));
    } else if let Operand::LocalOperand {name, ty: _} = num_elements {
        if matches!(&*num_elements.get_type(tys),llvm_ir::Type::IntegerType {bits: 32}) {
            let score = ScoreHolder::from_local_name(name.clone(),4);

            for _i in 1..type_size {
                cmds.push(make_op(stackptr(),"+=",score[0].clone()));
            }
        } else if let llvm_ir::Type::IntegerType {bits} = &*num_elements.get_type(tys) {
            dumploc(debugloc);
            todo!("Allocate with a {}-bit size",bits);
        } else {
            dumploc(debugloc);
            todo!("Allocate {:?}",&*num_elements.get_type(tys));
        }
    } else {
        todo!("{:?}", num_elements);
    };
    

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
        dest: dest_all,
        in_bounds: _,
        debugloc,
    }: &GetElementPtr,
    globals: &GlobalVarList,
    tys: &Types,
) -> Vec<Command> {
    let dest = ScoreHolder::from_local_name(dest_all.clone(), 4);
    let dest = dest[0].clone();

    let mut offset: i32 = 0;
    let mut ty = address.get_type(tys);

    let mut cmds = Vec::new();

    assert!(matches!(&*ty, Type::PointerType { .. }));

    for index in indices {
        match (*ty).clone() {
            Type::PointerType { pointee_type, .. } => {
                let pointee_size = type_layout(&pointee_type, tys).pad_to_align().size();

                ty = pointee_type;

                match eval_maybe_const(index, globals, tys) {
                    MaybeConst::Const(c) => offset += pointee_size as i32 * c,
                    MaybeConst::NonConst(a, b) => {
                        if b.len() == 1 {
                            let b = b.into_iter().next().unwrap();

                            cmds.extend(a);
                            for _ in 0..pointee_size {
                                cmds.push(make_op(dest.clone(), "+=", b.clone()));
                            }
                        } else if b.len() == 2 {
                            let dest = ScoreHolder::from_local_name(dest_all.clone(), 8);
                            let mut b = b.into_iter();

                            let (dest_lo, dest_hi) = (dest[0].clone(),dest[1].clone());
                            let b_lo = b.next().unwrap();
                            let b_hi = b.next().unwrap();
                            let add64 = add_64_bit(dest_lo,dest_hi,b_lo,b_hi,dest_all.clone());

                            cmds.extend(a);
                            for _ in 0..pointee_size {
                                cmds.extend(add64.clone());
                            }
                        } else {
                            dumploc(debugloc);
                            unreachable!("[FATAL] Invalid IR: GetElementPtr can only be word-sized (32-bit) or doubleword-sized (64-bit)");
                        }
                    }
                }
            }
            Type::StructType {
                element_types,
                is_packed,
            } => {
                let index = if let MaybeConst::Const(c) = eval_maybe_const(index, globals, tys) {
                    c
                } else {
                    unreachable!("attempt to index struct at runtime")
                };

                offset += offset_of(&element_types, is_packed, index as u32, tys) as i32;

                ty = element_types.into_iter().nth(index as usize).unwrap().clone();
            }
            Type::NamedStructType { name, .. } => {
                let index = if let MaybeConst::Const(c) = eval_maybe_const(index, globals, tys) {
                    c
                } else {
                    unreachable!("attempt to index named struct at runtime")
                };

                let (element_types, is_packed) = as_struct_ty(tys.named_struct_def(&name).unwrap()).unwrap();

                offset += offset_of(&element_types, is_packed, index as u32, tys) as i32;
                ty = element_types[index as usize].clone();
            }
            Type::ArrayType { element_type, .. } => {
                let elem_size = type_layout(&element_type, tys).pad_to_align().size();

                match eval_maybe_const(index, globals, tys) {
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

                ty = element_type;
            }
            _ => todo!("{:?}", ty),
        }
    }

    let mut start_cmds = match eval_maybe_const(address, globals, tys) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum BitOp {
    And,
    Or,
    Xor,
}

fn compile_bitwise_word(operand0: &Operand, operand1: &Operand, dest: Name, op: BitOp, globals: &GlobalVarList, tys: &Types) -> Vec<Command> {
    let ty = operand0.get_type(tys);
    let layout = type_layout(&ty, tys);

    let dest = ScoreHolder::from_local_name(dest, layout.size());

    let (mut cmds, op0) = eval_operand(operand0, globals, tys);
    let (tmp, op1) = eval_operand(operand1, globals, tys);
    cmds.extend(tmp);

    for (dest, (op0, op1)) in dest.into_iter().zip(op0.into_iter().zip(op1.into_iter())) {
        cmds.push(assign(param(0, 0), op0));
        cmds.push(assign(param(1, 0), op1));

        let id = match op {
            BitOp::And => "intrinsic:and",
            BitOp::Or => "intrinsic:or",
            BitOp::Xor => "intrinsic:xor",
        };

        cmds.push(McFuncCall {
            id: McFuncId::new(id)
        }.into());

        cmds.push(assign(dest, return_holder(0)));
    }

    cmds
}

pub fn compile_instr(
    instr: &Instruction,
    parent: &Function,
    globals: &HashMap<&Name, (u32, Option<Constant>)>,
    tys: &Types,
    _options: &BuildOptions,
) -> (Vec<Command>, Option<Vec<Command>>) {
    let result = match instr {
        // We use an empty stack
        Instruction::Alloca(alloca) => compile_alloca(alloca, tys),
        Instruction::GetElementPtr(gep) => compile_getelementptr(gep, globals, tys),
        Instruction::Select(Select {
            condition,
            true_value,
            false_value,
            dest,
            ..
        }) => {
            let (mut cmds, true_val) = eval_operand(true_value, globals, tys);
            let (tmp, false_val) = eval_operand(false_value, globals, tys);
            cmds.extend(tmp);
            let (tmp, cond) = eval_operand(condition, globals, tys);
            cmds.extend(tmp);

            let dest_size = type_layout(&true_value.get_type(tys), tys).size();

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
            let (mut cmds, addr) = eval_operand(address, globals, tys);

            assert_eq!(addr.len(), 1, "multiword addr {:?}", address);

            let addr = addr[0].clone();

            let value_size = type_layout(&value.get_type(tys), tys).size();
            if value_size % 4 == 0 && alignment % 4 == 0 {
                // If we're directly storing a constant,
                // we can skip writing to a temporary value
                let write_cmds = match eval_maybe_const(value, globals, tys) {
                    MaybeConst::Const(value) => vec![write_ptr_const(value)],
                    MaybeConst::NonConst(eval_cmds, ids) => {
                        cmds.extend(eval_cmds);

                        ids.into_iter().map(write_ptr).collect()
                    }
                };

                let tmp = get_unique_holder();
                cmds.push(assign(tmp.clone(), addr.clone()));
                cmds.push(make_op_lit(tmp.clone(), "%=", 4));
                cmds.push(mark_assertion_matches(false, tmp, 0..=0));

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
                let (eval_cmds, value) = eval_operand(value, globals, tys);
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
                let (eval_cmds, value) = eval_operand(value, globals, tys);
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
                let (tmp, val) = eval_operand(value, globals, tys);
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
                let (tmp, val) = eval_operand(value, globals, tys);
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
            let addr_ty = address.get_type(tys);
            let pointee_type = if let Type::PointerType { pointee_type, .. } = &*addr_ty {
                pointee_type
            } else {
                unreachable!()
            };

            let (mut cmds, addr) = eval_operand(address, globals, tys);

            assert_eq!(addr.len(), 1, "multiword address {:?}", address);
            let addr = addr[0].clone();

            let pointee_layout = type_layout(&pointee_type, tys);

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
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::AddAssign, globals, tys),
        Instruction::Sub(Sub {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::SubAssign, globals, tys),
        Instruction::Mul(Mul {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::MulAssign, globals, tys),
        Instruction::SDiv(SDiv {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::DivAssign, globals, tys),
        Instruction::SRem(SRem {
            operand0,
            operand1,
            dest,
            ..
        }) => compile_arithmetic(operand0, operand1, dest, ScoreOpKind::ModAssign, globals, tys),
        Instruction::UDiv(UDiv {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            let (mut cmds, source0) = eval_operand(operand0, globals, tys);
            let (tmp, source1) = eval_operand(operand1, globals, tys);
            cmds.extend(tmp.into_iter());

            // FIXME: THIS DOES AN SREM
            for s in source0.iter().cloned() {
                cmds.push(mark_assertion_matches(true, s, ..=-1));
            }

            for s in source1.iter().cloned() {
                cmds.push(mark_assertion_matches(true, s, ..=-1));
            }

            let dest = ScoreHolder::from_local_name(
                dest.clone(),
                type_layout(&operand0.get_type(tys), tys).size(),
            );

            if let Type::VectorType {
                element_type,
                num_elements,
            } = &*operand0.get_type(tys)
            {
                if !matches!(&**element_type, Type::IntegerType { bits: 32 }) {
                    todo!("{:?}", element_type)
                }

                assert_eq!(source0.len(), *num_elements);
                assert_eq!(source1.len(), *num_elements);
                assert_eq!(dest.len(), *num_elements);
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
            let (mut cmds, source0) = eval_operand(operand0, globals, tys);
            let (tmp, source1) = eval_operand(operand1, globals, tys);
            cmds.extend(tmp.into_iter());

            // FIXME: THIS DOES AN SREM
            for s in source0.iter().cloned() {
                cmds.push(mark_assertion_matches(true, s, ..=-1));
            }

            for s in source1.iter().cloned() {
                cmds.push(mark_assertion_matches(true, s, ..=-1));
            }

            let dest = ScoreHolder::from_local_name(
                dest.clone(),
                type_layout(&operand0.get_type(tys), tys).size(),
            );

            if let Type::VectorType {
                element_type,
                num_elements,
            } = &*operand0.get_type(tys)
            {
                if !matches!(&**element_type, Type::IntegerType { bits: 32 }) {
                    todo!("{:?}", element_type)
                }

                assert_eq!(source0.len(), *num_elements);
                assert_eq!(source1.len(), *num_elements);
                assert_eq!(dest.len(), *num_elements);
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
        }) if operand0.get_type(tys).as_ref() == &Type::IntegerType { bits: 64 } => {
            let is_eq = pred == &IntPredicate::EQ;

            // TODO: When operand1 is a constant, we can optimize the direct comparison into a `matches`
            let (mut cmds, op0) = eval_operand(operand0, globals, tys);
            let (tmp_cmds, op1) = eval_operand(operand1, globals, tys);
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
            let (mut cmds, target) = eval_operand(operand0, globals, tys);
            let (tmp_cmds, source) = eval_operand(operand1, globals, tys);
            cmds.extend(tmp_cmds);

            match &*operand0.get_type(tys) {
                Type::IntegerType { bits: 64 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 1).into_iter().next().unwrap();

                    let mut target = target.into_iter();
                    let target_lo = target.next().unwrap();
                    let target_hi = target.next().unwrap();

                    let mut source = source.into_iter();
                    let source_lo = source.next().unwrap();
                    let source_hi = source.next().unwrap();

                    match predicate {
                        IntPredicate::UGT => {
                            let hi_is_gt = get_unique_holder();
                            let hi_is_eq = get_unique_holder();
                            let lo_is_gt = get_unique_holder();

                            cmds.extend(compile_unsigned_cmp(target_hi.clone(), source_hi.clone(), hi_is_gt.clone(), cir::Relation::GreaterThan));
                            cmds.extend(compile_unsigned_cmp(target_lo, source_lo, lo_is_gt.clone(), cir::Relation::GreaterThan));
                            cmds.push(compile_signed_cmp(target_hi, source_hi, hi_is_eq.clone(), cir::Relation::Eq, true));

                            cmds.push(assign_lit(dest.clone(), 0));
                            
                            let mut hi_gt_check = Execute::new();
                            hi_gt_check.with_if(ExecuteCondition::Score {
                                target: hi_is_gt.into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((1..=1).into()),
                            });
                            hi_gt_check.with_run(assign_lit(dest.clone(), 1));
                            cmds.push(hi_gt_check.into());

                            let mut lo_gt_check = Execute::new();
                            lo_gt_check.with_if(ExecuteCondition::Score {
                                target: hi_is_eq.into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((1..=1).into()),
                            });
                            lo_gt_check.with_if(ExecuteCondition::Score {
                                target: lo_is_gt.into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((1..=1).into()),
                            });
                            lo_gt_check.with_run(assign_lit(dest, 1));
                            cmds.push(lo_gt_check.into());
                        }
                        IntPredicate::ULT => {
                            let hi_is_lt = get_unique_holder();
                            let hi_is_eq = get_unique_holder();
                            let lo_is_lt = get_unique_holder();

                            cmds.extend(compile_unsigned_cmp(target_hi.clone(), source_hi.clone(), hi_is_lt.clone(), cir::Relation::LessThan));
                            cmds.extend(compile_unsigned_cmp(target_lo, source_lo, lo_is_lt.clone(), cir::Relation::LessThan));
                            cmds.push(compile_signed_cmp(target_hi, source_hi, hi_is_eq.clone(), cir::Relation::Eq, true));

                            cmds.push(assign_lit(dest.clone(), 0));
                            
                            let mut hi_lt_check = Execute::new();
                            hi_lt_check.with_if(ExecuteCondition::Score {
                                target: hi_is_lt.into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((1..=1).into()),
                            });
                            hi_lt_check.with_run(assign_lit(dest.clone(), 1));
                            cmds.push(hi_lt_check.into());

                            let mut lo_lt_check = Execute::new();
                            lo_lt_check.with_if(ExecuteCondition::Score {
                                target: hi_is_eq.into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((1..=1).into()),
                            });
                            lo_lt_check.with_if(ExecuteCondition::Score {
                                target: lo_is_lt.into(),
                                target_obj: OBJECTIVE.into(),
                                kind: ExecuteCondKind::Matches((1..=1).into()),
                            });
                            lo_lt_check.with_run(assign_lit(dest, 1));
                            cmds.push(lo_lt_check.into());
                        }
                        p => todo!("{:?}", p),
                    }

                    cmds
                }
                Type::VectorType {
                    element_type,
                    num_elements,
                } if matches!(&**element_type, Type::IntegerType { bits: 32 }) => {
                    if *num_elements > 4 {
                        todo!()
                    }

                    let dest = ScoreHolder::from_local_name(dest.clone(), *num_elements)
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
                } if matches!(&**element_type, Type::IntegerType { bits: 8 }) => {
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
                ty => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                        .into_iter()
                        .next()
                        .unwrap();

                    if target.len() != 1 || source.len() != 1 {
                        if target.len() == 2 && source.len() == 2 {
                            cmds.extend(compile_normal_icmp(target[1].clone(), source[1].clone(),predicate, dest.clone()));

                            let conditionals = compile_normal_icmp(target[0].clone(), source[0].clone(), predicate, dest);

                            cmds.extend(
                                conditionals
                                .into_iter()
                                .map(|current_command| -> Command {
                                    let mut out_command = Execute::new();

                                    out_command.with_if(ExecuteCondition::Score {
                                        target: Target::Uuid(target[1].clone()),
                                        target_obj: OBJECTIVE.to_string(),
                                        kind: ExecuteCondKind::Relation {
                                            relation: cir::Relation::Eq,
                                            source: Target::Uuid(source[1].clone()),
                                            source_obj: OBJECTIVE.to_string()
                                        }
                                    });
                                    out_command.with_run::<cir::Command>(current_command.clone());

                                    out_command.into()
                                })
                            );
                        } else {
                            println!("Target len is {}, source len is {} and ty is {:?} and predicate is {:?}",target.len(),source.len(), ty, predicate);
                            todo!()
                        }
                    } else {
                        let target = target.into_iter().next().unwrap();
                        let source = source.into_iter().next().unwrap();

                        cmds.extend(compile_normal_icmp(target, source, predicate, dest));
                    }

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
            let to_type_size = type_layout(to_type, tys).size();

            let dst = ScoreHolder::from_local_name(dest.clone(), to_type_size);

            let mut cmds = Vec::new();

            for (value, block) in incoming_values {
                let block_idx = parent
                    .basic_blocks
                    .iter()
                    .position(|b| &b.name == block)
                    .unwrap() as i32;

                cmds.push(Command::Comment(format!(
                    "block {}\nvalue {:?}",
                    block, value
                )));

                let (tmp, val) = eval_operand(value, globals, tys);
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
        Instruction::Call(call) => return compile_call(call, globals, tys),
        Instruction::BitCast(BitCast {
            operand,
            dest,
            to_type,
            ..
        }) => {
            let (mut cmds, source) = eval_operand(operand, globals, tys);

            if source.len() != 1 {
                todo!("multiword source {:?}", source);
            }

            let source = source[0].clone();

            let dest = ScoreHolder::from_local_name(dest.clone(), type_layout(to_type, tys).size());

            if dest.len() != 1 {
                todo!("multiword dest {:?}", dest);
            }

            let dest = dest[0].clone();

            cmds.push(assign(dest, source));

            cmds
        }
        Instruction::Trunc(Trunc {
            operand,
            to_type,
            dest,
            ..
        }) if to_type.as_ref() == &Type::IntegerType { bits: 32 } => {
            if !matches!(&*operand.get_type(tys), Type::IntegerType { bits: 64 }) {
                todo!("{:?}", operand);
            }

            let (mut cmds, op) = eval_operand(operand, globals, tys);

            let dest = ScoreHolder::from_local_name(dest.clone(), 4)[0].clone();

            cmds.push(assign(dest, op[0].clone()));

            cmds
        }
        Instruction::Trunc(Trunc {
            operand,
            to_type,
            dest,
            ..
        }) => {
            let (mut cmds, op) = eval_operand(operand, globals, tys);

            let dest = ScoreHolder::from_local_name(dest.clone(), 1)
                .into_iter()
                .next()
                .unwrap();

            cmds.push(assign(dest.clone(), op[0].clone()));

            let bits = if let Type::IntegerType { bits } = &**to_type {
                *bits
            } else {
                todo!("{:?}", to_type)
            };

            if bits >= 31 {
                todo!()
            }

            // FIXME: Is this (and the other one) valid?
            cmds.push(make_op_lit(dest, "%=", 1 << bits));

            cmds
        }
        Instruction::ExtractValue(ExtractValue {
            aggregate,
            indices,
            dest,
            ..
        }) => {
            let (mut cmds, aggr) = eval_operand(aggregate, globals, tys);

            if indices.len() != 1 {
                todo!("{:?}", indices)
            }

            if let Type::StructType {
                element_types,
                is_packed,
            } = &*aggregate.get_type(tys)
            {
                let result_type = &element_types[indices[0] as usize];
                let size = type_layout(result_type, tys).size();

                let offset = offset_of(element_types, *is_packed, indices[0], tys);

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
            let aggr_ty = aggregate.get_type(tys);
            let aggr_layout = type_layout(&aggr_ty, tys);

            if indices.len() != 1 {
                todo!("indices {:?}", indices)
            }
            let index = indices[0];

            let (element_types, is_packed) = if let Type::StructType {
                element_types,
                is_packed,
            } = &*aggr_ty
            {
                (element_types, *is_packed)
            } else {
                todo!("{:?}", aggr_ty)
            };

            let (mut cmds, aggr) = eval_operand(aggregate, globals, tys);
            let (tmp, elem) = eval_operand(element, globals, tys);
            cmds.extend(tmp);

            let elem = elem[0].clone();

            let offset = offset_of(&element_types, is_packed, index, tys);

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

            if type_layout(&element.get_type(tys), tys).size() == 4 && offset % 4 == 0 {
                cmds.push(assign(dest[insert_idx].clone(), elem));
            } else if type_layout(&element.get_type(tys), tys).size() == 1 {
                cmds.push(mark_assertion_matches(true, dest[insert_idx].clone(), 0..=255));

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
            let (mut cmds, op) = eval_operand(operand, globals, tys);

            if matches!(&**to_type, Type::IntegerType { bits: 32 }) {
                let op = op.into_iter().next().unwrap();

                let dest = ScoreHolder::from_local_name(dest.clone(), 4)
                    .into_iter()
                    .next()
                    .unwrap();

                if matches!(&*operand.get_type(tys), Type::IntegerType { bits: 1 }) {
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
                    let (range, to_add, modu) =
                        if matches!(&*operand.get_type(tys), Type::IntegerType { bits: 8 }) {
                            (128..=255, -256, 256)
                        } else if matches!(&*operand.get_type(tys), Type::IntegerType { bits: 16 }) {
                            (32768..=65535, -65536, 65536)
                        } else {
                            todo!("{:?}", operand.get_type(tys));
                        };

                    cmds.push(assign(dest.clone(), op));
                    cmds.push(make_op_lit(dest.clone(), "%=", modu));
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
            } else if matches!(&**to_type, Type::IntegerType { bits: 64 }) {
                let dest = ScoreHolder::from_local_name(dest.clone(), 8);

                if matches!(&*operand.get_type(tys), Type::IntegerType { bits: 32 }) {
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
                } else if matches!(&*operand.get_type(tys), Type::IntegerType { bits: 8 }) {
                    let op = op.into_iter().next().unwrap();
                    cmds.push(assign(dest[0].clone(), op));
                    cmds.push(make_op_lit(dest[0].clone(), "%=", 256));
                    let mut exec = Execute::new();
                    exec.with_if(ExecuteCondition::Score {
                        target: dest[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        kind: ExecuteCondKind::Matches((128..=255).into()),
                    });
                    exec.with_run(ScoreAdd {
                        target: dest[0].clone().into(),
                        target_obj: OBJECTIVE.into(),
                        score: -256,
                    });
                    cmds.push(exec.into());
                    cmds.push(assign_lit(dest[1].clone(), 0)); // unconditionally initialize dest[1]
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
            let (mut cmds, op) = eval_operand(operand, globals, tys);

            let to_size = type_layout(to_type, tys).size();

            let dst = ScoreHolder::from_local_name(dest.clone(), to_size);

            if op.len() == 1 {
                cmds.push(assign(dst[0].clone(), op[0].clone()));

                match &*operand.get_type(tys) {
                    Type::IntegerType { bits } => {
                        if *bits < 32 {
                            cmds.push(make_op_lit(dst[0].clone(), "%=", 1 << bits));
                        }
                    }
                    Type::VectorType { element_type, num_elements: 4 } if matches!(&**element_type, Type::IntegerType { bits: 1 }) => {
                        let elem_ty = if let Type::VectorType { element_type, num_elements: 4 } = &**to_type {
                            element_type
                        } else {
                            panic!("{:?}", to_type)
                        };

                        if matches!(&**elem_ty, Type::IntegerType { bits: 32 }) {
                            let tmp = get_unique_holder();
                            cmds.push(assign(tmp.clone(), op[0].clone()));
                            for dst_word in dst {
                                cmds.push(assign(dst_word.clone(), tmp.clone()));
                                cmds.push(make_op_lit(dst_word, "%=", 2));
                                cmds.push(make_op_lit(tmp.clone(), "/=", 256));
                            }

                            return (cmds, None);
                        } else {
                            todo!("{:?}", elem_ty)
                        }
                    }
                    _ => todo!("{:?} -> {:?}", operand, to_type)
                }

                for dst in dst[1..].iter().cloned() {
                    cmds.push(assign_lit(dst, 0));
                }
            } else if op.len() == 2 {
                cmds.push(assign(dst[0].clone(), op[0].clone()));
                cmds.push(assign(dst[1].clone(), op[1].clone()));

                if let Type::IntegerType { bits } = &*operand.get_type(tys) {
                    if *bits < 64 {
                        assert!(*bits > 32);
                        cmds.push(make_op_lit(dst[1].clone(), "%=", 1 << (bits - 32)));
                    }
                } else {
                    todo!("{:?}", operand)
                };

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
            assert_eq!(operand0.get_type(tys), operand1.get_type(tys));

            let (mut cmds, op0) = eval_operand(operand0, globals, tys);

            let (tmp, op1) = eval_operand(operand1, globals, tys);

            cmds.extend(tmp);

            match &*operand0.get_type(tys) {
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
                _ => {
                    compile_bitwise_word(operand0, operand1, dest.clone(), BitOp::Or, globals, tys)
                }
            }
        }
        Instruction::And(And {
            operand0,
            operand1,
            dest,
            ..
        }) => {
            assert_eq!(operand0.get_type(tys), operand1.get_type(tys));

            let (mut cmds, op0) = eval_operand(operand0, globals, tys);
            let (tmp, op1) = eval_operand(operand1, globals, tys);
            cmds.extend(tmp);

            let layout = type_layout(&operand0.get_type(tys), tys);

            match &*operand0.get_type(tys) {
                Type::IntegerType { bits: 1 } => {
                    let dest = ScoreHolder::from_local_name(dest.clone(), layout.size());
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
                _ => {
                    compile_bitwise_word(operand0, operand1, dest.clone(), BitOp::And, globals, tys)
                }
            }
        }
        Instruction::Xor(xor) => compile_xor(xor, globals, tys),
        Instruction::Shl(shl) => compile_shl(shl, globals, tys),
        Instruction::LShr(lshr) => compile_lshr(lshr, globals, tys),
        Instruction::AShr(ashr) => compile_ashr(ashr, globals, tys),
        Instruction::PtrToInt(PtrToInt {
            operand,
            to_type,
            dest,
            ..
        }) if to_type.as_ref() == &Type::IntegerType { bits: 32 } => {
            if !matches!(&*operand.get_type(tys), Type::PointerType{ .. }) {
                todo!("{:?}", operand)
            }

            let (mut cmds, op) = eval_operand(operand, globals, tys);
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
            assert_eq!(operand.get_type(tys).as_ref(), &Type::IntegerType { bits: 32 });

            if !matches!(&*to_type.get_type(tys), Type::PointerType{ .. }) {
                todo!("{:?}", operand)
            }

            let (mut cmds, op) = eval_operand(operand, globals, tys);
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
            let (mut cmds, op0) = eval_operand(operand0, globals, tys);
            let (tmp, op1) = eval_operand(operand1, globals, tys);
            cmds.extend(tmp);

            let op0_ty = operand0.get_type(tys);
            let element_type = if let Type::VectorType { element_type, .. } = &*op0_ty {
                element_type
            } else {
                unreachable!()
            };

            let op0_len = if let Type::VectorType { num_elements, .. } = &*mask.get_type(tys) {
                *num_elements
            } else {
                unreachable!()
            };

            let mask_vals = match &**mask {
                Constant::AggregateZero(t) => {
                    if let Type::VectorType { element_type: _, num_elements } = &**t {
                        vec![ConstantRef::new(Constant::Int { bits: 32, value: 0 }); *num_elements]
                    } else {
                        unreachable!()
                    }
                }
                Constant::Vector(v) => v.clone(),
                _ => unreachable!("mask: {:?}", mask),
            };

            let dest_type = Type::VectorType {
                element_type: element_type.clone(),
                num_elements: mask_vals.len(),
            };

            let dest = ScoreHolder::from_local_name(dest.clone(), type_layout(&dest_type, tys).size());

            match &**element_type {
                Type::IntegerType { bits: 32 } => {
                    for (dest, mask_idx) in dest.into_iter().zip(mask_vals.into_iter()) {
                        match &*mask_idx {
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
                        match &*mask_idx {
                            Constant::Undef(_) => {}
                            Constant::Int { bits: 32, value } => {
                                let value = *value as usize;

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
            let element_type = if let Type::VectorType { element_type, .. } = &*vector.get_type(tys) {
                (*element_type).clone()
            } else {
                unreachable!()
            };

            let dest =
                ScoreHolder::from_local_name(dest.clone(), type_layout(&element_type, tys).size());

            let (mut cmds, vec) = eval_operand(vector, globals, tys);

            match eval_maybe_const(index, globals, tys) {
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
            let element_type = if let Type::VectorType { element_type, .. } = &*vector.get_type(tys) {
                (*element_type).clone()
            } else {
                unreachable!()
            };

            if !matches!(&*element_type, Type::IntegerType { bits: 32 }) {
                todo!("{:?}", vector)
            }

            let dest =
                ScoreHolder::from_local_name(dest.clone(), type_layout(&vector.get_type(tys), tys).size());

            let (mut cmds, vec) = eval_operand(vector, globals, tys);

            let (tmp, elem) = eval_operand(element, globals, tys);
            cmds.extend(tmp);

            assert_eq!(elem.len(), 1);
            let elem = elem.into_iter().next().unwrap();

            match eval_maybe_const(index, globals, tys) {
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
    globals: &GlobalVarList,
    tys: &Types,
) -> MaybeConst {
    match con {
        Constant::GlobalReference { name, .. } => {
            let addr = globals
                .get(name)
                .unwrap_or_else(|| panic!("failed to get {:?}", name))
                .0;

            if addr == u32::MAX {
                let mut name = if let Name::Name(name) = name {
                    (**name).clone()
                } else {
                    todo!()
                };
                name.push_str("%%fixup_func_ref");
                // FIXME: ew ew ew ew
                MaybeConst::NonConst(vec![], vec![ScoreHolder::new_unchecked(name)])
            } else {
                MaybeConst::Const(addr as i32)
            }
        }
        Constant::PtrToInt(tmp) => {
            let llvm_ir::constant::PtrToInt {
                operand,
                ..
            } = &*tmp;

            if let Constant::GlobalReference { name, .. } = &**operand {
                let addr = globals
                    .get(name)
                    .unwrap_or_else(|| panic!("failed to get {:?}", name))
                    .0;
                MaybeConst::Const(addr as i32)
            } else {
                todo!("{:?}", operand)
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
        Constant::Struct { values, is_packed: _, name: _ } => {
            if values.iter().all(|v| matches!(&*v.get_type(tys), Type::IntegerType { bits: 32 })) {
                let (cmds, words) = values
                    .iter()
                    .map(|v| eval_constant(v, globals, tys).force_eval())
                    .map(|(c, w)| {
                        assert_eq!(c.len(), 1);
                        assert_eq!(w.len(), 1);
                        (c.into_iter().next().unwrap(), w.into_iter().next().unwrap())
                    })
                    .unzip();

                MaybeConst::NonConst(cmds, words)
            } else {
                todo!("{:?}", values);
            }
        }
        Constant::BitCast(bitcast) => eval_constant(&bitcast.operand, globals, tys),
        Constant::Undef(ty) => {
            // TODO: This can literally be *anything* you want it to be

            let len = type_layout(ty, tys).size();

            let num = get_unique_num();

            let (cmds, holders) = (0..((len + 3) / 4))
                .map(|idx| {
                    let holder = ScoreHolder::new(format!("%temp{}%{}", num, idx)).unwrap();
                    (assign_lit(holder.clone(), 0), holder)
                })
                .unzip();

            MaybeConst::NonConst(cmds, holders)
        }
        Constant::GetElementPtr(g) => MaybeConst::Const(getelementptr_const(&g, globals, tys) as i32),
        Constant::Null(_) => MaybeConst::Const(0),
        Constant::AggregateZero(t) => {
            if let Type::VectorType { element_type, num_elements } = &**t {
                let size = type_layout(&element_type, tys).size() * num_elements;
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
            } else {
                todo!("{:?}", t)
            }
        }
        Constant::Vector(elems) => {
            // TODO: This is ugly, please fix all of this

            let as_8 = elems
                .iter()
                .map(|e| {
                    if e.get_type(tys) == tys.i8() {
                        if let Constant::Int { bits: 8, value } = &**e {
                            Some(*value as u8)
                        } else {
                            todo!()
                        }
                    } else {
                        None
                    }
                })
                .collect::<Option<Vec<u8>>>();

            let as_32 = elems
                .iter()
                .map(|e| {
                    if e.get_type(tys) == tys.i32() {
                        if let MaybeConst::Const(c) = eval_constant(e, globals, tys) {
                            Some(c)
                        } else {
                            todo!("{:?}", e)
                        }
                    } else {
                        None
                    }
                })
                .collect::<Option<Vec<i32>>>();

            let as_64 = elems
                .iter()
                .map(|e| {
                    if e.get_type(tys) == tys.i64() {
                        if let Constant::Int { bits: 64, value } = &**e {
                            Some(*value)
                        } else {
                            todo!()
                        }
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
            } = &*icmp;
            if let MaybeConst::Const(op0) = eval_constant(&operand0, globals, tys) {
                if let MaybeConst::Const(op1) = eval_constant(&operand1, globals, tys) {
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
            } = &*s;
            let condition = match &**condition {
                Constant::ICmp(i) => {
                    let ICmpConst {
                        predicate,
                        operand0,
                        operand1,
                    } = &*i;
                    if let Constant::BitCast(BitCastConst { operand, .. }) = &**operand0 {
                        if let Constant::GlobalReference { name, .. } = &**operand {
                            let value = globals.get(name).unwrap().0;
                            if let Constant::Null(_) = &**operand1 {
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
                            todo!("{:?}", operand)
                        }
                    } else {
                        todo!("{:?}", operand0)
                    }
                }
                _ => todo!("{:?}", condition),
            };
            if condition {
                eval_constant(&true_value, globals, tys)
            } else {
                eval_constant(&false_value, globals, tys)
            }
        }
        _ => todo!("evaluate constant {:?}", con),
    }
}

pub fn eval_maybe_const(
    op: &Operand,
    globals: &GlobalVarList,
    tys: &Types,
) -> MaybeConst {
    match op {
        Operand::LocalOperand { name, ty } => {
            let len = type_layout(ty, tys).size();

            let holders = ScoreHolder::from_local_name(name.clone(), len);

            MaybeConst::NonConst(Vec::new(), holders)
        }
        Operand::ConstantOperand(con) => eval_constant(con, globals, tys),
        _ => todo!("operand {:?}", op),
    }
}

pub fn eval_operand(
    op: &Operand,
    globals: &GlobalVarList,
    tys: &Types,
) -> (Vec<Command>, Vec<ScoreHolder>) {
    eval_maybe_const(op, globals, tys).force_eval()
}

lazy_static! {
    pub static ref TEMP_CNT: Mutex<u32> = Mutex::new(0);
    pub static ref FREE_PTR: Mutex<u32> = Mutex::new(4);
}

struct StaticAllocator(u32);

impl StaticAllocator {
    pub fn reserve(&mut self, mut amount: u32) -> u32 {
        if amount % 4 != 0 {
            amount += 4 - (amount % 4);
        }

        let result = self.0;
        self.0 += amount;
        result
    }
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
        let tys = Types::blank_for_testing();

        let ty = Type::VectorType {
            element_type: tys.get_for_type(&Type::IntegerType { bits: 8 }),
            num_elements: 4,
        };

        assert_eq!(type_layout(&ty, &tys), Layout::from_size_align(4, 4).unwrap());
    }

    #[test]
    fn struct_offset() {
        let tys = Types::blank_for_testing();

        // TODO: More comprehensive test
        let element_types = vec![
            tys.get_for_type(&Type::IntegerType { bits: 32 }),
            tys.get_for_type(&Type::IntegerType { bits: 32 }),
        ];

        assert_eq!(offset_of(&element_types, false, 0, &tys), 0);
        assert_eq!(offset_of(&element_types, false, 1, &tys), 4);
        assert_eq!(offset_of(&element_types, true, 0, &tys), 0);
        assert_eq!(offset_of(&element_types, true, 1, &tys), 4);
    }
}
