use crate::cir::{Function, FunctionId};
use lazy_static::lazy_static;

static INTRINSIC_STRS: &[(&str, &str)] = &[
    ("memcpy", include_str!("intrinsic/memcpy.mcfunction")),
    (
        "memcpy_inner",
        include_str!("intrinsic/memcpy_inner.mcfunction"),
    ),
    ("or", include_str!("intrinsic/or.mcfunction")),
    ("or_inner", include_str!("intrinsic/or_inner.mcfunction")),
    (
        "pop_and_branch",
        include_str!("intrinsic/pop_and_branch.mcfunction"),
    ),
    ("setptr", include_str!("intrinsic/setptr.mcfunction")),
    (
        "shift_from_ptr",
        include_str!("intrinsic/shift_from_ptr.mcfunction"),
    ),
    (
        "shift_from_ptr_inner",
        include_str!("intrinsic/shift_from_ptr_inner.mcfunction"),
    ),
    ("xor", include_str!("intrinsic/xor.mcfunction")),
    ("xor_inner", include_str!("intrinsic/xor_inner.mcfunction")),
    ("and", include_str!("intrinsic/and.mcfunction")),
    ("and_inner", include_str!("intrinsic/and_inner.mcfunction")),
];

lazy_static! {
    pub static ref INTRINSICS: Vec<Function> = {
        INTRINSIC_STRS
            .iter()
            .map(|(name, body)| {
                let id = FunctionId::new(format!("intrinsic:{}", name));
                let cmds = body
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.parse().unwrap())
                    .collect();

                Function { id, cmds }
            })
            .collect()
    };
}

#[cfg(test)]
mod test {
    use crate::Interpreter;
    use crate::cir;
    use crate::compile_ir::{self, param, return_holder};
    use super::*;

    fn get_by_name(name: &str) -> &'static Function {
        INTRINSICS.iter().find(|f| f.id == FunctionId::new(name)).unwrap_or_else(|| panic!("Could not find {:?}", name))
    }

    fn test_shift_from_ptr(a: i32, ptr: i32) {
        let expected = (a as u32 >> (8 * (ptr % 4))) as i32;
        let mut interp = Interpreter::new_raw(vec![get_by_name("intrinsic:shift_from_ptr_inner").clone(), get_by_name("intrinsic:shift_from_ptr").clone()], "");
        interp.rust_scores.insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp.rust_scores.insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp.rust_scores.insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp.rust_scores.insert(param(0, 0), a as i32);
        interp.rust_scores.insert(compile_ir::ptr(), ptr);
        interp.run_to_end().unwrap();
        let result = *interp.rust_scores.get(&param(0, 0)).unwrap();

        if result != expected {
            println!("=========================");
            println!("Input:    {:>10} ({:#010X})", a, a);
            println!("Expected: {:>10} ({:#010X})", expected, expected);
            println!("Actual:   {:>10} ({:#010X})", result, result);
            println!("%ptr: {}", ptr);
            panic!();
        }
    }

    #[test]
    fn shift_from_ptr_zero() {
        for i in 0..4 {
            println!("--- %ptr {} ---", i);
            test_shift_from_ptr(0, i);
        }
    }

    #[test]
    fn shift_from_ptr_neg1() {
        for i in 0..4 {
            println!("--- %ptr {} ---", i);
            test_shift_from_ptr(-1, i);
        }
    }

    #[test]
    fn shift_from_ptr_i32_min() {
        for i in 0..4 {
            println!("--- %ptr {} ---", i);
            test_shift_from_ptr(i32::MIN, i);
        }
    }

    #[test]
    fn shift_from_ptr_other() {
        for i in 0..4 {
            println!("--- %ptr {} ---", i);
            test_shift_from_ptr(1234567890, i);
        }
    }

    fn call_bitwise_intrin(a: i32, b: i32, expected: i32, name: &str) {
        let mut interp = Interpreter::new_raw(vec![get_by_name(&format!("{}_inner", name)).clone(), get_by_name(name).clone()], "");
        interp.rust_scores.insert(param(0, 0), a);
        interp.rust_scores.insert(param(1, 0), b);
        interp.run_to_end().unwrap();
        let result = *interp.rust_scores.get(&return_holder(0)).unwrap();
        assert_eq!(result, expected);
        assert_eq!(interp.rust_scores.len(), 3);
    }

    fn test_and(a: i32, b: i32) {
        call_bitwise_intrin(a, b, a & b, "intrinsic:and")
    }

    fn test_or(a: i32, b: i32) {
        call_bitwise_intrin(a, b, a | b, "intrinsic:or")
    }

    fn test_xor(a: i32, b: i32) {
        call_bitwise_intrin(a, b, a ^ b, "intrinsic:xor")
    }

    #[test]
    fn xor() {
        test_xor(0, 0);
        test_xor(0xFFFF_0000_u32 as i32, 0x0000FFFF);
        test_xor(0x00AA_0000, 0x00FF0000);
        test_xor(-52, -123561);
        test_xor(-23566, 1352);
    }

    #[test]
    fn or() {
        test_or(0, 0);
        test_or(0xFFFF_0000_u32 as i32, 0x0000FFFF);
        test_or(0x00AA_0000, 0x00FF0000);
        test_or(-52, -123561);
        test_or(-23566, 1352);
    }

    #[test]
    fn and() {
        test_and(0, 0);
        test_and(0xFFFF_0000_u32 as i32, 0x0000FFFF);
        test_and(0x00AA_0000, 0x00FF0000);
        test_and(-52, -123561);
        test_and(-23566, 1352);
    }
}