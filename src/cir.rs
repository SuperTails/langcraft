use llvm_ir::Name;
use std::fmt;
use std::ops::{RangeFrom, RangeInclusive, RangeToInclusive};
use std::string::ToString;
pub use raw_text::*;

mod raw_text;

/// Characters not allowed:
/// All non-printing characters (anywhere)
/// whitespace (anywhere)
/// '*' (by itself)
/// '@' (as the first character)
/// '"' (technically allowed, but complicates JSON)
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, serde::Serialize)]
pub struct ScoreHolder(String);

impl ScoreHolder {
    pub fn new(mut string: String) -> Result<Self, String> {
        if string.is_empty() {
            return Err(string);
        }

        if string.len() == 1 && string == "*" {
            return Err(string);
        }

        if string.starts_with('@') {
            return Err(string);
        }

        if string.contains(|c: char| c.is_control() || c.is_whitespace()) {
            return Err(string);
        }

        if string.contains('"') {
            println!("TODO: allow quotation marks {:?}", string);
            string.truncate(string.len() - 1);
            string.remove(0);
            assert!(!string.contains('"'))
        }

        Ok(ScoreHolder(string))
    }

    pub fn from_local_name(name: llvm_ir::Name, type_size: usize) -> Vec<Self> {
        let prefix = match name {
            llvm_ir::Name::Number(n) => format!("%{}", n),
            llvm_ir::Name::Name(n) => n,
        };

        (0..((type_size + 3) / 4))
            .map(|idx| ScoreHolder::new(format!("{}%{}", prefix, idx)).unwrap())
            .collect()
    }

    /// Replaces any characters in the input string that are not valid in a score holder
    /// with '_'
    pub fn new_lossy(string: String) -> Self {
        todo!("{:?}", string)
    }
}

impl AsRef<str> for ScoreHolder {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScoreHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Uuid(ScoreHolder),
    Selector(Selector),
    Asterisk,
}

impl From<ScoreHolder> for Target {
    fn from(score_holder: ScoreHolder) -> Self {
        Target::Uuid(score_holder)
    }
}

impl From<Selector> for Target {
    fn from(selector: Selector) -> Self {
        Target::Selector(selector)
    }
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(into = "String")]
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

impl Into<String> for Selector {
    fn into(self) -> String {
        self.to_string()
    }
}

// TODO: These support a much more limit set of characters than a scoreboard objective
// TODO: This should be an enum, probably
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectorArg(pub String);

impl fmt::Display for SelectorArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
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

impl From<RangeToInclusive<i32>> for McRange {
    fn from(r: RangeToInclusive<i32>) -> Self {
        Self::To(r)
    }
}

impl From<RangeFrom<i32>> for McRange {
    fn from(r: RangeFrom<i32>) -> Self {
        Self::From(r)
    }
}

impl From<RangeInclusive<i32>> for McRange {
    fn from(r: RangeInclusive<i32>) -> Self {
        Self::Between(r)
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
        Self::new_sub(name, Name::Number(0), 0)
    }

    pub fn new_block<T: ToString>(name: T, block: Name) -> Self {
        Self::new_sub(name, block, 0)
    }

    pub fn new_sub<T: ToString>(name: T, mut block: Name, sub: usize) -> Self {
        let mut name = name.to_string();
        name = name.replace(|c| c == '$' || c == '.' || c == '-', "_");
        name = name.to_ascii_lowercase();

        if let Name::Name(n) = &mut block {
            *n = n.replace(|c| c == '$' || c == '.' || c == '-', "_");
            *n = n.to_ascii_lowercase();
        }

        FunctionId { name, block, sub }
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;

        if self.block != Name::Number(0) || self.sub != 0 {
            match &self.block {
                Name::Number(n) => write!(f, "-block{}", n)?,
                Name::Name(n) => write!(f, "-block{}", n)?,
            }
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
/// ```text
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
        pos: BlockPos,
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
    Fill(Fill),
    SetBlock(SetBlock),
    ScoreOp(ScoreOp),
    ScoreSet(ScoreSet),
    ScoreGet(ScoreGet),
    ScoreAdd(ScoreAdd),
    Execute(Execute),
    FuncCall(FuncCall),
    Data(Data),
    Tellraw(Box<Tellraw>),
    Comment(String),
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Fill(s) => s.fmt(f),
            Command::SetBlock(s) => s.fmt(f),
            Command::ScoreOp(s) => s.fmt(f),
            Command::ScoreSet(s) => s.fmt(f),
            Command::ScoreGet(s) => s.fmt(f),
            Command::ScoreAdd(s) => s.fmt(f),
            Command::Execute(s) => s.fmt(f),
            Command::FuncCall(s) => s.fmt(f),
            Command::Data(s) => s.fmt(f),
            Command::Tellraw(s) => s.fmt(f),
            Command::Comment(s) => {
                let mut commented = s.replace('\n', "\n# ");
                commented.insert_str(0, "# ");
                write!(f, "{}", commented)
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Fill {
    pub start: String,
    pub end: String,
    pub block: String,
}

impl fmt::Display for Fill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fill {} {} {}", self.start, self.end, self.block)
    }
}

impl From<Fill> for Command {
    fn from(f: Fill) -> Self {
        Command::Fill(f)
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
        Command::Tellraw(Box::new(t))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Tellraw {
    pub target: Target,
    pub message: Vec<TextComponent>,
}

type NbtPath = String;
type BlockPos = String;
type StorageId = String;
type StringNbt = String;

impl fmt::Display for Tellraw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tellraw {} {}", self.target, serde_json::to_string(&self.message).unwrap())
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct SetBlock {
    pub pos: String,
    pub block: BlockPos,
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
