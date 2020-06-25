use std::fmt;
use std::string::ToString;

// Objectives are `i32`
//  - Name [a-zA-Z0-9_.\-+]
//  - Criterion
//  - Display name (JSON)

// Entities can have *scores* in certain objectives
// Score holder is a player's name or entity's UUID that has scores in some objective

/* Execute:

`<TARGET>` is the same as the one for the `data` command

... align <axes> -> execute
... anchored <anchor> -> execute
... as <targets> -> execute
... at <targets> -> execute
... facing (<pos>|entity <targets> <anchor>) -> execute
... in <dimension> -> execute
... positioned (<pos>|as <targets>) -> execute
... rotated (<rot>|as <targets>) -> execute
... store (result|success)
    ... <TARGET> <path> (byte|short|int|long|float|double) <scale> -> execute
    ... bossbar <id> (max|value) -> execute
    ... score <targets> <objective> -> execute
... (if|unless)
    ... block <pos> <block> -> [execute]
    ... blocks <start> <end> <destination> (all|masked) -> [execute]
    ... data
        ... block <sourcePos> <path> -> [execute]
        ... entity <source> <path> -> [execute]
        ... storage <source> <path> -> [execute]
    ... entity <entities> -> [execute]
    ... predicate <predicate> -> [execute]
    ... score <target> <targetObjective>
        ... (< | <= | = | > | >=) <source> <sourceObjective> -> [execute]
        ... matches <range> -> [execute]
... run <command>

*/

/* Scoreboard (players functions)

TODO: scoreboard players set <targets> <objective> <score>
TODO: scoreboard players reset <targets> [<objectives>]

*/

type Objective = String;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectorArg;

impl fmt::Display for SelectorArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Selector {
    var: SelectorVariable,
    args: Vec<SelectorArg>,
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.var)?;
        let args = self.args.iter().map(|a| a.to_string()).collect::<Vec<String>>();
        if !args.is_empty() {
            write!(f, "{}", args.join(","))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SelectorVariable {
    NearestPlayer,
    RandomPlayer,
    AllPlayers,
    AllEntities,
    ThisEntity,
}

impl fmt::Display for SelectorVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NearestPlayer => write!(f, "@p"),
            Self::RandomPlayer => write!(f, "@r"),
            Self::AllPlayers => write!(f, "@a"),
            Self::AllEntities => write!(f, "@e"),
            Self::ThisEntity => write!(f, "@s"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Uuid(String),
    Selector(Selector),
    Asterisk,
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uuid(uuid) => write!(f, "{}", uuid),
            Self::Selector(selector) => write!(f, "{}", selector),
            Self::Asterisk => write!(f, "*"),
        }
    }
}

/// `scoreboard players operation <targets> <targetObjective> <operation> <source> <sourceObjective>`
/// 
/// `<operation>` may be: `+=`, `-=`, `*=`, `/=`, `%=`, `=`, `<` (min), `>` (max), `><` (swap)
/// 
/// Both `target` and `source` may be `*`, which uses all entites tracked by (TODO: the scoreboard? that objective?)
///
/// All operations treat a null score as 0
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScoreOp {
    target: Target,
    target_obj: Objective,
    kind: ScoreOpKind,
    source: Target,
    source_obj: Objective,
}

impl fmt::Display for ScoreOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "scoreboard players operation {} {} {} {} {}",
            self.target,
            self.target_obj,
            self.kind,
            self.source,
            self.source_obj
        )
            
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScoreOpKind {
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    Assign,
    Min,
    Max,
    Swap,
}

impl fmt::Display for ScoreOpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScoreOpKind::AddAssign => write!(f, "+="),
            ScoreOpKind::SubAssign => write!(f, "-="),
            ScoreOpKind::MulAssign => write!(f, "*="),
            ScoreOpKind::DivAssign => write!(f, "/="),
            ScoreOpKind::ModAssign => write!(f, "%="),
            ScoreOpKind::Assign => write!(f, "="),
            ScoreOpKind::Min => write!(f, "<"),
            ScoreOpKind::Max => write!(f, ">"),
            ScoreOpKind::Swap => write!(f, "><"),
        }
    }
}

/*

/data
    ... get <TARGET> [<path>] [<scale>]
    ... merge <TARGET> <nbt>
    ... modify <TARGET> <targetPath> <MODIFICATION>
        ... from <SOURCE> [<sourcePath>]
        ... value <value>
    ... remove <TARGET> <path>

<TARGET> = <SOURCE> = (block <targetPos> | entity <target> | storage <target>)
<MODIFICATION> = (append | insert <index> | merge | prepend | set)

*/