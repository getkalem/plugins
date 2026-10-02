//! VBA modules parsed into the syntax tree of [`super::ast`] ([MS-VBAL] 5).

use super::ast::*;
use super::lexer::{self, Line, Tok};

/// A parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// One-based line.
    pub line: usize,
    /// The message.
    pub message: String,
}

type PResult<T> = Result<T, ParseError>;

/// One statement's tokens and line.
#[derive(Debug, Clone)]
struct Item {
    line: usize,
    toks: Vec<Tok>,
}

/// Splits logical lines into statements at `:`; a label (`name:`) is its
/// own statement; a one-line `If … Then …` keeps the rest of its line.
fn items(lines: Vec<Line>) -> Vec<Item> {
    let mut out = Vec::new();
    for l in lines {
        let mut cur: Vec<Tok> = Vec::new();
        let mut toks = l.toks.into_iter().peekable();
        let mut first = true;
        while let Some(t) = toks.next() {
            // A label at the start of a line.
            if first
                && cur.is_empty()
                && matches!(&t, Tok::Ident(s) if !is_statement_keyword(s))
                && toks.peek().is_some_and(|n| n.is_p(":"))
            {
                toks.next();
                out.push(Item {
                    line: l.number,
                    toks: vec![t, Tok::Punct(":")],
                });
                continue;
            }
            first = false;
            if t.is_p(":")
                && !(cur.first().is_some_and(|f| f.is_kw("if"))
                    && cur.iter().any(|c| c.is_kw("then")))
            {
                if !cur.is_empty() {
                    out.push(Item {
                        line: l.number,
                        toks: std::mem::take(&mut cur),
                    });
                }
                continue;
            }
            cur.push(t);
        }
        if !cur.is_empty() {
            out.push(Item {
                line: l.number,
                toks: cur,
            });
        }
    }
    out
}

fn is_statement_keyword(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "else"
            | "end"
            | "next"
            | "loop"
            | "wend"
            | "resume"
            | "exit"
            | "dim"
            | "call"
            | "debug"
            | "beep"
            | "stop"
    )
}

/// Parses a module's source.
pub fn parse_module(name: &str, source: &str) -> PResult<Module> {
    let lines = lexer::lex(source).map_err(|e| ParseError {
        line: e.line,
        message: e.message,
    })?;
    let mut p = Parser {
        items: items(lines),
        pos: 0,
    };
    let mut m = Module {
        name: name.to_owned(),
        ..Module::default()
    };
    while let Some(item) = p.peek().cloned() {
        let t = &item.toks;
        let mut i = 0;
        let mut private = false;
        while t.get(i).is_some_and(|x| {
            x.is_kw("public")
                || x.is_kw("private")
                || x.is_kw("friend")
                || x.is_kw("static")
                || x.is_kw("global")
        }) {
            private |= t[i].is_kw("private");
            i += 1;
        }
        let head = t.get(i);
        if head.is_some_and(|h| h.is_kw("attribute")) || head.is_some_and(|h| h.is_kw("implements"))
        {
            p.pos += 1;
        } else if head.is_some_and(|h| h.is_kw("option")) {
            if t.get(i + 1).is_some_and(|x| x.is_kw("base")) {
                m.option_base = match t.get(i + 2) {
                    Some(Tok::Integer(v)) => *v,
                    _ => 0,
                };
            }
            p.pos += 1;
        } else if head.is_some_and(|h| h.is_kw("sub") || h.is_kw("function") || h.is_kw("property"))
        {
            let proc = p.parse_proc(i, private)?;
            m.procs.push(proc);
        } else if head.is_some_and(|h| h.is_kw("declare")) {
            m.refused
                .push((item.line, "Declare (calls into Windows libraries)".into()));
            p.pos += 1;
        } else if head.is_some_and(|h| h.is_kw("enum")) {
            p.pos += 1;
            let mut next = 0i64;
            while let Some(it) = p.peek().cloned() {
                p.pos += 1;
                if it.toks.first().is_some_and(|x| x.is_kw("end")) {
                    break;
                }
                if let Some(Tok::Ident(n)) = it.toks.first() {
                    let value = if it.toks.get(1).is_some_and(|x| x.is_p("=")) {
                        let e = Parser::expr_of(&it.toks[2..], it.line)?;
                        if let Expr::Integer(v) = e {
                            next = v;
                        }
                        e
                    } else {
                        Expr::Integer(next)
                    };
                    m.consts.push((n.to_ascii_lowercase(), value));
                    next += 1;
                }
            }
        } else if head.is_some_and(|h| h.is_kw("type")) {
            m.refused
                .push((item.line, "Type (user-defined types)".into()));
            while let Some(it) = p.peek().cloned() {
                p.pos += 1;
                if it.toks.first().is_some_and(|x| x.is_kw("end"))
                    && it.toks.get(1).is_some_and(|x| x.is_kw("type"))
                {
                    break;
                }
            }
        } else if head.is_some_and(|h| h.is_kw("const")) {
            m.consts.extend(Parser::consts(&t[i + 1..], item.line)?);
            p.pos += 1;
        } else if head.is_some_and(|h| h.is_kw("dim")) || i > 0 {
            let start = if head.is_some_and(|h| h.is_kw("dim")) {
                i + 1
            } else {
                i
            };
            if t.get(start).is_some_and(|x| x.is_kw("withevents")) {
                p.pos += 1;
                continue;
            }
            m.vars.extend(Parser::decls(&t[start..], item.line)?);
            p.pos += 1;
        } else {
            return Err(ParseError {
                line: item.line,
                message: format!("unexpected `{}` outside a procedure", tokens_text(t)),
            });
        }
    }
    Ok(m)
}

fn tokens_text(t: &[Tok]) -> String {
    t.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

struct Parser {
    items: Vec<Item>,
    pos: usize,
}

/// A cursor over one statement's tokens.
struct Toks<'a> {
    t: &'a [Tok],
    i: usize,
    line: usize,
}

impl<'a> Toks<'a> {
    fn peek(&self) -> Option<&'a Tok> {
        self.t.get(self.i)
    }

    fn peek_at(&self, n: usize) -> Option<&'a Tok> {
        self.t.get(self.i + n)
    }

    fn next(&mut self) -> Option<&'a Tok> {
        let t = self.t.get(self.i);
        self.i += 1;
        t
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_kw(kw)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eat_p(&mut self, p: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_p(p)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn done(&self) -> bool {
        self.i >= self.t.len()
    }

    fn err<T>(&self, m: impl Into<String>) -> PResult<T> {
        Err(ParseError {
            line: self.line,
            message: m.into(),
        })
    }

    fn expect_p(&mut self, p: &str) -> PResult<()> {
        if self.eat_p(p) {
            Ok(())
        } else {
            let found = self
                .peek()
                .map_or("the end of the line".to_owned(), |t| format!("`{t}`"));
            self.err(format!("`{p}` expected, {found} found"))
        }
    }

    fn ident(&mut self) -> PResult<String> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s.clone()),
            Some(t) => self.err(format!("a name expected, `{t}` found")),
            None => self.err("a name expected"),
        }
    }

    // Expressions, lowest precedence first.
    fn expr(&mut self) -> PResult<Expr> {
        self.binary(0)
    }

    fn binary(&mut self, level: usize) -> PResult<Expr> {
        const LEVELS: [&[&str]; 9] = [
            &["imp"],
            &["eqv"],
            &["xor"],
            &["or"],
            &["and"],
            &[],
            &["=", "<>", "<", ">", "<=", ">=", "like", "is"],
            &["&"],
            &["+", "-"],
        ];
        if level == 5 {
            // `Not` binds looser than comparisons.
            if self.eat_kw("not") {
                let e = self.binary(5)?;
                return Ok(Expr::Unary("not", Box::new(e)));
            }
            return self.binary(6);
        }
        if level >= LEVELS.len() {
            return self.mod_level();
        }
        let mut lhs = self.binary(level + 1)?;
        while let Some(op) = self.peek().and_then(|t| op_of(t, LEVELS[level])) {
            self.i += 1;
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn mod_level(&mut self) -> PResult<Expr> {
        let mut lhs = self.intdiv_level()?;
        while self.eat_kw("mod") {
            let rhs = self.intdiv_level()?;
            lhs = Expr::Binary("mod", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn intdiv_level(&mut self) -> PResult<Expr> {
        let mut lhs = self.mul_level()?;
        while self.eat_p("\\") {
            let rhs = self.mul_level()?;
            lhs = Expr::Binary("\\", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn mul_level(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let op = if self.eat_p("*") {
                "*"
            } else if self.eat_p("/") {
                "/"
            } else {
                break;
            };
            let rhs = self.unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.eat_p("-") {
            let e = self.unary()?;
            return Ok(Expr::Unary("-", Box::new(e)));
        }
        if self.eat_p("+") {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> PResult<Expr> {
        let mut lhs = self.postfix()?;
        while self.eat_p("^") {
            // `-` binds looser than `^`: 2 ^ -1 is allowed.
            let rhs = if self.eat_p("-") {
                Expr::Unary("-", Box::new(self.postfix()?))
            } else {
                self.postfix()?
            };
            lhs = Expr::Binary("^", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat_p(".") {
                let name = self.ident()?;
                e = Expr::Member(Box::new(e), name.to_ascii_lowercase());
            } else if self.eat_p("!") {
                // `Sheets!Data`: the default member with a string key.
                let name = self.ident()?;
                e = Expr::Call(
                    Box::new(e),
                    vec![Arg {
                        name: None,
                        value: Some(Expr::Str(name)),
                    }],
                );
            } else if self.peek().is_some_and(|t| t.is_p("(")) {
                self.i += 1;
                let args = self.args(")")?;
                e = Expr::Call(Box::new(e), args);
            } else {
                break;
            }
        }
        Ok(e)
    }

    /// Arguments up to `close` (consumed); `None` closes at the end of the tokens.
    fn args(&mut self, close: &str) -> PResult<Vec<Arg>> {
        let mut out = Vec::new();
        if self.eat_p(close) {
            return Ok(out);
        }
        loop {
            let name = if matches!(self.peek(), Some(Tok::Ident(_)))
                && self.peek_at(1).is_some_and(|t| t.is_p(":="))
            {
                let n = self.ident()?.to_ascii_lowercase();
                self.i += 1;
                Some(n)
            } else {
                None
            };
            let value = if self.peek().is_some_and(|t| t.is_p(",") || t.is_p(close))
                || (close.is_empty() && self.done())
            {
                None
            } else {
                Some(self.expr()?)
            };
            out.push(Arg { name, value });
            if self.eat_p(",") {
                continue;
            }
            if close.is_empty() {
                if self.done() {
                    break;
                }
                return self.err(format!(
                    "unexpected `{}`",
                    self.peek().map(ToString::to_string).unwrap_or_default()
                ));
            }
            self.expect_p(close)?;
            break;
        }
        Ok(out)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let Some(t) = self.next() else {
            return self.err("an expression expected");
        };
        Ok(match t {
            Tok::Number(n) => Expr::Number(*n),
            Tok::Integer(n) => Expr::Integer(*n),
            Tok::Str(s) => Expr::Str(s.clone()),
            Tok::Date(d) => Expr::Date(parse_date_literal(d).ok_or_else(|| ParseError {
                line: self.line,
                message: format!("cannot read the date #{d}#"),
            })?),
            Tok::Punct("(") => {
                let e = self.expr()?;
                self.expect_p(")")?;
                e
            }
            Tok::Punct(".") => Expr::WithMember(self.ident()?.to_ascii_lowercase()),
            Tok::Ident(s) => {
                let l = s.to_ascii_lowercase();
                match l.as_str() {
                    "true" => Expr::Bool(true),
                    "false" => Expr::Bool(false),
                    "nothing" => Expr::Nothing,
                    "empty" => Expr::Empty,
                    "null" => Expr::Null,
                    "new" => {
                        let ty = self.ident()?.to_ascii_lowercase();
                        Expr::Call(
                            Box::new(Expr::Name("new".into())),
                            vec![Arg {
                                name: None,
                                value: Some(Expr::Str(ty)),
                            }],
                        )
                    }
                    _ if s.starts_with('[') => Expr::Bracket(s[1..s.len() - 1].to_owned()),
                    _ => Expr::Name(l),
                }
            }
            Tok::Punct(p) => return self.err(format!("unexpected `{p}`")),
        })
    }
}

fn op_of(t: &Tok, ops: &[&'static str]) -> Option<&'static str> {
    match t {
        Tok::Punct(p) => ops.iter().find(|o| *o == p).copied(),
        Tok::Ident(s) => ops.iter().find(|o| o.eq_ignore_ascii_case(s)).copied(),
        _ => None,
    }
}

/// `#2026-10-03#`, `#10/3/2026#`, `#14:30#`, `#10/3/2026 2:30 PM#` as a serial.
pub fn parse_date_literal(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date_part, time_part) = if s.contains(':') {
        match s.split_once(' ') {
            Some((d, t)) if !d.contains(':') => (Some(d), Some(t)),
            _ => (None, Some(s)),
        }
    } else {
        (Some(s), None)
    };
    let mut serial = 0.0;
    if let Some(d) = date_part {
        let (y, m, dd) = if d.contains('-') {
            let p: Vec<&str> = d.split('-').collect();
            (
                p.first()?.parse().ok()?,
                p.get(1)?.parse().ok()?,
                p.get(2)?.parse().ok()?,
            )
        } else {
            // VBA date literals are month/day/year.
            let p: Vec<&str> = d.split('/').collect();
            (
                p.get(2)?.parse().ok()?,
                p.first()?.parse().ok()?,
                p.get(1)?.parse().ok()?,
            )
        };
        serial = crate::numfmt::date_to_serial(y, m, dd, false) as f64;
    }
    if let Some(t) = time_part {
        let t = t.trim();
        let (clock, pm) = match t.to_ascii_uppercase() {
            u if u.ends_with("PM") => (t[..t.len() - 2].trim(), Some(true)),
            u if u.ends_with("AM") => (t[..t.len() - 2].trim(), Some(false)),
            _ => (t, None),
        };
        let p: Vec<f64> = clock
            .split(':')
            .map(|x| x.trim().parse().ok())
            .collect::<Option<_>>()?;
        let mut h = *p.first()?;
        if let Some(pm) = pm {
            h %= 12.0;
            if pm {
                h += 12.0;
            }
        }
        serial +=
            (h * 3600.0 + p.get(1).unwrap_or(&0.0) * 60.0 + p.get(2).unwrap_or(&0.0)) / 86_400.0;
    }
    Some(serial)
}

impl Parser {
    fn peek(&self) -> Option<&Item> {
        self.items.get(self.pos)
    }

    fn expr_of(t: &[Tok], line: usize) -> PResult<Expr> {
        let mut c = Toks { t, i: 0, line };
        let e = c.expr()?;
        if !c.done() {
            return c.err(format!(
                "unexpected `{}`",
                c.peek().map(ToString::to_string).unwrap_or_default()
            ));
        }
        Ok(e)
    }

    fn consts(t: &[Tok], line: usize) -> PResult<Vec<(String, Expr)>> {
        let mut c = Toks { t, i: 0, line };
        let mut out = Vec::new();
        loop {
            let name = c.ident()?.to_ascii_lowercase();
            if c.eat_kw("as") {
                c.ident()?;
            }
            c.expect_p("=")?;
            out.push((name, c.expr()?));
            if !c.eat_p(",") {
                break;
            }
        }
        Ok(out)
    }

    fn decls(t: &[Tok], line: usize) -> PResult<Vec<Decl>> {
        let mut c = Toks { t, i: 0, line };
        let mut out = Vec::new();
        while !c.done() {
            c.eat_kw("withevents");
            let name = c.ident()?.to_ascii_lowercase();
            let bounds = if c.eat_p("(") {
                let mut b = Vec::new();
                if !c.eat_p(")") {
                    loop {
                        let first = c.expr()?;
                        if c.eat_kw("to") {
                            b.push(Bound {
                                lower: Some(first),
                                upper: c.expr()?,
                            });
                        } else {
                            b.push(Bound {
                                lower: None,
                                upper: first,
                            });
                        }
                        if c.eat_p(")") {
                            break;
                        }
                        c.expect_p(",")?;
                    }
                }
                Some(b)
            } else {
                None
            };
            let mut ty = None;
            let mut new = false;
            if c.eat_kw("as") {
                new = c.eat_kw("new");
                let mut n = c.ident()?.to_ascii_lowercase();
                // `Excel.Range`, `Scripting.Dictionary`.
                while c.eat_p(".") {
                    n = c.ident()?.to_ascii_lowercase();
                }
                // `String * 10`.
                if c.eat_p("*") {
                    c.expr()?;
                }
                ty = Some(n);
            }
            out.push(Decl {
                name,
                bounds,
                ty,
                new,
            });
            if !c.eat_p(",") {
                break;
            }
        }
        if !c.done() {
            return c.err("unexpected tokens after a declaration");
        }
        Ok(out)
    }

    fn parse_proc(&mut self, i: usize, private: bool) -> PResult<Proc> {
        let item = self.items[self.pos].clone();
        self.pos += 1;
        let mut c = Toks {
            t: &item.toks,
            i,
            line: item.line,
        };
        let kind = c.ident()?.to_ascii_lowercase();
        let function = kind == "function" || kind == "property";
        let end_kw = kind.clone();
        if kind == "property" {
            // `Property Get`, `Let`, `Set`: only `Get` is a value.
            c.ident()?;
        }
        let name = c.ident()?;
        let mut params = Vec::new();
        if c.eat_p("(") && !c.eat_p(")") {
            loop {
                let mut p = Param {
                    name: String::new(),
                    by_val: false,
                    optional: None,
                    param_array: false,
                    array: false,
                };
                loop {
                    if c.eat_kw("optional") {
                        p.optional = Some(None);
                    } else if c.eat_kw("byval") {
                        p.by_val = true;
                    } else if c.eat_kw("byref") {
                    } else if c.eat_kw("paramarray") {
                        p.param_array = true;
                    } else {
                        break;
                    }
                }
                p.name = c.ident()?.to_ascii_lowercase();
                if c.eat_p("(") {
                    c.expect_p(")")?;
                    p.array = true;
                }
                if c.eat_kw("as") {
                    c.ident()?;
                    while c.eat_p(".") {
                        c.ident()?;
                    }
                }
                if c.eat_p("=") {
                    p.optional = Some(Some(c.expr()?));
                }
                params.push(p);
                if c.eat_p(")") {
                    break;
                }
                c.expect_p(",")?;
            }
        }
        let body = self.block(&[&["end", &end_kw]])?;
        self.pos += 1;
        Ok(Proc {
            name,
            function,
            private,
            params,
            body,
            line: item.line,
        })
    }

    /// Statements up to one of the terminators (not consumed), each a
    /// sequence of keywords the statement starts with.
    fn block(&mut self, until: &[&[&str]]) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        loop {
            let Some(item) = self.peek().cloned() else {
                let what = until.first().map(|u| u.join(" ")).unwrap_or_default();
                return Err(ParseError {
                    line: self.items.last().map_or(0, |i| i.line),
                    message: format!("`{what}` missing"),
                });
            };
            if until.iter().any(|u| starts_with_kws(&item.toks, u)) {
                return Ok(out);
            }
            self.pos += 1;
            out.push(self.statement(item)?);
        }
    }

    fn statement(&mut self, item: Item) -> PResult<Stmt> {
        let line = item.line;
        let t = &item.toks;
        let mut c = Toks { t, i: 0, line };
        let stmt = |kind| Ok(Stmt { line, kind });
        if t.len() == 2
            && t[1].is_p(":")
            && let Tok::Ident(l) = &t[0]
        {
            return stmt(StmtKind::Label(l.to_ascii_lowercase()));
        }
        let first = c.peek().cloned();
        let kw = match &first {
            Some(Tok::Ident(s)) => s.to_ascii_lowercase(),
            _ => String::new(),
        };
        match kw.as_str() {
            "dim" | "static" | "private" | "public" => {
                c.next();
                if c.eat_kw("const") {
                    return stmt(StmtKind::Const(Self::consts(&t[c.i..], line)?));
                }
                stmt(StmtKind::Dim(Self::decls(&t[c.i..], line)?))
            }
            "const" => stmt(StmtKind::Const(Self::consts(&t[1..], line)?)),
            "redim" => {
                c.next();
                let preserve = c.eat_kw("preserve");
                stmt(StmtKind::ReDim {
                    preserve,
                    decls: Self::decls(&t[c.i..], line)?,
                })
            }
            "set" | "let" => {
                c.next();
                let target = c.postfix()?;
                c.expect_p("=")?;
                let value = c.expr()?;
                if !c.done() {
                    return c.err("unexpected tokens after an assignment");
                }
                stmt(if kw == "set" {
                    StmtKind::Set(target, value)
                } else {
                    StmtKind::Assign(target, value)
                })
            }
            "call" => {
                c.next();
                let e = c.postfix()?;
                let (target, args) = match e {
                    Expr::Call(t, a) => (*t, a),
                    e => (e, Vec::new()),
                };
                stmt(StmtKind::Call(target, args))
            }
            "if" => self.parse_if(item.clone()),
            "for" => {
                c.next();
                if c.eat_kw("each") {
                    let var = c.postfix()?;
                    if !c.eat_kw("in") {
                        return c.err("`In` expected");
                    }
                    let over = c.expr()?;
                    let body = self.block(&[&["next"]])?;
                    self.pos += 1;
                    return stmt(StmtKind::ForEach { var, over, body });
                }
                let var = c.postfix()?;
                c.expect_p("=")?;
                let from = c.expr()?;
                if !c.eat_kw("to") {
                    return c.err("`To` expected");
                }
                let to = c.expr()?;
                let step = if c.eat_kw("step") {
                    Some(c.expr()?)
                } else {
                    None
                };
                let body = self.block(&[&["next"]])?;
                self.pos += 1;
                stmt(StmtKind::For {
                    var,
                    from,
                    to,
                    step,
                    body,
                })
            }
            "do" => {
                c.next();
                let pre = self.loop_test(&mut c)?;
                let body = self.block(&[&["loop"]])?;
                let end = self.items[self.pos].clone();
                self.pos += 1;
                let mut e = Toks {
                    t: &end.toks,
                    i: 1,
                    line: end.line,
                };
                let post = self.loop_test(&mut e)?;
                stmt(StmtKind::Do { pre, post, body })
            }
            "while" => {
                c.next();
                let cond = c.expr()?;
                let body = self.block(&[&["wend"]])?;
                self.pos += 1;
                stmt(StmtKind::Do {
                    pre: Some((false, cond)),
                    post: None,
                    body,
                })
            }
            "select" => {
                c.next();
                c.eat_kw("case");
                let subject = c.expr()?;
                let mut cases = Vec::new();
                let mut otherwise = Vec::new();
                // Skip to the first `Case`.
                let _ = self.block(&[&["case"], &["end", "select"]])?;
                loop {
                    let head = self.items[self.pos].clone();
                    self.pos += 1;
                    if starts_with_kws(&head.toks, &["end", "select"]) {
                        break;
                    }
                    let body = self.block(&[&["case"], &["end", "select"]])?;
                    let mut h = Toks {
                        t: &head.toks,
                        i: 1,
                        line: head.line,
                    };
                    if h.eat_kw("else") {
                        otherwise = body;
                        continue;
                    }
                    let mut list = Vec::new();
                    loop {
                        if h.eat_kw("is") {
                            let op = match h.next() {
                                Some(Tok::Punct(p))
                                    if ["=", "<>", "<", ">", "<=", ">="].contains(p) =>
                                {
                                    *p
                                }
                                _ => return h.err("a comparison expected after `Is`"),
                            };
                            list.push(CaseItem::Is(op, h.expr()?));
                        } else {
                            let a = h.expr()?;
                            if h.eat_kw("to") {
                                list.push(CaseItem::Range(a, h.expr()?));
                            } else {
                                list.push(CaseItem::Value(a));
                            }
                        }
                        if !h.eat_p(",") {
                            break;
                        }
                    }
                    cases.push((list, body));
                }
                stmt(StmtKind::Select {
                    subject,
                    cases,
                    otherwise,
                })
            }
            "with" => {
                c.next();
                let e = c.expr()?;
                let body = self.block(&[&["end", "with"]])?;
                self.pos += 1;
                stmt(StmtKind::With(e, body))
            }
            "exit" => {
                c.next();
                stmt(StmtKind::Exit(c.ident()?.to_ascii_lowercase()))
            }
            "end" if t.len() == 1 => stmt(StmtKind::End),
            "stop" if t.len() == 1 => stmt(StmtKind::End),
            "on" => {
                c.next();
                if !c.eat_kw("error") {
                    return c.err("only `On Error` is supported");
                }
                if c.eat_kw("resume") {
                    return stmt(StmtKind::OnErrorResumeNext);
                }
                if c.eat_kw("goto") {
                    return match c.next() {
                        Some(Tok::Integer(0)) => stmt(StmtKind::OnErrorGoTo(None)),
                        Some(Tok::Ident(l)) => {
                            stmt(StmtKind::OnErrorGoTo(Some(l.to_ascii_lowercase())))
                        }
                        _ => c.err("a label expected"),
                    };
                }
                c.err("`On Error` needs `Resume Next` or `GoTo`")
            }
            "goto" => {
                c.next();
                stmt(StmtKind::GoTo(c.ident()?.to_ascii_lowercase()))
            }
            "resume" => {
                c.next();
                if c.eat_kw("next") {
                    return stmt(StmtKind::Resume(Some("next".into())));
                }
                match c.next() {
                    Some(Tok::Ident(l)) => stmt(StmtKind::Resume(Some(l.to_ascii_lowercase()))),
                    _ => stmt(StmtKind::Resume(None)),
                }
            }
            "debug"
                if t.get(1).is_some_and(|x| x.is_p("."))
                    && t.get(2).is_some_and(|x| x.is_kw("print")) =>
            {
                // `Debug.Print a; b, c`: `;` and `,` both separate.
                let mut args = Vec::new();
                c.i = 3;
                while !c.done() {
                    if c.eat_p(";") || c.eat_p(",") {
                        continue;
                    }
                    args.push(Arg {
                        name: None,
                        value: Some(c.expr()?),
                    });
                }
                stmt(StmtKind::Call(
                    Expr::Member(Box::new(Expr::Name("debug".into())), "print".into()),
                    args,
                ))
            }
            "beep" | "doevents" if t.len() == 1 => stmt(StmtKind::Nothing),
            "else" | "elseif" | "next" | "loop" | "wend" | "case" | "end" => c.err(format!(
                "`{}` without its opening statement",
                tokens_text(t)
            )),
            "open" | "close" | "print" | "write" | "input" | "line" | "kill" | "mkdir"
            | "rmdir" | "chdir" | "filecopy" | "name"
                if !t.get(1).is_some_and(|x| x.is_p("=") || x.is_p(".")) =>
            {
                c.err(format!(
                    "`{}`: file access is not available to macros in Kalem",
                    first.map(|f| f.to_string()).unwrap_or_default()
                ))
            }
            _ => {
                // An assignment or a call.
                let target = c.postfix()?;
                if c.eat_p("=") {
                    let value = c.expr()?;
                    if !c.done() {
                        return c.err(format!(
                            "unexpected `{}`",
                            c.peek().map(ToString::to_string).unwrap_or_default()
                        ));
                    }
                    return stmt(StmtKind::Assign(target, value));
                }
                if c.done() {
                    return stmt(match target {
                        Expr::Call(t, a) => StmtKind::Call(*t, a),
                        e => StmtKind::Call(e, Vec::new()),
                    });
                }
                // `Foo a, b`: arguments without parentheses. `Foo (a), b`
                // read `Foo (a)` as a call with one argument: undo that.
                let (callee, mut args) = match target {
                    Expr::Call(t, a) if c.peek().is_some_and(|x| x.is_p(",")) && a.len() == 1 => {
                        let first = a.into_iter().next().and_then(|a| a.value);
                        c.i += 1;
                        (
                            *t,
                            vec![Arg {
                                name: None,
                                value: first,
                            }],
                        )
                    }
                    e => (e, Vec::new()),
                };
                args.extend(c.args("")?);
                stmt(StmtKind::Call(callee, args))
            }
        }
    }

    fn loop_test(&self, c: &mut Toks<'_>) -> PResult<Option<(bool, Expr)>> {
        if c.eat_kw("while") {
            return Ok(Some((false, c.expr()?)));
        }
        if c.eat_kw("until") {
            return Ok(Some((true, c.expr()?)));
        }
        Ok(None)
    }

    fn parse_if(&mut self, item: Item) -> PResult<Stmt> {
        let line = item.line;
        let t = &item.toks;
        let then = t.iter().position(|x| x.is_kw("then")).ok_or(ParseError {
            line,
            message: "`Then` expected".into(),
        })?;
        let cond = Self::expr_of(&t[1..then], line)?;
        if then + 1 < t.len() {
            // One line: `If c Then a: b Else d`.
            let rest = &t[then + 1..];
            let else_at = top_level_else(rest);
            let (yes, no) = match else_at {
                Some(e) => (&rest[..e], &rest[e + 1..]),
                None => (rest, &[][..]),
            };
            let yes = self.inline(yes, line)?;
            let no = self.inline(no, line)?;
            return Ok(Stmt {
                line,
                kind: StmtKind::If {
                    arms: vec![(cond, yes)],
                    otherwise: no,
                },
            });
        }
        let mut arms = Vec::new();
        let mut otherwise = Vec::new();
        let mut cond = Some(cond);
        loop {
            let body = self.block(&[&["elseif"], &["else"], &["end", "if"], &["endif"]])?;
            let head = self.items[self.pos].clone();
            self.pos += 1;
            if let Some(c) = cond.take() {
                arms.push((c, body));
            } else {
                otherwise = body;
            }
            if starts_with_kws(&head.toks, &["end", "if"])
                || starts_with_kws(&head.toks, &["endif"])
            {
                break;
            }
            if head.toks[0].is_kw("elseif") {
                let th = head
                    .toks
                    .iter()
                    .position(|x| x.is_kw("then"))
                    .unwrap_or(head.toks.len());
                cond = Some(Self::expr_of(&head.toks[1..th], head.line)?);
            } else if head.toks.len() > 1 && head.toks[1].is_kw("if") {
                // `Else If` on one line is `ElseIf` written apart only when
                // a `Then` ends it; otherwise it opens a nested block.
                return Err(ParseError {
                    line: head.line,
                    message: "write `ElseIf` as one word".into(),
                });
            }
        }
        Ok(Stmt {
            line,
            kind: StmtKind::If { arms, otherwise },
        })
    }

    /// Statements of a one-line `If`, split at `:`.
    fn inline(&mut self, t: &[Tok], line: usize) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        for part in t.split(|x| x.is_p(":")) {
            if part.is_empty() {
                continue;
            }
            let item = Item {
                line,
                toks: part.to_vec(),
            };
            if part[0].is_kw("if") && part.iter().any(|x| x.is_kw("then")) {
                out.push(self.parse_if(Item {
                    line,
                    toks: t[t.len() - part.len()..].to_vec(),
                })?);
                break;
            }
            out.push(self.statement(item)?);
        }
        Ok(out)
    }
}

/// The `Else` of a one-line `If`, not one of a nested one-line `If`.
fn top_level_else(t: &[Tok]) -> Option<usize> {
    let mut depth = 0;
    for (i, x) in t.iter().enumerate() {
        if x.is_kw("if") {
            depth += 1;
        } else if x.is_kw("else") {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

fn starts_with_kws(t: &[Tok], kws: &[&str]) -> bool {
    kws.iter()
        .enumerate()
        .all(|(i, k)| t.get(i).is_some_and(|x| x.is_kw(k)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_parses() {
        let src = r#"Attribute VB_Name = "Module1"
Option Explicit
Private total As Long
Const RATE = 0.18
Public Enum Color
    Red = 1
    Green
End Enum

Sub Fill()
    Dim i As Long, s As String, a(1 To 3) As Double
    For i = 1 To 10 Step 2
        Cells(i, 1).Value = i * RATE
        If i > 5 Then Exit For Else s = s & i
    Next i
    With Worksheets("Data")
        .Range("A1").Value = "x"
    End With
    Select Case total
        Case 1, 2: s = "low"
        Case 3 To 5, Is > 100
            s = "mid"
        Case Else
            s = "?"
    End Select
    Do While i > 0: i = i - 1: Loop
    On Error Resume Next
    MsgBox "Done " & s, vbInformation, Title:="Kalem"
    Call Helper(1, , 3)
End Sub

Function Helper(a, Optional b = 2, Optional c As Long) As Long
    Helper = a + b + c
End Function
"#;
        let m = parse_module("Module1", src).unwrap();
        assert_eq!(m.procs.len(), 2);
        assert_eq!(m.vars[0].name, "total");
        assert_eq!(m.consts.len(), 3);
        assert_eq!(m.consts[2], ("green".into(), Expr::Integer(2)));
        let body = &m.procs[0].body;
        assert!(matches!(body[1].kind, StmtKind::For { .. }));
        assert!(matches!(body[3].kind, StmtKind::Select { ref cases, .. } if cases.len() == 2));
        let StmtKind::Call(_, args) = &body[6].kind else {
            panic!("{:?}", body[6])
        };
        assert_eq!(args.len(), 3);
        assert_eq!(args[2].name.as_deref(), Some("title"));
        let StmtKind::Call(_, args) = &body[7].kind else {
            panic!()
        };
        assert!(args[1].value.is_none());
        assert_eq!(m.procs[1].params[1].optional, Some(Some(Expr::Integer(2))));
    }

    #[test]
    fn precedence() {
        let e = Parser::expr_of(
            &lexer::lex("a = 1 + 2 * 3 ^ 2 And Not b Or c & \"x\" = d").unwrap()[0].toks,
            1,
        )
        .unwrap();
        let Expr::Binary("or", lhs, _) = e else {
            panic!("{e:?}")
        };
        assert!(matches!(*lhs, Expr::Binary("and", _, _)));
        let e = Parser::expr_of(&lexer::lex("-2 ^ 2").unwrap()[0].toks, 1).unwrap();
        assert!(matches!(e, Expr::Unary("-", _)));
        let e = Parser::expr_of(&lexer::lex("7 \\ 2 Mod 3").unwrap()[0].toks, 1).unwrap();
        assert!(matches!(e, Expr::Binary("mod", _, _)));
    }

    #[test]
    fn dates_and_errors() {
        assert_eq!(parse_date_literal("2023-03-15"), Some(45_000.0));
        assert_eq!(parse_date_literal("3/15/2023 12:00 PM"), Some(45_000.5));
        assert!(parse_module("M", "Sub A()\n  Open \"x\" For Input As #1\nEnd Sub").is_err());
        assert!(parse_module("M", "Sub A()\n  If x Then\nEnd Sub").is_err());
    }
}
