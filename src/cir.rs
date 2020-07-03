use llvm_ir::Name;
use std::fmt;
use std::ops::{RangeFrom, RangeInclusive, RangeToInclusive};
use std::string::ToString;

#[derive(Debug, PartialEq, Hash, Clone)]
pub enum McRange {
    To(RangeToInclusive<i32>),
    From(RangeFrom<i32>),
    Between(RangeInclusive<i32>),
}

impl McRange {
    pub fn contains(&self, item: i32) -> bool {
        match self {
            Self::To(r) => r.contains(&item),
            Self::From(r) => r.contains(&item),
            Self::Between(r) => r.contains(&item),
        }
    }
}

impl fmt::Display for McRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::To(r) => write!(f, "..{}", r.end),
            Self::From(r) => write!(f, "{}..", r.start),
            Self::Between(r) => write!(f, "{}..{}", r.start(), r.end()),
        }
    }
}

// TODO: There's many more variants
#[derive(Debug, PartialEq, Clone)]
pub enum Predicate {
    Inverted(Box<Predicate>),
    /// Minecraft calls this "alternative"
    Or(Vec<Predicate>),
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{ ")?;

        match self {
            Predicate::Inverted(inner) => write!(
                f,
                "\"condition\": \"minecraft:inverted\", \"term\": {}",
                inner
            )?,
            Predicate::Or(inner) => {
                let inner = inner
                    .iter()
                    .map(|i| format!("{}", i))
                    .collect::<Vec<String>>();

                write!(
                    f,
                    "\tcondition\": \"minecraft:alternative\", \"term\": [{}]",
                    inner.join(", ")
                )?;
            }
        }

        write!(f, " }}")
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionId {
    pub name: String,
    pub block: Name,
    pub sub: usize,
}

impl FunctionId {
    pub fn new<T: ToString>(name: T) -> Self {
        FunctionId {
            name: name.to_string(),
            block: Name::Number(0),
            sub: 0,
        }
    }

    pub fn new_block<T: ToString>(name: T, block: Name) -> Self {
        FunctionId {
            name: name.to_string(),
            block,
            sub: 0,
        }
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;

        let block = if let Name::Number(n) = self.block {
            n
        } else {
            todo!("{:?}", self.name)
        };

        if block != 0 || self.sub != 0 {
            write!(f, "-block{}", block)?;
        }
        if self.sub != 0 {
            write!(f, "-sub{}", self.sub)?;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Function {
    pub id: FunctionId,
    pub cmds: Vec<Command>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FuncCall {
    pub id: FunctionId,
}

impl fmt::Display for FuncCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id = self.id.to_string();
        if id.contains(':') {
            write!(f, "function {}", id)
        } else {
            write!(f, "function rust:{}", id)
        }
    }
}

// Objectives are `i32`
//  - Name [a-zA-Z0-9_.\-+]
//  - Criterion
//  - Display name (JSON)

// Entities can have *scores* in certain objectives
// Score holder is a player's name or entity's UUID that has scores in some objective

/// Execute:
/// `<TARGET>` is the same as the one for the `data` command
///
/// ```
/// execute
/// ... align <axes> -> execute
/// ... anchored <anchor> -> execute
/// ... as <targets> -> execute
/// ... at <targets> -> execute
/// ... facing (<pos>|entity <targets> <anchor>) -> execute
/// ... in <dimension> -> execute
/// ... positioned (<pos>|as <targets>) -> execute
/// ... rotated (<rot>|as <targets>) -> execute
/// ... store (result|success)
///     ... <TARGET> <path> (byte|short|int|long|float|double) <scale> -> execute
///     ... bossbar <id> (max|value) -> execute
///     ... score <targets> <objective> -> execute
/// ... (if|unless)
///     ... block <pos> <block> -> [execute]
///     ... blocks <start> <end> <destination> (all|masked) -> [execute]
///     ... data
///         ... block <sourcePos> <path> -> [execute]
///         ... entity <source> <path> -> [execute]
///         ... storage <source> <path> -> [execute]
///     ... entity <entities> -> [execute]
///     ... predicate <predicate> -> [execute]
///     ... score <target> <targetObjective>
///         ... (< | <= | = | > | >=) <source> <sourceObjective> -> [execute]
///         ... matches <range> -> [execute]
/// ... run <command>
/// ```

#[derive(Debug, PartialEq, Clone, Default)]
pub struct Execute {
    pub subcommands: Vec<ExecuteSubCmd>,
    pub run: Option<Box<Command>>,
}

impl fmt::Display for Execute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "execute")?;
        for sub in self.subcommands.iter() {
            write!(f, " {}", sub)?;
        }
        if let Some(run) = &self.run {
            write!(f, " run {}", run)?;
        }
        Ok(())
    }
}

impl Execute {
    pub fn new() -> Self {
        Execute::default()
    }

    pub fn with_subcmd(&mut self, cmd: ExecuteSubCmd) -> &mut Self {
        self.subcommands.push(cmd);
        self
    }

    pub fn with_if(&mut self, cond: ExecuteCondition) -> &mut Self {
        self.with_subcmd(ExecuteSubCmd::Condition {
            is_unless: false,
            cond,
        })
    }

    pub fn with_unless(&mut self, cond: ExecuteCondition) -> &mut Self {
        self.with_subcmd(ExecuteSubCmd::Condition {
            is_unless: true,
            cond,
        })
    }

    pub fn with_as(&mut self, target: Target) -> &mut Self {
        self.with_subcmd(ExecuteSubCmd::As { target })
    }

    pub fn with_at(&mut self, target: Target) -> &mut Self {
        self.with_subcmd(ExecuteSubCmd::At { target })
    }

    pub fn with_run<C: Into<Command>>(&mut self, cmd: C) -> &mut Self {
        assert!(self.run.is_none());

        self.run = Some(Box::new(cmd.into()));
        self
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExecuteSubCmd {
    // TODO: There's others lol
    Condition {
        is_unless: bool,
        cond: ExecuteCondition,
    },
    Store {
        is_success: bool,
        kind: ExecuteStoreKind,
    },
    As {
        target: Target,
    },
    At {
        target: Target,
    },
}

impl fmt::Display for ExecuteSubCmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Condition { is_unless, cond } => {
                if *is_unless {
                    write!(f, "unless")?;
                } else {
                    write!(f, "if")?;
                }

                write!(f, " {}", cond)
            }
            Self::Store { is_success, kind } => {
                write!(f, "store ")?;
                if *is_success {
                    write!(f, "success ")?;
                } else {
                    write!(f, "result ")?;
                }
                write!(f, "{}", kind)
            }
            Self::As { target } => write!(f, "as {}", target),
            Self::At { target } => write!(f, "at {}", target),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExecuteStoreKind {
    // TODO: There's 2 other kinds
    Score {
        target: Target,
        objective: Objective,
    },
    Data {
        target: DataTarget,
        path: String,
        ty: String,
        scale: f32,
    },
}

impl fmt::Display for ExecuteStoreKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Score { target, objective } => write!(f, "score {} {}", target, objective),
            Self::Data {
                target,
                path,
                ty,
                scale,
            } => write!(f, "{} {} {} {}", target, path, ty, scale),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExecuteCondition {
    // TODO: There's more
    Score {
        target: Target,
        target_obj: Objective,
        kind: ExecuteCondKind,
    },
    Block {
        pos: String,
        block: String,
    },
}

impl fmt::Display for ExecuteCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecuteCondition::Score {
                target,
                target_obj,
                kind,
            } => write!(f, "score {} {} {}", target, target_obj, kind),
            ExecuteCondition::Block { pos, block } => write!(f, "block {} {}", pos, block),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ExecuteCondKind {
    Relation {
        relation: Relation,
        source: Target,
        source_obj: Objective,
    },
    Matches(McRange),
}

impl fmt::Display for ExecuteCondKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relation {
                relation,
                source,
                source_obj,
            } => write!(f, "{} {} {}", relation, source, source_obj),
            Self::Matches(range) => write!(f, "matches {}", range),
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Hash, Clone)]
pub enum Relation {
    LessThan,
    LessThanEq,
    Eq,
    GreaterThan,
    GreaterThanEq,
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Relation::LessThan => write!(f, "<"),
            Relation::LessThanEq => write!(f, "<="),
            Relation::Eq => write!(f, "="),
            Relation::GreaterThan => write!(f, ">"),
            Relation::GreaterThanEq => write!(f, ">="),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    SetBlock(SetBlock),
    ScoreOp(ScoreOp),
    ScoreSet(ScoreSet),
    ScoreGet(ScoreGet),
    ScoreAdd(ScoreAdd),
    Execute(Execute),
    FuncCall(FuncCall),
    Data(Data),
    Tellraw(Tellraw),
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::SetBlock(s) => s.fmt(f),
            Command::ScoreOp(s) => s.fmt(f),
            Command::ScoreSet(s) => s.fmt(f),
            Command::ScoreGet(s) => s.fmt(f),
            Command::ScoreAdd(s) => s.fmt(f),
            Command::Execute(s) => s.fmt(f),
            Command::FuncCall(s) => s.fmt(f),
            Command::Data(s) => s.fmt(f),
            Command::Tellraw(s) => s.fmt(f),
        }
    }
}

impl From<ScoreOp> for Command {
    fn from(s: ScoreOp) -> Self {
        Command::ScoreOp(s)
    }
}

impl From<ScoreSet> for Command {
    fn from(s: ScoreSet) -> Self {
        Command::ScoreSet(s)
    }
}

impl From<Execute> for Command {
    fn from(e: Execute) -> Self {
        Command::Execute(e)
    }
}

impl From<FuncCall> for Command {
    fn from(f: FuncCall) -> Self {
        Command::FuncCall(f)
    }
}

impl From<Data> for Command {
    fn from(d: Data) -> Self {
        Command::Data(d)
    }
}

impl From<ScoreGet> for Command {
    fn from(s: ScoreGet) -> Self {
        Command::ScoreGet(s)
    }
}

impl From<ScoreAdd> for Command {
    fn from(s: ScoreAdd) -> Self {
        Command::ScoreAdd(s)
    }
}

impl From<SetBlock> for Command {
    fn from(s: SetBlock) -> Self {
        Command::SetBlock(s)
    }
}

impl From<Tellraw> for Command {
    fn from(t: Tellraw) -> Self {
        Command::Tellraw(t)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct Tellraw {
    pub target: Target,
    pub message: String,
}

impl fmt::Display for Tellraw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tellraw {} {}", self.target, self.message)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct SetBlock {
    pub pos: String,
    pub block: String,
    pub kind: SetBlockKind,
}

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub enum SetBlockKind {
    Destroy,
    Keep,
    Replace,
}

impl fmt::Display for SetBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "setblock {} {} {}", self.pos, self.block, self.kind)
    }
}

impl fmt::Display for SetBlockKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetBlockKind::Destroy => write!(f, "destroy"),
            SetBlockKind::Keep => write!(f, "keep"),
            SetBlockKind::Replace => write!(f, "replace"),
        }
    }
}

type Objective = String;

// TODO: This should be an enum, probably
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectorArg(pub String);

impl fmt::Display for SelectorArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Selector {
    pub var: SelectorVariable,
    pub args: Vec<SelectorArg>,
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.var)?;
        let args = self
            .args
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<String>>();
        if !args.is_empty() {
            write!(f, "[{}]", args.join(","))
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

/* Scoreboard (players functions)

TODO: scoreboard players reset <targets> [<objectives>]
*/

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScoreGet {
    pub target: Target,
    pub target_obj: Objective,
}

impl fmt::Display for ScoreGet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "scoreboard players get {} {}",
            self.target, self.target_obj
        )
    }
}

/// `scoreboard players set <targets> <objective> <score>`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScoreAdd {
    pub target: Target,
    pub target_obj: Objective,
    pub score: i32,
}

impl fmt::Display for ScoreAdd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scoreboard players ")?;
        let score = if self.score < 0 {
            write!(f, "remove ")?;
            -self.score
        } else {
            write!(f, "add ")?;
            self.score
        };

        write!(f, "{} {} {}", self.target, self.target_obj, score)
    }
}

/// `scoreboard players set <targets> <objective> <score>`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScoreSet {
    pub target: Target,
    pub target_obj: Objective,
    pub score: i32,
}

impl fmt::Display for ScoreSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "scoreboard players set {} {} {}",
            self.target, self.target_obj, self.score
        )
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
    pub target: Target,
    pub target_obj: Objective,
    pub kind: ScoreOpKind,
    pub source: Target,
    pub source_obj: Objective,
}

impl fmt::Display for ScoreOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "scoreboard players operation {} {} {} {} {}",
            self.target, self.target_obj, self.kind, self.source, self.source_obj
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

#[derive(Debug, PartialEq, Clone)]
pub struct Data {
    pub target: DataTarget,
    pub kind: DataKind,
}

impl fmt::Display for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "data ")?;
        match &self.kind {
            DataKind::Get { .. } => write!(f, "get ")?,
            DataKind::Modify { .. } => write!(f, "modify ")?,
        }
        write!(f, "{} ", self.target)?;
        match &self.kind {
            DataKind::Get { path, scale } => write!(f, "{} {}", path, scale),
            DataKind::Modify { path, kind, source } => write!(f, "{} {} {}", path, kind, source),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DataKind {
    Get {
        path: String,
        scale: f32,
    },
    Modify {
        path: String,
        kind: DataModifyKind,
        source: DataModifySource,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum DataModifyKind {
    // TODO: There's others
    Set,
}

impl fmt::Display for DataModifyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Set => write!(f, "set"),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DataModifySource {
    // TODO: There's another
    // TODO: This can technically be other datatypes too, I think
    Value(i32),
}

impl fmt::Display for DataModifySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataModifySource::Value(v) => write!(f, "value {}", v),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum DataTarget {
    // TODO: More
    Block(String),
    Entity(Target),
}

impl fmt::Display for DataTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataTarget::Block(b) => write!(f, "block {}", b),
            DataTarget::Entity(e) => write!(f, "entity {}", e),
        }
    }
}
