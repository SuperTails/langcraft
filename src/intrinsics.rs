use crate::cir::{Function, FunctionId, Command, Execute, FuncCall};
use lazy_static::lazy_static;
use std::collections::{HashSet, HashMap};

static INTRINSIC_STRS: &[(&str, &str)] = &[
    ("intrinsic:lshr", include_str!("intrinsic/lshr.mcfunction")),
    (
        "intrinsic:lshr/getshift",
        include_str!("intrinsic/lshr/getshift.mcfunction"),
    ),
    (
        "intrinsic:lshr/inner",
        include_str!("intrinsic/lshr/inner.mcfunction"),
    ),
    (
        "intrinsic:memcpy",
        include_str!("intrinsic/memcpy.mcfunction"),
    ),
    (
        "intrinsic:memcpy/next_byte",
        include_str!("intrinsic/memcpy/next_byte.mcfunction"),
    ),
    (
        "intrinsic:memcpy/inner",
        include_str!("intrinsic/memcpy/inner.mcfunction"),
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
        "intrinsic:store_halfword_unaligned",
        include_str!("intrinsic/store_halfword_unaligned.mcfunction"),
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
        "intrinsic:load_halfword_unaligned",
        include_str!("intrinsic/load_halfword_unaligned.mcfunction"),
    ),
    ("intrinsic:bcmp", include_str!("intrinsic/bcmp.mcfunction")),
    (
        "intrinsic:bcmp_inner",
        include_str!("intrinsic/bcmp_inner.mcfunction"),
    ),
    (
        "intrinsic:memset",
        include_str!("intrinsic/memset.mcfunction"),
    ),
    (
        "intrinsic:memset_inner",
        include_str!("intrinsic/memset_inner.mcfunction"),
    ),
    (
        "intrinsic:store_word_unaligned",
        include_str!("intrinsic/store_word_unaligned.mcfunction"),
    ),
    (
        "intrinsic:load_word_unaligned",
        include_str!("intrinsic/load_word_unaligned.mcfunction"),
    ),
    (
        "intrinsic:mul_32_to_64",
        include_str!("intrinsic/mul_32_to_64.mcfunction"),
    ),
    (
        "intrinsic:llvm_fshr_i32",
        include_str!("intrinsic/llvm_fshr_i32.mcfunction"),
    ),
    (
        "intrinsic:llvm_ctlz_i32",
        include_str!("intrinsic/llvm_ctlz_i32.mcfunction"),
    ),
    (
        "intrinsic:llvm_ctlz_i32_inner",
        include_str!("intrinsic/llvm_ctlz_i32_inner.mcfunction"),
    ),
    ("intrinsic:shl", include_str!("intrinsic/shl.mcfunction")),
];

lazy_static! {
    pub static ref INTRINSICS: Vec<Function> = {
        INTRINSIC_STRS
            .iter()
            .map(|(name, body)| Function::from_str(FunctionId::new(name.to_owned()), body).unwrap())
            .collect()
    };

    pub static ref INTRINSIC_COUNTS: HashMap<FunctionId, Option<usize>> = {
        let mut vals = HashMap::new();
        for idx in 0..INTRINSICS.len() {
            estimate_intr(idx, &mut vals);
        }
        vals
    };
}

fn estimate_cmd(cmd: &Command, vals: &mut HashMap<FunctionId, Option<usize>>, visited: &mut HashSet<&FunctionId>) -> Option<usize> {
    match cmd {
        Command::Execute(Execute { run: Some(run), subcommands: _ }) => Some(1 + estimate_cmd(run, vals, visited)?),
        Command::FuncCall(FuncCall { id }) => {
            let idx = INTRINSICS.iter().enumerate().find(|(_, f)| &f.id == id).unwrap().0;
            estimate_intr_inner(idx, vals, visited)
        },
        _ => Some(1),
    }
}

fn estimate_intr(idx: usize, vals: &mut HashMap<FunctionId, Option<usize>>) -> Option<usize> {
    let mut visited = HashSet::new();
    estimate_intr_inner(idx, vals, &mut visited)
}

fn estimate_intr_inner(idx: usize, vals: &mut HashMap<FunctionId, Option<usize>>, visited: &mut HashSet<&FunctionId>) -> Option<usize> {
    if let Some(result) = vals.get(&INTRINSICS[idx].id) {
        *result
    } else if visited.contains(&INTRINSICS[idx].id) {
        vals.insert(INTRINSICS[idx].id.clone(), None);
        None
    } else {
        visited.insert(&INTRINSICS[idx].id);

        let count = || -> Option<usize> {
            let mut result = 0;
            for cmd in INTRINSICS[idx].cmds.iter() {
                result += estimate_cmd(cmd, vals, visited)?;
            }
            Some(result)
        }();

        vals.insert(INTRINSICS[idx].id.clone(), count);
        count
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cir;
    use crate::compile_ir::{self, param, return_holder};
    use crate::Interpreter;
    use std::convert::TryInto;

    fn create_interp(start_func: &str) -> Interpreter {
        let idx = INTRINSICS
            .iter()
            .enumerate()
            .find(|(_, f)| f.id == FunctionId::new(start_func))
            .unwrap()
            .0;

        let mut interp = Interpreter::new_raw(INTRINSICS.clone(), "");

        interp.call_stack = vec![(idx, 0)];

        for i in 0..31 {
            interp.rust_scores.insert(
                cir::ScoreHolder::new(format!("%%{}", 1 << i)).unwrap(),
                1 << i,
            );
        }

        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%SIXTEEN".into()).unwrap(), 16);
        interp
            .rust_scores
            .insert(cir::ScoreHolder::new("%%-1".into()).unwrap(), -1);

        interp
    }

    fn test_mul_32_to_64(lhs: u32, rhs: u32) {
        let expected = lhs as u64 * rhs as u64;

        let mut interp = create_interp("intrinsic:mul_32_to_64");
        interp.rust_scores.insert(param(0, 0), lhs as i32);
        interp.rust_scores.insert(param(1, 0), rhs as i32);
        println!("Running with lhs {}", lhs);
        interp.run_to_end().unwrap();

        let actual_lo = *interp.rust_scores.get(&return_holder(0)).unwrap() as u32;
        let actual_hi = *interp.rust_scores.get(&return_holder(1)).unwrap() as u32;

        let actual = actual_lo as u64 | ((actual_hi as u64) << 32);

        if expected != actual {
            println!("Input 1:  {:>20} ({:#018X})", lhs, lhs);
            println!("Input 2:  {:>20} ({:#018X})", rhs, rhs);
            println!("Expected: {:>20} ({:#018X})", expected, expected);
            println!("Actual:   {:>20} ({:#018X})", actual, actual);
            panic!();
        }
    }

    #[test]
    #[ignore]
    fn llvm_ctlz_i32() {
        todo!()
    }

    #[test]
    #[ignore]
    fn llvm_fshr_i32() {
        todo!()
    }

    #[test]
    #[ignore]
    fn shl() {
        todo!()
    }

    #[test]
    fn mul_32_to_64() {
        test_mul_32_to_64(123, 0);
        test_mul_32_to_64(0, 123);
        test_mul_32_to_64(25, 42);
        test_mul_32_to_64(42, 25);
        test_mul_32_to_64(70000, 123567);
        test_mul_32_to_64(555555555, 439358984);
        test_mul_32_to_64(0xFF_00_12_56, 123455);
    }

    fn test_bcmp(mem_a: &[u8], mem_b: &[u8], start: usize, len: usize) {
        let mut interp = create_interp("intrinsic:bcmp");

        interp.rust_scores.insert(param(0, 0), start as i32);
        interp.rust_scores.insert(param(1, 0), 0x100 + start as i32);
        interp.rust_scores.insert(param(2, 0), len as i32);

        for (idx, byte) in mem_a.iter().copied().enumerate() {
            interp.set_byte(byte, idx).unwrap();
        }
        for (idx, byte) in mem_b.iter().copied().enumerate() {
            interp.set_byte(byte, idx + 0x100).unwrap();
        }

        let expected = (mem_a[start..][..len] != mem_b[start..][..len]) as i32;

        interp.run_to_end().unwrap();

        let actual = interp.get_rust_score(&return_holder(0)).unwrap();

        assert_eq!(expected, actual);
    }

    #[test]
    fn store_word_unaligned() {
        let mut interp = create_interp("intrinsic:store_word_unaligned");

        let lo_word = [0xAA, 0xAA, 0xAA, 0x12];
        let hi_word = [0xEA, 0x56, 0x88, 0xAA];

        interp.memory[1] = 0xAAAA_AAAA_u32 as i32;
        interp.memory[2] = 0xAAAA_AAAA_u32 as i32;

        interp.rust_scores.insert(compile_ir::ptr(), 7);
        interp
            .rust_scores
            .insert(param(0, 0), i32::from_le_bytes([0x12, 0xEA, 0x56, 0x88]));
        interp.run_to_end().unwrap();

        assert_eq!(interp.memory[1].to_le_bytes(), lo_word);
        assert_eq!(interp.memory[2].to_le_bytes(), hi_word);
    }

    #[test]
    fn load_word_unaligned() {
        let mut interp = create_interp("intrinsic:load_word_unaligned");

        let lo_word = [0xAA, 0xAA, 0xAA, 0x12];
        let hi_word = [0xEA, 0x56, 0x78, 0xAA];

        let expected = 0x78_56_EA_12_i32;

        interp.memory[1] = i32::from_le_bytes(lo_word);
        interp.memory[2] = i32::from_le_bytes(hi_word);

        interp.rust_scores.insert(compile_ir::ptr(), 7);
        interp.run_to_end().unwrap();

        let actual = *interp.rust_scores.get(&return_holder(0)).unwrap();

        if actual != expected {
            println!("Expected: {:>11} ({:#010X})", expected, expected);
            println!("Actual:   {:>11} ({:#010X})", actual, actual);
            panic!();
        }
    }

    // TODO: Generalize, and then test very large memcpy calls
    #[test]
    fn memcpy() {
        let mut interp = create_interp("intrinsic:memcpy");

        for idx in 0..50 {
            interp.set_byte(0xAA, idx).unwrap();
        }

        // Setup source
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9];
        for (idx, byte) in data.iter().copied().enumerate() {
            interp.set_byte(byte, 5 + idx).unwrap();
        }

        println!("Memory before: (address, value)");
        for b in 0..32 {
            print!("{:02X} ", b);
        }
        println!();
        for b in 0..32 {
            print!("{:02X} ", interp.get_byte(b).unwrap());
        }

        interp.rust_scores.insert(param(0, 0), 15);
        interp.rust_scores.insert(param(1, 0), 5);
        interp.rust_scores.insert(param(2, 0), data.len() as i32);
        interp.rust_scores.insert(param(4, 0), 0);
        interp.run_to_end().unwrap();

        println!("Memory after: (address, value)");
        for b in 0..32 {
            print!("{:02X} ", b);
        }
        println!();
        for b in 0..32 {
            print!("{:02X} ", interp.get_byte(b).unwrap());
        }

        for (idx, expected) in data.iter().copied().enumerate() {
            // Make sure nothing changed
            let actual = interp.get_byte(5 + idx).unwrap();
            assert_eq!(expected, actual);
        }

        for (idx, expected) in data.iter().copied().enumerate() {
            let actual = interp.get_byte(15 + idx).unwrap();
            assert_eq!(expected, actual);
        }
        // Make sure nothing changed
        assert_eq!(0xAA, interp.get_byte(14).unwrap());
        assert_eq!(0xAA, interp.get_byte(15 + 9).unwrap());
    }

    #[test]
    fn bcmp() {
        test_bcmp(
            &[12, 34, 9, 8, 7, 6, 5, 99, 0],
            &[43, 21, 9, 8, 7, 6, 5, 88, 5],
            2,
            5,
        );

        test_bcmp(
            &[12, 34, 9, 8, 7, 6, 5, 99, 0],
            &[43, 21, 9, 8, 7, 6, 5, 88, 5],
            2,
            6,
        );
    }

    #[test]
    fn load_byte() {
        let mut interp = create_interp("intrinsic:load_byte");
        let start_stack = interp.call_stack.clone();

        let word = [0x12, 0xEA, 0x56, 0x78];

        interp.memory[1] = i32::from_le_bytes(word);

        for (pt, expected) in word.iter().copied().enumerate() {
            interp.call_stack = start_stack.clone();
            interp.rust_scores.insert(compile_ir::ptr(), pt as i32 + 4);
            interp.run_to_end().unwrap();
            let result = *interp.rust_scores.get(&param(0, 0)).unwrap();
            assert_eq!(result, expected as i32);
        }
    }

    #[test]
    fn load_halfword() {
        let mut interp = create_interp("intrinsic:load_halfword");
        let start_stack = interp.call_stack.clone();

        let word = [0x12, 0xEA, 0x56, 0x78];
        interp.memory[1] = i32::from_le_bytes(word);

        for (pt, expected) in word.chunks_exact(2).enumerate() {
            let expected = u16::from_le_bytes(expected.try_into().unwrap()) as i32;

            interp.call_stack = start_stack.clone();
            interp
                .rust_scores
                .insert(compile_ir::ptr(), 2 * pt as i32 + 4);
            interp.run_to_end().unwrap();
            let result = *interp.rust_scores.get(&param(0, 0)).unwrap();
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn store_halfword() {
        let mut interp = create_interp("intrinsic:store_halfword");
        let start_stack = interp.call_stack.clone();

        let bytes = [0x12, 0xEA, 0x34, 0xBC];

        let mut expected = 0;
        for (idx, halfword) in bytes.chunks_exact(2).enumerate() {
            let halfword = u16::from_le_bytes(halfword.try_into().unwrap()) as i32;

            interp.call_stack = start_stack.clone();
            interp
                .rust_scores
                .insert(compile_ir::ptr(), 8 + 2 * idx as i32);
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
        let mut interp = create_interp("intrinsic:store_byte");
        let start_stack = interp.call_stack.clone();

        let bytes = [0x12, 0xEA, 0x34, 0xBC];

        let mut expected = 0;
        for (idx, byte) in bytes.iter().copied().enumerate() {
            interp.call_stack = start_stack.clone();
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
        let mut interp = create_interp("intrinsic:lshr");
        interp.rust_scores.insert(param(0, 0), a);
        interp.rust_scores.insert(param(1, 0), shift);
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
        test_memset(3, 42, 9);
        test_memset(3, 42, 4);
        test_memset(3, 42, 2);

        test_memset(4, 42, 6);
        test_memset(4, 42, 4);
        test_memset(4, 42, 2);
    }

    fn test_memset(dest: usize, value: u8, len: usize) {
        let mut interp = create_interp("intrinsic:memset");

        for idx in 0..30 {
            interp.set_byte(0xAA, idx).unwrap();
        }

        interp.rust_scores.insert(param(0, 0), dest as i32);
        interp.rust_scores.insert(param(1, 0), value as i32);
        interp.rust_scores.insert(param(2, 0), len as i32);
        interp.run_to_end().unwrap();

        for idx in 0..len {
            let actual = interp.get_byte(dest + idx).unwrap();
            if value != actual {
                eprintln!("Dest:  {}", dest);
                eprintln!("Value: {}", value);
                eprintln!("Len:   {}", len);
                eprintln!();
                eprintln!("Address:  {}", dest + idx);
                eprintln!("Actual:   {:#04X}", actual);
                eprintln!("Expected: {:#04X}", value);
                panic!();
            }
        }

        // Make sure nothing changed
        assert_eq!(0xAA, interp.get_byte(dest - 1).unwrap());
        assert_eq!(0xAA, interp.get_byte(dest + len).unwrap());
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

    fn test_shift_from_ptr(a: i32, ptr: i32) {
        let expected = (a as u32 >> (8 * (ptr % 4))) as i32;
        let mut interp = create_interp("intrinsic:shift_from_ptr");

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
        let mut interp = create_interp(name);
        interp.rust_scores.insert(param(0, 0), a);
        interp.rust_scores.insert(param(1, 0), b);
        interp.run_to_end().unwrap();
        let result = *interp.rust_scores.get(&return_holder(0)).unwrap();
        assert_eq!(result, expected);
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
