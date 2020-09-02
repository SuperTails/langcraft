use crate::cir::{Command, Execute, FuncCall, Function, FunctionId, ScoreHolder};
use std::collections::{HashSet, HashMap};
use llvm_ir::Terminator;
use llvm_ir::terminator::{Br, CondBr};
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

    pub fn all_dests(&self, func_starts: &HashMap<String, FunctionId>) -> Option<Vec<FunctionId>> {
        if let Some(d) = self.get_dest(func_starts) {
            Some(vec![d])
        } else if let Some((_, d1, d2)) = self.get_cond_dest() {
            Some(vec![d1, d2])
        } else {
            None
        }
    }

    /// If this block ends in a CondBr, returns the condition, the true branch,
    /// and then the false branch
    pub fn get_cond_dest(&self) -> Option<(ScoreHolder, FunctionId, FunctionId)> {
        match self.term.as_ref()? {
            BlockEnd::Normal(Terminator::CondBr(CondBr { condition, true_dest, false_dest, debugloc: _ })) => {
                if let llvm_ir::Operand::LocalOperand { name, ty } = condition {
                    if matches!(&**ty, llvm_ir::Type::IntegerType { bits: 1 }) {
                        let cond = ScoreHolder::from_local_name(name.clone(), 1).into_iter().next().unwrap();
                        let true_dest = FunctionId::new_block(&self.parent.name, true_dest.clone());
                        let false_dest = FunctionId::new_block(&self.parent.name, false_dest.clone());
                        Some((cond, true_dest, false_dest))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
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

pub(crate) type BlockTree<'a> = (DiGraph<ChainNode<'a>, BlockEdge>, NodeIndex<u32>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockEdge {
    None,
    Cond {
        value: ScoreHolder,
        inverted: bool,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChainNode<'a> {
    pub block: &'a AbstractBlock,
    pub cmd_count: Option<usize>,
    pub depth: usize,
}

static MAX_INLINE_COMMANDS: usize = 10;

static MAX_TREE_DEPTH: usize = 1;

// this returns a tree, but petgraph is really nice
fn build_call_chain<'a>(funcs: &HashMap<&FunctionId, &'a AbstractBlock>, block: &'a AbstractBlock, func_starts: &HashMap<String, FunctionId>, counts: &CountList) -> BlockTree<'a> {
    let mut result = Graph::new();

    let root_cmds = counts.get(&block.body.id);
    let head = result.add_node(ChainNode {
        block,
        cmd_count: root_cmds,
        depth: 0,
    });
    let mut externals = HashSet::new();
    if root_cmds.is_some() {
        externals.insert(head);
    }


    let mut changed = true;
    while changed {
        changed = false;

        println!("External count: {}", externals.len());
        let ext = externals.iter().copied().collect::<Vec<_>>();
        for leaf in ext {
            let parent_cmds = result[leaf].cmd_count.unwrap();

            let new_depth = result[leaf].depth + 1;

            if new_depth > MAX_TREE_DEPTH {
                externals.remove(&leaf);
                continue;
            }

            if let Some(dest) = result[leaf].block.get_dest(func_starts) {
                if dest.name.contains("intrinsic") {
                    // TODO:
                } else {
                    let dest = funcs.get(&dest).unwrap_or_else(|| panic!("{}", dest));

                    if let Some(new_cmds) = counts.get(&dest.body.id) {
                        let inline_ok =
                            parent_cmds + new_cmds < MAX_INLINE_COMMANDS &&
                            dest.body.id != result[leaf].block.body.id;
                            // && !would_cause_duplicate(&result, head, dest, func_starts);

                        if inline_ok {
                            assert!(externals.remove(&leaf));

                            //println!("{} -> {}", result[leaf].block.body.id, dest.body.id);
                            //println!("count is now {}", parent_cmds + new_cmds);

                            //result[leaf].block.term = None;
                            let dest_node_idx = result.add_node(ChainNode { block: dest, cmd_count: Some(parent_cmds + new_cmds), depth: new_depth });
                            result.add_edge(leaf, dest_node_idx, BlockEdge::None);
                            assert!(externals.insert(dest_node_idx));


                            changed = true;
                        }
                    }
                }
            }

            if !changed {
                if let Some((c, true_dest, false_dest)) = result[leaf].block.get_cond_dest() {
                    let true_dest = funcs.get(&true_dest).unwrap_or_else(|| panic!("{}",  true_dest));
                    let false_dest = funcs.get(&false_dest).unwrap_or_else(|| panic!("{}", false_dest));

                    if let Some(true_cmds) = counts.get(&true_dest.body.id) {
                        if let Some(false_cmds) = counts.get(&false_dest.body.id) {
                            let new_cmds = true_cmds.max(false_cmds);
                            if parent_cmds + new_cmds < MAX_INLINE_COMMANDS {
                                assert!(externals.remove(&leaf));

                                //println!("{} -> {}\n\t -> {}", result[leaf].block.body.id, true_dest.body.id, false_dest.body.id);
                                //println!("count is now {}", parent_cmds + new_cmds);

                                //result[leaf].block.term = None;

                                let true_idx = result.add_node(ChainNode { block: true_dest, cmd_count: Some(parent_cmds + new_cmds), depth: new_depth });
                                result.add_edge(leaf, true_idx, BlockEdge::Cond { value: c.clone(), inverted: false });
                                assert!(externals.insert(true_idx));

                                let false_idx = result.add_node(ChainNode { block: false_dest, cmd_count: Some(parent_cmds + new_cmds), depth: new_depth });
                                result.add_edge(leaf, false_idx, BlockEdge::Cond { value: c.clone(), inverted: true });
                                assert!(externals.insert(false_idx));

                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    println!("Finished one");

    (result, head)
}

pub(crate) fn build_call_chains<'a>(funcs: &'a [AbstractBlock], func_starts: &HashMap<String, FunctionId>) -> Vec<BlockTree<'a>> {
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

    println!("Building chains out of {} MC basic blocks", funcs.len());

    let funcs = funcs.iter().map(|f| (&f.body.id, f)).collect::<HashMap<&FunctionId, &AbstractBlock>>();

    let graphs = funcs.iter().map(|(_, f)| build_call_chain(&funcs, f, func_starts, &counts)).collect::<Vec<_>>();

    println!("Max graph nodes: {}", graphs.iter().map(|g| g.0.node_count()).max().unwrap());

    /*for g in &graphs {
        println!("{:?}", g);
    }*/

    graphs
}

