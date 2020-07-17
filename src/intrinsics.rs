use crate::cir::{Function, FunctionId};
use lazy_static::lazy_static;

static INTRINSIC_STRS: &[(&str, &str)] = &[
    ("intrinsic:lshr", include_str!("intrinsic/lshr.mcfunction")),
    (
        "intrinsic/lshr:getshift",
        include_str!("intrinsic/lshr/getshift.mcfunction"),
    ),
    (
        "intrinsic/lshr:inner",
        include_str!("intrinsic/lshr/inner.mcfunction"),
    ),
    (
        "intrinsic:memcpy",
        include_str!("intrinsic/memcpy.mcfunction"),
    ),
    (
        "intrinsic:memcpy_inner",
        include_str!("intrinsic/memcpy_inner.mcfunction"),
    ),
    ("intrinsic:or", include_str!("intrinsic/or.mcfunction")),
    (
        "intrinsic:or_inner",
        include_str!("intrinsic/or_inner.mcfunction"),
    ),
    (
        "intrinsic:pop_and_branch",
        include_str!("intrinsic/pop_and_branch.mcfunction"),
    ),
    (
        "intrinsic:setptr",
        include_str!("intrinsic/setptr.mcfunction"),
    ),
    (
        "intrinsic:shift_from_ptr",
        include_str!("intrinsic/shift_from_ptr.mcfunction"),
    ),
    (
        "intrinsic:shift_from_ptr_inner",
        include_str!("intrinsic/shift_from_ptr_inner.mcfunction"),
    ),
    ("intrinsic:xor", include_str!("intrinsic/xor.mcfunction")),
    (
        "intrinsic:xor_inner",
        include_str!("intrinsic/xor_inner.mcfunction"),
    ),
    ("intrinsic:and", include_str!("intrinsic/and.mcfunction")),
    (
        "intrinsic:and_inner",
        include_str!("intrinsic/and_inner.mcfunction"),
    ),
    (
        "intrinsic:store_byte",
        include_str!("intrinsic/store_byte.mcfunction"),
    ),
    (
        "intrinsic:store_halfword",
        include_str!("intrinsic/store_halfword.mcfunction"),
    ),
    (
        "intrinsic:load_byte",
        include_str!("intrinsic/load_byte.mcfunction"),
    ),
    (
        "intrinsic:load_halfword",
        include_str!("intrinsic/load_halfword.mcfunction"),
    ),
    (
        "intrinsic:bcmp",
        include_str!("intrinsic/bcmp.mcfunction"),
    ),
    (
        "intrinsic:bcmp_inner",
        include_str!("intrinsic/bcmp_inner.mcfunction"),
    ),
    (
        "intrinsic:memset",
        include_str!("intrinsic/memset.mcfunction")
    ),
    (
        "intrinsic:memset_inner",
        include_str!("intrinsic/memset_inner.mcfunction")
    ),

];

lazy_static! {
    pub static ref INTRINSICS: Vec<Function> = {
        INTRINSIC_STRS
            .iter()
            .map(|(name, body)| {
                let id = FunctionId::new(name.to_owned());
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
    use super::*;
    use crate::cir;
    use crate::compile_ir::{self, param, return_holder};
    use crate::Interpreter;
    use std::convert::TryInto;

    fn test_bcmp(mem_a: &[u8], mem_b: &[u8], start: usize, len: usize) {
        let mut interp = Interpreter::new(vec![
            get_by_name("intrinsic:lshr").clone(),
            get_by_name("intrinsic:load_byte").clone(),
            get_by_name("intrinsic:bcmp_inner").clone(),
            get_by_name("intrinsic:bcmp").clone(),
        ], "");

        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp
            .rust_scores
            .insert(param(0, 0), start as i32);
        interp
            .rust_scores
            .insert(param(1, 0), 0x100 + start as i32);
        interp
            .rust_scores
            .insert(param(2, 0), len as i32);
        
        for (idx, byte) in mem_a.iter().copied().enumerate() {
            interp.set_byte(byte, idx);
        }
        for (idx, byte) in mem_b.iter().copied().enumerate() {
            interp.set_byte(byte, idx + 0x100);
        }

        let expected = (mem_a[start..][..len] != mem_b[start..][..len]) as i32;

        interp.run_to_end().unwrap();

        let actual = interp.get_rust_score(&return_holder(0)).unwrap();
        
        assert_eq!(expected, actual);
    }

    #[test]
    fn memcpy() {
        let mut interp = Interpreter::new(vec![
            get_by_name("intrinsic:lshr").clone(),
            get_by_name("intrinsic:load_byte").clone(),
            get_by_name("intrinsic:store_byte").clone(),
            get_by_name("intrinsic:and").clone(),
            get_by_name("intrinsic:memcpy").clone(),
        ], "");

        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%1".into()).unwrap(), 1);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%65536".into()).unwrap(), 65536);
        interp.rust_scores.insert(
            cir::ScoreHolder::new("%%16777216".into()).unwrap(),
            16777216,
        );

        for idx in 0..30 {
            interp.set_byte(0xAA, idx);
        }

        // Setup source
        let data = [1, 2, 3, 4, 5, 6, 7];
        for (idx, byte) in data.iter().copied().enumerate() {
            interp.set_byte(byte, 5 + idx);
        }


        interp.rust_scores.insert(param(0, 0), 15);
        interp.rust_scores.insert(param(1, 0), 5);
        interp.rust_scores.insert(param(2, 0), data.len() as i32);
        interp.run_to_end().unwrap();

        for (idx, expected) in data.iter().copied().enumerate() {
            // Make sure nothing changed
            let actual = interp.get_byte(5 + idx);
            assert_eq!(expected, actual);
        }
        for (idx, expected) in data.iter().copied().enumerate() {
            let actual = interp.get_byte(15 + idx);
            assert_eq!(expected, actual);
        }
        // Make sure nothing changed
        assert_eq!(0xAA, interp.get_byte(14));
        assert_eq!(0xAA, interp.get_byte(15 + 7));
    }

    #[test]
    fn bcmp() {
        test_bcmp(
            &[12, 34, 9, 8, 7, 6, 5, 99, 0],
            &[43, 21, 9, 8, 7, 6, 5, 88, 5],
            2,
            5
        );

        test_bcmp(
            &[12, 34, 9, 8, 7, 6, 5, 99, 0],
            &[43, 21, 9, 8, 7, 6, 5, 88, 5],
            2,
            6
        );
    }

    #[test]
    fn load_byte() {
        let mut interp = Interpreter::new(vec![
            get_by_name("intrinsic:lshr").clone(),
            get_by_name("intrinsic:load_byte").clone()
        ], "");
        let word = [0x12, 0xEA, 0x56, 0x78];
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp.memory[1] = i32::from_le_bytes(word);

        for (pt, expected) in word.iter().copied().enumerate() {
            interp.call_stack = vec![(1, 0)];
            interp.rust_scores.insert(compile_ir::ptr(), pt as i32 + 4);
            interp.run_to_end().unwrap();
            let result = *interp
                .rust_scores
                .get(&param(0, 0))
                .unwrap();
            assert_eq!(result, expected as i32);
        }
       
    }

    #[test]
    fn load_halfword() {
        let mut interp = Interpreter::new(vec![
            get_by_name("intrinsic:lshr").clone(),
            get_by_name("intrinsic:load_halfword").clone()
        ], "");
        let word = [0x12, 0xEA, 0x56, 0x78];
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%2".into()).unwrap(), 2);
        interp.memory[1] = i32::from_le_bytes(word);

        for (pt, expected) in word.chunks_exact(2).enumerate() {
            let expected = u16::from_le_bytes(expected.try_into().unwrap()) as i32;

            interp.call_stack = vec![(1, 0)];
            interp.rust_scores.insert(compile_ir::ptr(), 2 * pt as i32 + 4);
            interp.run_to_end().unwrap();
            let result = *interp
                .rust_scores
                .get(&param(0, 0))
                .unwrap();
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn store_halfword() {
        let mut interp = Interpreter::new_raw(
            vec![
                get_by_name("intrinsic:setptr").clone(),
                get_by_name("intrinsic:and").clone(),
                get_by_name("intrinsic:and_inner").clone(),
                get_by_name("intrinsic:store_halfword").clone(),
            ],
            "",
        );
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%1".into()).unwrap(), 1);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%2".into()).unwrap(), 2);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%65536".into()).unwrap(), 65536);
        interp.rust_scores.insert(
            cir::ScoreHolder::new("%%16777216".into()).unwrap(),
            16777216,
        );

        let bytes = [0x12, 0xEA, 0x34, 0xBC];

        let mut expected = 0;
        for (idx, halfword) in bytes.chunks_exact(2).enumerate() {
            let halfword = u16::from_le_bytes(halfword.try_into().unwrap()) as i32;

            interp.call_stack = vec![(3, 0)];
            interp.rust_scores.insert(compile_ir::ptr(), 8 + 2 * idx as i32);
            interp.rust_scores.insert(param(2, 0), halfword);
            interp.run_to_end().unwrap();

            expected += (bytes[idx * 2] as i32) << (8 * (2 * idx));
            expected += (bytes[idx * 2 + 1] as i32) << (8 * (2 * idx + 1));

            let actual = interp.memory[2];
            if expected != actual {
                println!("Expected: {:>11} ({:#010X})", expected, expected);
                println!("Actual:   {:>11} ({:#010X})", actual, actual);
                panic!();
            }
        }
        
    }

    #[test]
    fn store_byte() {
        let mut interp = Interpreter::new_raw(
            vec![
                get_by_name("intrinsic:setptr").clone(),
                get_by_name("intrinsic:and").clone(),
                get_by_name("intrinsic:and_inner").clone(),
                get_by_name("intrinsic:store_byte").clone(),
            ],
            "",
        );
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%1".into()).unwrap(), 1);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%65536".into()).unwrap(), 65536);
        interp.rust_scores.insert(
            cir::ScoreHolder::new("%%16777216".into()).unwrap(),
            16777216,
        );

        let bytes = [0x12, 0xEA, 0x34, 0xBC];

        let mut expected = 0;
        for (idx, byte) in bytes.iter().copied().enumerate() {
            interp.call_stack = vec![(3, 0)];
            interp.rust_scores.insert(compile_ir::ptr(), 8 + idx as i32);
            interp.rust_scores.insert(param(2, 0), byte as i32);
            interp.run_to_end().unwrap();
            expected += (bytes[idx] as i32) << (8 * idx);

            let actual = interp.memory[2];
            if expected != actual {
                println!("Expected: {:>11} ({:#010X})", expected, expected);
                println!("Actual:   {:>11} ({:#010X})", actual, actual);
                panic!();
            }
        }
    }

    fn test_lshr(a: i32, shift: i32) {
        let expected = (a as u32 >> shift) as i32;
        let mut interp = Interpreter::new_raw(
            vec![
                get_by_name("intrinsic/lshr:getshift").clone(),
                get_by_name("intrinsic/lshr:inner").clone(),
                get_by_name("intrinsic:lshr").clone(),
            ],
            "",
        );
        interp.rust_scores.insert(param(0, 0), a);
        interp.rust_scores.insert(param(1, 0), shift);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
        interp.run_to_end().unwrap();
        let actual = *interp.rust_scores.get(&param(0, 0)).unwrap();

        if expected != actual {
            println!("Shift: {}", shift);
            println!("Input:    {:>10} ({:#010X})", a, a);
            println!("Expected: {:>10} ({:#010X})", expected, expected);
            println!("Actual:   {:>10} ({:#010X})", actual, actual);
            panic!();
        }
    }

    #[test]
    fn memset() {
        todo!("THIS TEST")
    }

    #[test]
    fn lshr_i32_min() {
        for shift in 0..32 {
            test_lshr(i32::MIN, shift);
        }
    }

    #[test]
    fn lshr_i32_max() {
        for shift in 0..32 {
            test_lshr(i32::MAX, shift);
        }
    }

    #[test]
    fn lshr_zero() {
        for shift in 0..32 {
            test_lshr(0, shift);
        }
    }

    #[test]
    fn lshr_neg_one() {
        for shift in 0..32 {
            test_lshr(-1, shift);
        }
    }

    #[test]
    fn lshr_other_positive() {
        for shift in 0..32 {
            test_lshr(1234567890, shift)
        }
    }

    #[test]
    fn lshr_other_negative() {
        for shift in 0..32 {
            test_lshr(-1234567890, shift)
        }
    }

    fn get_by_name(name: &str) -> &'static Function {
        INTRINSICS
            .iter()
            .find(|f| f.id == FunctionId::new(name))
            .unwrap_or_else(|| panic!("Could not find {:?}", name))
    }

    fn test_shift_from_ptr(a: i32, ptr: i32) {
        let expected = (a as u32 >> (8 * (ptr % 4))) as i32;
        let mut interp = Interpreter::new_raw(
            vec![
                get_by_name("intrinsic:shift_from_ptr_inner").clone(),
                get_by_name("intrinsic:shift_from_ptr").clone(),
            ],
            "",
        );
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%FOUR".into()).unwrap(), 4);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%256".into()).unwrap(), 256);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);
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
        let mut interp = Interpreter::new_raw(
            vec![
                get_by_name(&format!("{}_inner", name)).clone(),
                get_by_name(name).clone(),
            ],
            "",
        );
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
