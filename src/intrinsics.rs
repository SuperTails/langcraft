use lazy_static::lazy_static;
use crate::cir::{Function, FunctionId};

static INTRINSIC_STRS: &[(&str, &str)] = &[
    ("memcpy", include_str!("intrinsic/memcpy.mcfunction")),
    ("or", include_str!("intrinsic/or.mcfunction")),
    ("or_inner", include_str!("intrinsic/or_inner.mcfunction")),
    ("pop_and_branch", include_str!("intrinsic/pop_and_branch.mcfunction")),
    ("setptr", include_str!("intrinsic/setptr.mcfunction")),
    ("shift_from_ptr", include_str!("intrinsic/shift_from_ptr.mcfunction")),
    ("shift_from_ptr_inner", include_str!("intrinsic/shift_from_ptr_inner.mcfunction")),
    ("xor", include_str!("intrinsic/xor.mcfunction")),
    ("xor_inner", include_str!("intrinsic/xor_inner.mcfunction")),
];

lazy_static! {
    pub static ref INTRINSICS: Vec<Function> = {
        INTRINSIC_STRS
            .iter()
            .map(|(name, body)| {
                let id = FunctionId::new(format!("intrinsic:{}", name));
                let cmds = body.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.parse().unwrap())
                    .collect();


                Function { id, cmds }
            })
            .collect()
    };
}