use crate::cir::{Command, FuncCall, Function, FunctionId};
use std::collections::HashSet;
use llvm_ir::Terminator;

pub(crate) enum BlockEnd {
    Inlined(Box<AbstractBlock>),
    StaticCall(FunctionId),
    DynCall,
    Normal(Box<Terminator>),
}

pub(crate) struct AbstractBlock {
    pub needs_prolog: bool,
    pub body: Function,
    pub term: BlockEnd,
}

pub(crate) fn has_call_cycle(funcs: &[AbstractBlock], index: usize) -> bool {
    let mut visited = HashSet::new();
    has_call_cycle_inner(funcs, index, &mut visited)
}

fn has_call_cycle_inner(funcs: &[AbstractBlock], index: usize, visited: &mut HashSet<usize>) -> bool {
    for cmd in funcs[index].body.cmds.iter() {
        if let Command::FuncCall(FuncCall { id }) = cmd {
            let callee_idx = funcs.iter().enumerate().find(|f| &f.1.body.id == id).unwrap().0;
            if visited.contains(&callee_idx) {
                return true;
            } else {
                visited.insert(callee_idx);
                if has_call_cycle_inner(funcs, callee_idx, visited) {
                    return true;
                }
            }
        }
    }

    false
}

/*#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn no_cycle() {
        let funcs = vec![
            Function {
                id: FunctionId::new("foo"),
                cmds: vec![Command::FuncCall(FuncCall { id: FunctionId::new("bar") })]
            },
            Function {
                id: FunctionId::new("bar"),
                cmds: vec![]
            },
        ];

        assert_eq!(has_call_cycle(&funcs, 0), false);
        assert_eq!(has_call_cycle(&funcs, 1), false);
    }

    #[test]
    fn has_cycle() {
        let funcs = vec![
            Function {
                id: FunctionId::new("foo"),
                cmds: vec![Command::FuncCall(FuncCall { id: FunctionId::new("bar") })]
            },
            Function {
                id: FunctionId::new("bar"),
                cmds: vec![Command::FuncCall(FuncCall { id: FunctionId::new("foo") })]
            },
        ];

        assert_eq!(has_call_cycle(&funcs, 0), true);
        assert_eq!(has_call_cycle(&funcs, 1), true);
    }

    #[test]
    fn has_loop() {
        let funcs = vec![
            Function {
                id: FunctionId::new("foo"),
                cmds: vec![Command::FuncCall(FuncCall { id: FunctionId::new("foo") })]
            },
        ];

        assert_eq!(has_call_cycle(&funcs, 0), true);
    }
}*/