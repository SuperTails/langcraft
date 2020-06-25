mod cir;

use saltwater::{Locatable, InternedStr, get_str, Type, Literal, types::FunctionType};
use saltwater::hir::{Initializer, Stmt, StmtType, ExprType, Declaration, Expr};
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::collections::HashMap;

static PROGRAM: &str = "
void print(int);

const int FORTY_TWO = 42;

int main() {
    print(FORTY_TWO);
}
";

fn is_print_symbol(Expr { expr, .. }: &Expr) -> bool {
    if let ExprType::Id(id) = &*expr {
        "print" == get_str!(id.get().id)
    } else {
        false
    }
}

fn compile_print(param: &Expr) {
    compile_expr(param);
    todo!("compile rest of print call");
}

fn compile_expr(Expr { expr, .. }: &Expr) {
    match expr {
        ExprType::FuncCall(func, params) => {
            if is_print_symbol(func) {
                assert_eq!(params.len(), 1);
                compile_print(&params[0]);
            }
        }
        // There's only one data type, haha
        ExprType::Cast(c) => {
            compile_expr(c)
        }
        // Pointers? What are those?
        ExprType::Deref(d) => {
            compile_expr(d)
        }
        ExprType::Id
        e => todo!("{:?}", e),       
    }
}

fn compile_body(body: &[Stmt]) {
    for Locatable { data: stmt, .. } in body {
        match stmt {
            StmtType::Expr(expr) => {
                compile_expr(expr);
            }
            _ => todo!("{:#?}", stmt),
        }
    }
}

lazy_static! {
    pub static ref RESERVED_GLOBALS: Mutex<HashMap<InternedStr, i32>> = Mutex::new(HashMap::new());
}

fn reserve_global(ident: InternedStr, initial_value: i32) {
    let mut globals = RESERVED_GLOBALS.lock().unwrap();
    if globals.contains_key(&ident) {
        panic!("duplicate global {}", get_str!(ident));
    }
    globals.insert(ident, initial_value);
}

fn literal_from_initializer(init: &Initializer) -> Option<i32> {
    if let Initializer::Scalar(e) = init {
        if let Expr { expr: ExprType::Cast(c), .. } = &**e {
            if let Expr { expr: ExprType::Literal(Literal::Int(l)), .. } = &**c {
                Some(*l as i32)
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

fn compile_decl(decl: &Declaration) {
    let variable = decl.symbol.get();

    match &variable.ctype {
        Type::Function(FunctionType { return_type, params, varargs }) => {
            if **return_type != Type::Void && "main" != get_str!(variable.id) {
                todo!("Non-void return type on a function");
            }

            if *varargs {
                todo!("variadic arguments");
            }

            eprintln!("function name: {}", get_str!(variable.id));

            if let Some(Initializer::FunctionBody(body)) = &decl.init {
                compile_body(body);
            }
        }
        Type::Int(true) => {
            let initial_value = decl.init.as_ref().map(|i| literal_from_initializer(i).unwrap()).unwrap_or(0);
            reserve_global(variable.id, initial_value);
        }
        Type::Char(_) => panic!("`char` not supported"),
        Type::Short(_) => panic!("`short` not supported"),
        Type::Long(_) => panic!("`long` not supported"),
        ctype => todo!("{:?}", ctype)
    }
}

fn main() {
    let program = saltwater::check_semantics(PROGRAM, saltwater::Opt {
        debug_ast: true,
        debug_hir: true,
        ..Default::default()
    });
    
    if !program.warnings.is_empty() {
        eprintln!("Warnings: {:?}", program.warnings);
    }

    let decls = program.result.unwrap();

    for Locatable { data, .. } in decls {
        compile_decl(&data);
    }
}