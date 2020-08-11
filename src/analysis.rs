use crate::cir::{Command, Execute, FuncCall, Function, FunctionId};
use std::collections::{HashSet, HashMap};
use llvm_ir::Terminator;
use llvm_ir::terminator::Br;
use petgraph::prelude::{DiGraph, Graph, NodeIndex};

#[derive(Debug, PartialEq, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum BlockEnd {
    StaticCall(String),
    DynCall,
    Normal(Terminator),
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct AbstractBlock {
    pub parent: llvm_ir::Function,
    pub needs_prolog: bool,
    pub body: Function,
    pub term: Option<BlockEnd>,
}

impl AbstractBlock {
    /// Returns the destination if this function
    /// ends with a branch or function call
    pub fn get_dest(&self, func_starts: &HashMap<String, FunctionId>) -> Option<FunctionId> {
        match self.term.as_ref()? {
            BlockEnd::StaticCall(c) => {
                assert!(c.starts_with("!FIXUPCALL "));
                Some(func_starts.get(&c["!FIXUPCALL ".len()..]).unwrap_or_else(|| panic!("failed to get {}", c)).clone())
            },
            BlockEnd::Normal(Terminator::Br(Br { dest, debugloc: _ })) => Some(FunctionId::new_block(&self.parent.name, dest.clone())),
            BlockEnd::DynCall => None,
            _ => None,
        }
    }

    /*pub fn last(&self) -> &AbstractBlock {
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
    */
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
    Some(total)
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

struct CountList(HashMap<FunctionId, Option<usize>>);

impl CountList {
    fn get(&self, id: &FunctionId) -> Option<usize> {
        *self.0.get(id).unwrap()
    }

    fn new(funcs: &[AbstractBlock], func_starts: &HashMap<String, FunctionId>, list: &HashMap<FunctionId, AbstractBlock>) -> Self {
        let mut s = CountList(HashMap::new());

        for block in funcs {
            s.0.insert(block.body.id.clone(), estimate_total_count(&list, func_starts, &block));
        }

        s
    }
}

pub(crate) type BlockTree = (DiGraph<(AbstractBlock, Option<usize>), ()>, NodeIndex<u32>);

// this returns a tree, but petgraph is really nice
fn build_call_chain(funcs: &[AbstractBlock], block: AbstractBlock, func_starts: &HashMap<String, FunctionId>, counts: &CountList) -> BlockTree {
    let mut result = Graph::new();

    let root_cmds = counts.get(&block.body.id);
    let head = result.add_node((block, root_cmds));
    let mut externals = HashSet::new();
    if root_cmds.is_some() {
        externals.insert(head);
    }

    let mut changed = true;
    while changed {
        changed = false;

        let ext = externals.iter().copied().collect::<Vec<_>>();
        for leaf in ext {
            let parent_cmds = result[leaf].1.unwrap();

            if let Some(dest) = result[leaf].0.get_dest(func_starts) {
                if dest.name.contains("intrinsic") {
                    // TODO:
                } else {
                    let dest = funcs.iter().find(|f| f.body.id == dest).unwrap_or_else(|| panic!("{}", dest));

                    if let Some(new_cmds) = counts.get(&dest.body.id) {
                        if parent_cmds + new_cmds < 60_000 && dest.body.id != result[leaf].0.body.id {
                            println!("{} -> {}", result[leaf].0.body.id, dest.body.id);
                            println!("count is now {}", parent_cmds + new_cmds);

                            result[leaf].0.term = None;
                            let dest_node_idx = result.add_node((dest.clone(), Some(parent_cmds + new_cmds)));
                            result.add_edge(leaf, dest_node_idx, ());
                            assert!(externals.remove(&leaf));
                            assert!(externals.insert(dest_node_idx));

                            changed = true;
                        }
                    }
                }
            }
        }
    }

    (result, head)
}

pub(crate) fn build_call_chains(funcs: &[AbstractBlock], func_starts: &HashMap<String, FunctionId>) -> Vec<BlockTree> {
    /*
        If leaf ends in a direct branch to tail:
            if total cmds okay and leaf != tail:
                append tail to leaf
        
        If leaf ends in a cond branch to tail1 or tail2:
            if total cmds okay and the path from root to leaf contains neither `tail1` nor `tail2`:
                append tail1 and tail2 to leaf
    */

    let list = funcs.iter().map(|f| (f.body.id.clone(), f.clone())).collect();
    let counts = CountList::new(funcs, func_starts, &list);

    let graphs = funcs.iter().cloned().map(|f| build_call_chain(funcs, f, func_starts, &counts)).collect::<Vec<_>>();

    println!("Max graph nodes: {}", graphs.iter().map(|g| g.0.node_count()).max().unwrap());

    /*for g in &graphs {
        println!("{:?}", g);
    }*/

    graphs
}

