use crate::cir::{
    Command, Execute, ExecuteCondKind, ExecuteCondition, ExecuteSubCmd, FuncCall, Function,
    ScoreOp, ScoreSet, Target,
};
use crate::parse::{self, BinaryStmt, Block, ConditionKind, Expr, Ident, IfStmt, Stmt};
use lazy_static::lazy_static;
use std::sync::Mutex;

pub fn compile_unit(parse::Unit { decls }: &parse::Unit) -> Vec<Function> {
    decls
        .iter()
        .flat_map(|decl| compile_function(decl))
        .collect()
}

fn compile_function(parse::Function { name, body }: &parse::Function) -> Vec<Function> {
    let (cmds, mut fns) = compile_block(body);
    fns.push(Function {
        name: format!("rust:{}", name.0),
        cmds,
    });
    fns
}

fn compile_block(program: &Block) -> (Vec<Command>, Vec<Function>) {
    let mut cmds = Vec::new();
    let mut fns = Vec::new();
    for stmt in &program.stmts {
        let (temp_cmds, temp_fns) = compile_stmt(stmt);
        cmds.extend(temp_cmds.into_iter());
        fns.extend(temp_fns.into_iter());
    }
    (cmds, fns)
}

lazy_static! {
    pub static ref TEMP_CNT: Mutex<u32> = Mutex::new(0);
}

fn get_unique_num() -> u32 {
    let mut lock = TEMP_CNT.lock().unwrap();
    let result = *lock;
    *lock += 1;
    result
}

fn eval_expr(expr: &Expr) -> (Vec<Command>, Ident) {
    match expr {
        Expr::Ident(ident) => (vec![], ident.clone()),
        Expr::Literal(score) => {
            let val = get_unique_num();

            let cmd = ScoreSet {
                target: Target::Uuid(format!("__temp{}", val)),
                target_obj: "rust".to_string(),
                score: *score,
            }
            .into();

            (vec![cmd], Ident(format!("__temp{}", val)))
        }
    }
}

fn compile_stmt(stmt: &Stmt) -> (Vec<Command>, Vec<Function>) {
    match stmt {
        Stmt::Binary(BinaryStmt { lhs, op, rhs }) => {
            let (mut commands, ident) = eval_expr(rhs);
            let cmd = ScoreOp {
                target: lhs.clone().into(),
                target_obj: "rust".to_string(),
                kind: *op,
                source: ident.into(),
                source_obj: "rust".to_string(),
            }
            .into();

            commands.push(cmd);

            (commands, vec![])
        }
        Stmt::FuncCall(parse::FuncCall { name }) => (
            vec![FuncCall {
                name: format!("rust:{}", name.0),
            }
            .into()],
            vec![],
        ),
        Stmt::If(IfStmt { conds, body }) => {
            let mut result = Vec::new();
            let mut cmd = Execute::new();
            for parse::Condition {
                inverted,
                lhs,
                kind,
            } in conds
            {
                let (lhs_cmds, lhs_ident) = eval_expr(lhs);
                result.extend(lhs_cmds.into_iter());

                let kind = match kind {
                    ConditionKind::Relation { relation, rhs } => {
                        let (rhs_cmds, rhs_ident) = eval_expr(rhs);
                        result.extend(rhs_cmds.into_iter());
                        ExecuteCondKind::Relation {
                            relation: relation.clone(),
                            source: rhs_ident.into(),
                            source_obj: "rust".to_string(),
                        }
                    }
                    ConditionKind::Matches(range) => ExecuteCondKind::Matches(range.clone()),
                };

                cmd.with_subcmd(ExecuteSubCmd::Condition {
                    is_unless: *inverted,
                    cond: ExecuteCondition::Score {
                        target: lhs_ident.into(),
                        target_obj: "rust".to_string(),
                        kind,
                    },
                });
            }

            // TODO: Optimize if statements with only a single function call in the body
            let functions = if !body.stmts.is_empty() {
                let unique = get_unique_num();

                cmd.with_run(Command::FuncCall(FuncCall {
                    name: format!("rust:__inner{}", unique),
                }));

                let (cmds, mut functions) = compile_block(body);
                functions.push(Function {
                    name: format!("__inner{}", unique),
                    cmds,
                });
                functions
            } else {
                vec![]
            };

            result.push(cmd.into());

            (result, functions)
        }
    }
}
