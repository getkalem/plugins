//! The syntax tree of a VBA module.

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal number.
    Number(f64),
    /// A literal integer.
    Integer(i64),
    /// A literal string.
    Str(String),
    /// A literal date, as a serial.
    Date(f64),
    /// `True`, `False`.
    Bool(bool),
    /// `Nothing`.
    Nothing,
    /// `Empty`.
    Empty,
    /// `Null`.
    Null,
    /// A name, lower case.
    Name(String),
    /// `.Member` inside `With`.
    WithMember(String),
    /// `object.member`.
    Member(Box<Expr>, String),
    /// `target(args)`: a call, an array element or a default member.
    Call(Box<Expr>, Vec<Arg>),
    /// A unary operator: `-`, `Not`.
    Unary(&'static str, Box<Expr>),
    /// A binary operator, lower case for the words (`and`, `mod`).
    Binary(&'static str, Box<Expr>, Box<Expr>),
    /// `[A1]`: a reference evaluated on the active sheet.
    Bracket(String),
}

/// An argument: positional, named (`Title:="x"`), or missing (`f(, 2)`).
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    /// The name for `name:=value`, lower case.
    pub name: Option<String>,
    /// The value; `None` when omitted.
    pub value: Option<Expr>,
}

/// A `Case` clause item.
#[derive(Debug, Clone, PartialEq)]
pub enum CaseItem {
    /// `Case 3`.
    Value(Expr),
    /// `Case 1 To 5`.
    Range(Expr, Expr),
    /// `Case Is > 3`.
    Is(&'static str, Expr),
}

/// The bounds of one array dimension: `(10)` or `(1 To 10)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    /// The lower bound, `Option Base` when absent.
    pub lower: Option<Expr>,
    /// The upper bound.
    pub upper: Expr,
}

/// A declared variable.
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    /// The name, lower case.
    pub name: String,
    /// `Dim a()` (dynamic) or `Dim a(1 To 3)`: `Some` for an array.
    pub bounds: Option<Vec<Bound>>,
    /// The type name, lower case (`long`, `string`, `range`); `None` for Variant.
    pub ty: Option<String>,
    /// `As New Collection`.
    pub new: bool,
}

/// A statement, with its line for messages.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    /// One-based source line.
    pub line: usize,
    /// What it does.
    pub kind: StmtKind,
}

/// What a statement does.
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `Dim`, `Static`, `Private`, `Public` inside a procedure.
    Dim(Vec<Decl>),
    /// `ReDim [Preserve] a(…)`.
    ReDim {
        /// Keep the elements.
        preserve: bool,
        /// The arrays and their new bounds.
        decls: Vec<Decl>,
    },
    /// `Const x = 1`.
    Const(Vec<(String, Expr)>),
    /// `target = value` (`Let`).
    Assign(Expr, Expr),
    /// `Set target = value`.
    Set(Expr, Expr),
    /// A call as a statement: `Foo 1, 2`, `Call Foo(1, 2)`, `obj.Method x`.
    Call(Expr, Vec<Arg>),
    /// `If … Then … ElseIf … Else … End If`.
    If {
        /// Conditions and their blocks.
        arms: Vec<(Expr, Vec<Stmt>)>,
        /// The `Else` block.
        otherwise: Vec<Stmt>,
    },
    /// `For v = a To b [Step s] … Next`.
    For {
        /// The counter.
        var: Expr,
        /// Start.
        from: Expr,
        /// End.
        to: Expr,
        /// Step.
        step: Option<Expr>,
        /// Body.
        body: Vec<Stmt>,
    },
    /// `For Each v In c … Next`.
    ForEach {
        /// The element variable.
        var: Expr,
        /// The collection, array or range.
        over: Expr,
        /// Body.
        body: Vec<Stmt>,
    },
    /// `Do [While|Until c] … Loop [While|Until c]`, and `While … Wend`.
    Do {
        /// A test before each pass: (until, condition).
        pre: Option<(bool, Expr)>,
        /// A test after each pass.
        post: Option<(bool, Expr)>,
        /// Body.
        body: Vec<Stmt>,
    },
    /// `Select Case e … End Select`.
    Select {
        /// The tested expression.
        subject: Expr,
        /// Cases.
        cases: Vec<(Vec<CaseItem>, Vec<Stmt>)>,
        /// `Case Else`.
        otherwise: Vec<Stmt>,
    },
    /// `With o … End With`.
    With(Expr, Vec<Stmt>),
    /// `Exit Sub`, `Exit Function`, `Exit For`, `Exit Do`, `Exit Property`.
    Exit(String),
    /// `On Error Resume Next`.
    OnErrorResumeNext,
    /// `On Error GoTo label` (`None` for `GoTo 0`).
    OnErrorGoTo(Option<String>),
    /// `GoTo label`.
    GoTo(String),
    /// `label:`.
    Label(String),
    /// `Resume Next`, `Resume label`, `Resume`.
    Resume(Option<String>),
    /// `Err.Raise` and other statements are calls; `End` stops everything.
    End,
    /// `Debug.Print a; b` — kept as a call to `debug.print`.
    Nothing,
}

/// A procedure parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Lower case name.
    pub name: String,
    /// `ByVal`.
    pub by_val: bool,
    /// `Optional … = default`.
    pub optional: Option<Option<Expr>>,
    /// `ParamArray`.
    pub param_array: bool,
    /// Declared array (`a()`).
    pub array: bool,
}

/// A `Sub`, `Function` or `Property Get`.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    /// The name as written.
    pub name: String,
    /// `Function` (returns a value).
    pub function: bool,
    /// `Private`.
    pub private: bool,
    /// Parameters.
    pub params: Vec<Param>,
    /// Body.
    pub body: Vec<Stmt>,
    /// The `Sub` line.
    pub line: usize,
}

/// A module: its variables, constants and procedures.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    /// The module's name.
    pub name: String,
    /// Module-level declarations.
    pub vars: Vec<Decl>,
    /// Module-level constants.
    pub consts: Vec<(String, Expr)>,
    /// Procedures.
    pub procs: Vec<Proc>,
    /// `Option Base 1`.
    pub option_base: i64,
    /// Calls the parser rejected outright (`Declare`), with lines.
    pub refused: Vec<(usize, String)>,
}
