use crate::cir::{Command, Execute, FuncCall, Function, FunctionId};
use std::collections::HashSet;
use llvm_ir::Terminator;
use llvm_ir::terminator::Br;
use std::collections::HashMap;

#[derive(Debug, PartialEq, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum BlockEnd {
    Inlined(Box<AbstractBlock>),
    StaticCall(String),
    DynCall,
    Normal(Terminator),
}

fn estimate_count(
    visited: &mut HashSet<FunctionId>,
    list: &HashMap<FunctionId, AbstractBlock>,
    func_starts: &HashMap<String, FunctionId>,
    cmd: &Command
) -> Option<usize> {
    match cmd {
        Command::Execute(Execute { run: Some(run), subcommands: _ }) => Some(1 + estimate_count(visited, list, func_starts, &**run)?),
        Command::FuncCall(FuncCall { id }) => {
            if id.name.starts_with("intrinsic:") {
                *crate::intrinsics::INTRINSIC_COUNTS.get(id).unwrap()
            } else if id.name == "stdout:putc" {
                // FIXME:
                None
            } else {
                todo!("{}", id)
            }
        }
        _ => Some(1),
    }
}

pub(crate) fn estimate_total_count(
    list: &HashMap<FunctionId, AbstractBlock>,
    func_starts: &HashMap<String, FunctionId>,
    block: &AbstractBlock
) -> Option<usize> {
    let mut visited = HashSet::new();
    estimate_total_count_inner(&mut visited, list, func_starts, block)
}

fn estimate_total_count_inner(
    visited: &mut HashSet<FunctionId>,
    list: &HashMap<FunctionId, AbstractBlock>,
    func_starts: &HashMap<String, FunctionId>,
    block: &AbstractBlock
) -> Option<usize> {
    let mut total = 0;
    for cmd in block.body.cmds.iter() {
        total += estimate_count(visited, list, func_starts, cmd)?;
    }
    if let BlockEnd::Inlined(ab) = &block.term {
        total += estimate_total_count(list, func_starts, ab)?;
    }
    Some(total)
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct AbstractBlock {
    pub parent: llvm_ir::Function,
    pub needs_prolog: bool,
    pub body: Function,
    pub term: BlockEnd,
}

impl AbstractBlock {
    /// Returns the destination if this function
    /// ends with a branch or function call
    pub fn get_dest(&self, func_starts: &HashMap<String, FunctionId>) -> Option<FunctionId> {
        match &self.term {
            BlockEnd::Inlined(ab) => ab.get_dest(func_starts),
            BlockEnd::StaticCall(c) => {
                assert!(c.starts_with("!FIXUPCALL "));
                Some(func_starts.get(&c["!FIXUPCALL ".len()..]).unwrap_or_else(|| panic!("failed to get {}", c)).clone())
            },
            BlockEnd::Normal(Terminator::Br(Br { dest, debugloc: _ })) => Some(FunctionId::new_block(&self.parent.name, dest.clone())),
            BlockEnd::DynCall => None,
            _ => None,
        }
    }

    pub fn last(&self) -> &AbstractBlock {
        if let BlockEnd::Inlined(ab) = &self.term {
            ab.last()
        } else {
            self
        }
    }

    pub fn last_mut(&mut self) -> &mut AbstractBlock {
        if let BlockEnd::Inlined(ref mut ab) = self.term {
            ab.last_mut()
        } else {
            self
        }
    }

    pub fn replace_term(&mut self, block: AbstractBlock) {
        self.last_mut().term = BlockEnd::Inlined(Box::new(block));
    }

    pub fn print_chain(&self) {
        self.print_chain_inner(true);
        println!();
    }

    fn print_chain_inner(&self, first: bool) {
        if !first {
            print!(" -> ");
        }

        print!("{}", self.body.id);
        if let BlockEnd::Inlined(b) = &self.term {
            b.print_chain_inner(false);
        }
    }
}

pub(crate) fn has_call_cycle(funcs: &[&Function], index: usize) -> bool {
    let mut visited = HashSet::new();
    has_call_cycle_inner(funcs, index, &mut visited)
}

fn has_call_cycle_inner(funcs: &[&Function], index: usize, visited: &mut HashSet<usize>) -> bool {
    for cmd in funcs[index].cmds.iter() {
        if let Command::FuncCall(FuncCall { id }) = cmd {
            let callee_idx = funcs.iter().enumerate().find(|f| &f.1.id == id).unwrap().0;
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