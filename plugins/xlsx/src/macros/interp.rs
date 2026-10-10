//! The interpreter: statements run over the syntax tree, the workbook
//! reached through [`super::excel`].

use std::collections::HashMap;
use std::rc::Rc;

use super::ast::*;
use super::builtins;
use super::value::*;
use super::{Limits, MacroHost, RunReport};
use crate::workbook::Workbook;

/// How a block ended.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Flow {
    Next,
    /// `Exit Sub`, `Exit For`… with the keyword.
    Exit(String),
    /// `GoTo label`, also `Resume label`.
    GoTo(String),
    /// `Resume Next` in an error handler: on after the statement that
    /// failed.
    ResumeNext,
    /// `Resume` in an error handler: the statement that failed again.
    Resume,
    /// `End`.
    End,
}

#[derive(Debug, Clone, PartialEq)]
enum OnErr {
    Off,
    ResumeNext,
    GoTo(String),
}

/// A procedure's run.
pub(crate) struct Frame {
    locals: Scope,
    with: Vec<V>,
    on_error: OnErr,
    in_handler: bool,
    /// The function's name and its result variable.
    result: Option<(String, Slot)>,
    /// The module the procedure is in.
    pub(crate) module: usize,
    /// The procedure (its module and index), whose labels an error
    /// handler starts at.
    proc: Option<(Rc<Module>, usize)>,
}

/// A procedure's place: module and index.
type ProcRef = (usize, usize);

/// The interpreter's state over one run.
pub struct Interp<'a> {
    pub(crate) wb: &'a mut Workbook,
    pub(crate) host: &'a mut dyn MacroHost,
    modules: Vec<Rc<Module>>,
    globals: Scope,
    consts: HashMap<String, V>,
    procs: HashMap<String, Vec<ProcRef>>,
    limits: Limits,
    steps: u64,
    started: crate::time::Instant,
    depth: usize,
    /// The active sheet and cell (zero-based).
    pub(crate) active_sheet: usize,
    pub(crate) active_cell: (u32, u32),
    /// The sheet each document module stands for.
    pub(crate) doc_sheets: Vec<Option<usize>>,
    /// `Err`.
    pub(crate) err: (i64, String, String),
    pub(crate) report: RunReport,
    pub(crate) line: usize,
    /// The module running now, for messages.
    pub(crate) cur_module: usize,
}

impl std::fmt::Debug for Interp<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interp")
            .field("steps", &self.steps)
            .field("line", &self.line)
            .finish_non_exhaustive()
    }
}

fn need_obj(v: &V) -> R<&Obj> {
    match v {
        V::Obj(o) => Ok(o),
        V::Nothing => Err(RtError::new(
            91,
            "Object variable or With block variable not set",
        )),
        _ => Err(RtError::new(424, "Object required")),
    }
}

impl<'a> Interp<'a> {
    pub(crate) fn new(
        wb: &'a mut Workbook,
        host: &'a mut dyn MacroHost,
        modules: Vec<Module>,
        limits: Limits,
    ) -> R<Self> {
        let modules: Vec<Rc<Module>> = modules.into_iter().map(Rc::new).collect();
        let mut procs: HashMap<String, Vec<ProcRef>> = HashMap::new();
        for (mi, m) in modules.iter().enumerate() {
            for (pi, p) in m.procs.iter().enumerate() {
                procs
                    .entry(p.name.to_ascii_lowercase())
                    .or_default()
                    .push((mi, pi));
            }
        }
        // Document modules: a sheet's code name, then `SheetN` by position.
        let mut code_names: Vec<(String, usize)> = Vec::new();
        for i in 0..wb.sheets().len() {
            if let Ok(s) = wb.sheet(i)
                && let Some(c) = &s.code_name
            {
                code_names.push((c.to_ascii_lowercase(), i));
            }
        }
        let doc_sheets = modules
            .iter()
            .map(|m| {
                let n = m.name.to_ascii_lowercase();
                code_names.iter().find(|(c, _)| *c == n).map(|(_, i)| *i)
            })
            .collect();
        let mut it = Self {
            wb,
            host,
            modules,
            globals: Scope::new(),
            consts: HashMap::new(),
            procs,
            limits,
            steps: 0,
            started: crate::time::Instant::now(),
            depth: 0,
            active_sheet: 0,
            active_cell: (0, 0),
            doc_sheets,
            err: (0, String::new(), String::new()),
            report: RunReport::default(),
            line: 0,
            cur_module: 0,
        };
        it.active_sheet = (0..it.wb.sheets().len())
            .find(|&i| it.wb.sheets()[i].kind == crate::workbook::SheetKind::Worksheet)
            .unwrap_or(0);
        // Module-level constants and variables.
        let mods = it.modules.clone();
        for (mi, m) in mods.iter().enumerate() {
            let mut f = it.frame(mi, None);
            for (name, e) in &m.consts {
                let v = it.eval(&mut f, e)?;
                it.consts.insert(name.clone(), v);
            }
            for d in &m.vars {
                let v = it.initial(&mut f, d)?;
                it.globals.insert(d.name.clone(), slot(v));
            }
        }
        Ok(it)
    }

    fn frame(&self, module: usize, result: Option<(String, Slot)>) -> Frame {
        Frame {
            locals: Scope::new(),
            with: Vec::new(),
            on_error: OnErr::Off,
            in_handler: false,
            result,
            module,
            proc: None,
        }
    }

    /// Stops the run when the host could not answer a dialog now.
    pub(crate) fn check_stopped(&self) -> R<()> {
        if self.host.stopped() {
            return Err(RtError {
                number: -2,
                description: "waiting for an answer".into(),
                line: self.line,
                fatal: true,
            });
        }
        Ok(())
    }

    /// The module where the run is (or stopped).
    pub(crate) fn current_module(&self) -> String {
        self.modules
            .get(self.cur_module)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }

    fn tick(&mut self) -> R<()> {
        self.steps += 1;
        if self.steps > self.limits.steps {
            return Err(RtError::fatal(format!(
                "the macro was stopped after {} steps (an endless loop?)",
                self.limits.steps
            )));
        }
        if self.steps.is_multiple_of(4096)
            && self.started.elapsed().as_millis() as u64 > self.limits.millis
        {
            return Err(RtError::fatal(format!(
                "the macro was stopped after {} seconds",
                self.limits.millis / 1000
            )));
        }
        Ok(())
    }

    /// Runs a procedure by name (`Module1.Name` or `Name`) with arguments.
    pub(crate) fn run(&mut self, name: &str, args: Vec<V>) -> R<V> {
        let (module, proc_name) = match name.split_once('.') {
            Some((m, p)) => (Some(m.to_ascii_lowercase()), p.to_ascii_lowercase()),
            None => (None, name.to_ascii_lowercase()),
        };
        let found = self
            .procs
            .get(&proc_name)
            .and_then(|v| {
                v.iter()
                    .find(|(mi, _)| {
                        module
                            .as_ref()
                            .is_none_or(|m| self.modules[*mi].name.eq_ignore_ascii_case(m))
                    })
                    .copied()
            })
            .ok_or_else(|| RtError::fatal(format!("no macro named {name}")))?;
        let slots = args.into_iter().map(slot).collect();
        self.call_proc(found, slots)
    }

    fn call_proc(&mut self, (mi, pi): ProcRef, args: Vec<Slot>) -> R<V> {
        if self.depth >= self.limits.depth {
            return Err(RtError::new(28, "Out of stack space"));
        }
        let module = self.modules[mi].clone();
        let proc = &module.procs[pi];
        let result = proc
            .function
            .then(|| (proc.name.to_ascii_lowercase(), slot(V::Empty)));
        let mut f = self.frame(mi, result.clone());
        f.proc = Some((module.clone(), pi));
        let mut args = args.into_iter();
        for p in &proc.params {
            if p.param_array {
                let rest: Vec<V> = args.by_ref().map(|s| s.borrow().clone()).collect();
                let mut arr = Array::new(&[(0, rest.len() as i64 - 1)]);
                arr.data = rest;
                f.locals.insert(p.name.clone(), slot(V::Arr(Box::new(arr))));
                break;
            }
            let given = args.next().filter(|s| !matches!(*s.borrow(), V::Missing));
            let s = match given {
                Some(s) if p.by_val => slot(s.borrow().clone()),
                Some(s) => s,
                None => match &p.optional {
                    Some(Some(default)) => {
                        let v = self.eval(&mut f, default)?;
                        slot(v)
                    }
                    Some(None) => slot(V::Missing),
                    None => {
                        return Err(RtError::new(
                            449,
                            format!("Argument not optional: {}", p.name),
                        ));
                    }
                },
            };
            f.locals.insert(p.name.clone(), s);
        }
        self.depth += 1;
        let caller = std::mem::replace(&mut self.cur_module, mi);
        let r = self.run_body(&mut f, &proc.body);
        if r.is_ok() {
            self.cur_module = caller;
        }
        self.depth -= 1;
        if r? == Flow::End {
            return Err(RtError {
                number: -1,
                description: String::new(),
                line: 0,
                fatal: true,
            });
        }
        Ok(result.map_or(V::Empty, |(_, s)| s.borrow().clone()))
    }

    /// A procedure's body, with its labels for `GoTo`.
    fn run_body(&mut self, f: &mut Frame, body: &[Stmt]) -> R<Flow> {
        let mut start = 0;
        loop {
            match self.block(f, &body[start..])? {
                Flow::GoTo(label) => {
                    let at = body
                        .iter()
                        .position(|s| matches!(&s.kind, StmtKind::Label(l) if *l == label))
                        .ok_or_else(|| {
                            RtError::fatal(format!(
                                "label {label} not found (labels inside blocks are not supported)"
                            ))
                        })?;
                    start = at + 1;
                }
                Flow::End => return Ok(Flow::End),
                _ => return Ok(Flow::Next),
            }
        }
    }

    fn block(&mut self, f: &mut Frame, body: &[Stmt]) -> R<Flow> {
        let mut i = 0;
        while let Some(s) = body.get(i) {
            self.tick()?;
            self.line = s.line;
            match self.stmt(f, s) {
                Ok(Flow::Next) => {}
                Ok(flow) => return Ok(flow),
                Err(mut e) => {
                    if e.line == 0 {
                        e.line = s.line;
                    }
                    if e.fatal {
                        return Err(e);
                    }
                    match f.on_error.clone() {
                        OnErr::ResumeNext => {
                            self.err = (e.number, e.description, "VBAProject".into())
                        }
                        OnErr::GoTo(l) if !f.in_handler => {
                            self.err = (e.number, e.description, "VBAProject".into());
                            f.in_handler = true;
                            match self.handle(f, &l)? {
                                Flow::ResumeNext => {}
                                // The failed statement again.
                                Flow::Resume => continue,
                                // The handler ran to `End Sub`: the
                                // procedure ends.
                                Flow::Next => return Ok(Flow::Exit("sub".into())),
                                flow => return Ok(flow),
                            }
                        }
                        _ => return Err(e),
                    }
                }
            }
            i += 1;
        }
        Ok(Flow::Next)
    }

    /// An error taken by `On Error GoTo label`: the handler runs where the
    /// error was, from its label on, so that `Resume Next` goes on after
    /// the statement that failed and `Resume` runs it again, inside the
    /// loops it was in; `Resume label`, `GoTo` and `Exit` leave as they
    /// would from the handler.
    fn handle(&mut self, f: &mut Frame, label: &str) -> R<Flow> {
        let Some((module, pi)) = f.proc.clone() else {
            return Ok(Flow::GoTo(label.to_owned()));
        };
        let body = &module.procs[pi].body;
        let at = body
            .iter()
            .position(|s| matches!(&s.kind, StmtKind::Label(l) if l == label))
            .ok_or_else(|| {
                RtError::fatal(format!(
                    "label {label} not found (labels inside blocks are not supported)"
                ))
            })?;
        self.block(f, &body[at + 1..])
    }

    fn stmt(&mut self, f: &mut Frame, s: &Stmt) -> R<Flow> {
        match &s.kind {
            StmtKind::Dim(decls) => {
                for d in decls {
                    let v = self.initial(f, d)?;
                    f.locals.insert(d.name.clone(), slot(v));
                }
            }
            StmtKind::ReDim { preserve, decls } => {
                for d in decls {
                    let bounds = self.bounds(f, d.bounds.as_deref().unwrap_or(&[]))?;
                    let target = self.var_slot(f, &d.name);
                    let new = match (&*target.borrow(), preserve) {
                        (V::Arr(old), true) => old.resized(&bounds),
                        _ => Array::new(&bounds),
                    };
                    *target.borrow_mut() = V::Arr(Box::new(new));
                }
            }
            StmtKind::Const(list) => {
                for (n, e) in list {
                    let v = self.eval(f, e)?;
                    f.locals.insert(n.clone(), slot(v));
                }
            }
            StmtKind::Assign(target, value) => {
                let v = self.eval(f, value)?;
                // `Let` of an object assigns its default value.
                let v = match v {
                    V::Obj(o) => self.default_value(&o)?,
                    v => v,
                };
                self.assign(f, target, v, false)?;
            }
            StmtKind::Set(target, value) => {
                let v = self.eval(f, value)?;
                if !matches!(v, V::Obj(_) | V::Nothing) {
                    return Err(RtError::new(424, "Object required"));
                }
                self.assign(f, target, v, true)?;
            }
            StmtKind::Call(target, args) => {
                self.call_expr(f, target, args, true)?;
            }
            StmtKind::If { arms, otherwise } => {
                for (cond, body) in arms {
                    let c = self.eval(f, cond)?;
                    if matches!(c, V::Null) {
                        continue;
                    }
                    if to_bool(&c)? {
                        return self.block(f, body);
                    }
                }
                return self.block(f, otherwise);
            }
            StmtKind::For {
                var,
                from,
                to,
                step,
                body,
            } => {
                let from = self.eval(f, from)?;
                let to = to_num(&self.eval(f, to)?)?;
                let step = match step {
                    Some(e) => to_num(&self.eval(f, e)?)?,
                    None => 1.0,
                };
                let ints = matches!(from, V::Int(_)) && step.fract() == 0.0;
                let mut i = to_num(&from)?;
                loop {
                    if (step >= 0.0 && i > to) || (step < 0.0 && i < to) {
                        break;
                    }
                    self.assign(
                        f,
                        var,
                        if ints { V::Int(i as i64) } else { V::Num(i) },
                        false,
                    )?;
                    match self.block(f, body)? {
                        Flow::Next => {}
                        Flow::Exit(k) if k == "for" => break,
                        other => return Ok(other),
                    }
                    self.tick()?;
                    // The counter as the body left it.
                    i = to_num(&self.eval(f, var)?)? + step;
                }
            }
            StmtKind::ForEach { var, over, body } => {
                let items = self.eval(f, over)?;
                let elements = self.elements(items)?;
                let objects = elements.iter().any(|e| matches!(e, V::Obj(_)));
                for e in elements {
                    self.assign(f, var, e, objects)?;
                    match self.block(f, body)? {
                        Flow::Next => {}
                        Flow::Exit(k) if k == "for" => break,
                        other => return Ok(other),
                    }
                }
            }
            StmtKind::Do { pre, post, body } => loop {
                if let Some((until, c)) = pre {
                    let v = to_bool(&self.eval(f, c)?)?;
                    if v == *until {
                        break;
                    }
                }
                match self.block(f, body)? {
                    Flow::Next => {}
                    Flow::Exit(k) if k == "do" => break,
                    other => return Ok(other),
                }
                if let Some((until, c)) = post {
                    let v = to_bool(&self.eval(f, c)?)?;
                    if v == *until {
                        break;
                    }
                }
                self.tick()?;
            },
            StmtKind::Select {
                subject,
                cases,
                otherwise,
            } => {
                let v = self.eval(f, subject)?;
                for (items, body) in cases {
                    let mut hit = false;
                    for it in items {
                        hit = match it {
                            CaseItem::Value(e) => {
                                let x = self.eval(f, e)?;
                                compare(&v, &x)? == Some(std::cmp::Ordering::Equal)
                            }
                            CaseItem::Range(a, b) => {
                                let (a, b) = (self.eval(f, a)?, self.eval(f, b)?);
                                compare(&v, &a)?.is_some_and(|o| o.is_ge())
                                    && compare(&v, &b)?.is_some_and(|o| o.is_le())
                            }
                            CaseItem::Is(op, e) => {
                                let x = self.eval(f, e)?;
                                cmp_op(op, compare(&v, &x)?)
                            }
                        };
                        if hit {
                            break;
                        }
                    }
                    if hit {
                        return self.block(f, body);
                    }
                }
                return self.block(f, otherwise);
            }
            StmtKind::With(e, body) => {
                let o = self.eval(f, e)?;
                f.with.push(o);
                let r = self.block(f, body);
                f.with.pop();
                return r;
            }
            StmtKind::Exit(k) => return Ok(Flow::Exit(k.clone())),
            StmtKind::OnErrorResumeNext => {
                f.on_error = OnErr::ResumeNext;
                self.err = (0, String::new(), String::new());
            }
            StmtKind::OnErrorGoTo(l) => {
                f.on_error = l.clone().map_or(OnErr::Off, OnErr::GoTo);
                f.in_handler = false;
                self.err = (0, String::new(), String::new());
            }
            StmtKind::GoTo(l) => return Ok(Flow::GoTo(l.clone())),
            StmtKind::Label(_) | StmtKind::Nothing => {}
            StmtKind::Resume(_) if !f.in_handler => {
                return Err(RtError::new(20, "Resume without error"));
            }
            StmtKind::Resume(target) => {
                f.in_handler = false;
                self.err = (0, String::new(), String::new());
                return Ok(match target.as_deref() {
                    Some("next") => Flow::ResumeNext,
                    None => Flow::Resume,
                    Some(l) => Flow::GoTo(l.to_owned()),
                });
            }
            StmtKind::End => return Ok(Flow::End),
        }
        Ok(Flow::Next)
    }

    fn bounds(&mut self, f: &mut Frame, b: &[Bound]) -> R<Vec<(i64, i64)>> {
        let base = self.modules[f.module].option_base;
        b.iter()
            .map(|b| {
                let lower = match &b.lower {
                    Some(e) => to_int(&self.eval(f, e)?)?,
                    None => base,
                };
                Ok((lower, to_int(&self.eval(f, &b.upper)?)?))
            })
            .collect()
    }

    /// A declared variable's first value: an array, a typed zero, `Nothing`.
    fn initial(&mut self, f: &mut Frame, d: &Decl) -> R<V> {
        if let Some(b) = &d.bounds {
            let bounds = self.bounds(f, b)?;
            return Ok(V::Arr(Box::new(Array::new(&bounds))));
        }
        if d.new {
            return self.new_object(d.ty.as_deref().unwrap_or(""));
        }
        Ok(match d.ty.as_deref() {
            Some("integer" | "long" | "byte" | "longlong" | "longptr") => V::Int(0),
            Some("double" | "single" | "currency" | "decimal") => V::Num(0.0),
            Some("string") => V::Str(String::new()),
            Some("boolean") => V::Bool(false),
            Some("date") => V::Date(0.0),
            Some("variant") | None => V::Empty,
            Some(_) => V::Nothing,
        })
    }

    pub(crate) fn new_object(&mut self, ty: &str) -> R<V> {
        match ty.to_ascii_lowercase().as_str() {
            "collection" => Ok(V::Obj(Obj::Collection(Default::default()))),
            "dictionary" | "scripting.dictionary" => {
                Ok(V::Obj(Obj::Dictionary(Default::default())))
            }
            other => Err(RtError::fatal(format!(
                "creating `{other}` objects is not available to macros in Kalem"
            ))),
        }
    }

    /// The elements `For Each` walks.
    fn elements(&mut self, v: V) -> R<Vec<V>> {
        match v {
            V::Arr(a) => Ok(a.data),
            V::Obj(o) => super::excel::elements(self, &o),
            _ => Err(RtError::new(
                92,
                "For loop not initialized: For Each needs an array or a collection",
            )),
        }
    }

    /// The variable `name` in scope, created as a local when unknown.
    fn var_slot(&mut self, f: &mut Frame, name: &str) -> Slot {
        if let Some((n, s)) = &f.result
            && n == name
        {
            return s.clone();
        }
        if let Some(s) = f.locals.get(name) {
            return s.clone();
        }
        if let Some(s) = self.globals.get(name) {
            return s.clone();
        }
        let s = slot(V::Empty);
        f.locals.insert(name.to_owned(), s.clone());
        s
    }

    fn find_var(&self, f: &Frame, name: &str) -> Option<Slot> {
        if let Some((n, s)) = &f.result
            && n == name
        {
            return Some(s.clone());
        }
        f.locals
            .get(name)
            .or_else(|| self.globals.get(name))
            .cloned()
    }

    fn assign(&mut self, f: &mut Frame, target: &Expr, v: V, set: bool) -> R<()> {
        match target {
            Expr::Name(n) => {
                if self.find_var(f, n).is_none() && !set && self.global_object(f, n).is_some() {
                    // `ActiveCell = 5`: the object's default member.
                    let o = self.eval(f, target)?;
                    return super::excel::set_default(self, need_obj(&o)?, Vec::new(), v);
                }
                let s = self.var_slot(f, n);
                *s.borrow_mut() = v;
                Ok(())
            }
            Expr::Call(callee, args) => {
                if let Expr::Name(n) = &**callee
                    && let Some(s) = self.find_var(f, n)
                {
                    let idx = self.arg_values(f, args)?;
                    let holder = s.borrow().clone();
                    return match holder {
                        V::Arr(_) => {
                            let ix: Vec<i64> = idx.iter().map(to_int).collect::<R<_>>()?;
                            let mut b = s.borrow_mut();
                            let V::Arr(arr) = &mut *b else { unreachable!() };
                            let o = arr
                                .offset(&ix)
                                .ok_or_else(|| RtError::new(9, "Subscript out of range"))?;
                            arr.data[o] = v;
                            Ok(())
                        }
                        V::Obj(o) => super::excel::set_default(self, &o, idx, v),
                        _ => Err(RtError::new(9, "Subscript out of range")),
                    };
                }
                if let Some((obj, m)) = member_parts(callee) {
                    let o = self.eval_member_object(f, obj)?;
                    let idx = self.arg_values(f, args)?;
                    if super::excel::settable(&o, m) {
                        return super::excel::set(self, &o, m, idx, v);
                    }
                    let target = super::excel::get(self, &o, m, idx)?;
                    return super::excel::set_default(self, need_obj(&target)?, Vec::new(), v);
                }
                let o = self.eval(f, target)?;
                super::excel::set_default(self, need_obj(&o)?, Vec::new(), v)
            }
            Expr::Member(obj, m) => {
                let o = self.eval_member_object(f, obj)?;
                super::excel::set(self, &o, m, Vec::new(), v)
            }
            Expr::WithMember(m) => {
                let o = f
                    .with
                    .last()
                    .cloned()
                    .ok_or_else(|| RtError::new(91, "`.member` outside With"))?;
                super::excel::set(self, need_obj(&o)?, m, Vec::new(), v)
            }
            _ => Err(RtError::fatal("this cannot be assigned to")),
        }
    }

    /// The object before `.member`: `With`'s, or an expression's.
    fn eval_member_object(&mut self, f: &mut Frame, obj: &Expr) -> R<Obj> {
        let v = self.eval(f, obj)?;
        need_obj(&v).cloned()
    }

    fn arg_values(&mut self, f: &mut Frame, args: &[Arg]) -> R<Vec<V>> {
        args.iter()
            .map(|a| match &a.value {
                Some(e) => self.eval(f, e),
                None => Ok(V::Missing),
            })
            .collect()
    }

    /// Arguments by position, named ones placed by the callee's parameter
    /// names.
    fn named_args(&mut self, f: &mut Frame, args: &[Arg], names: &[&str]) -> R<Vec<V>> {
        let mut out: Vec<V> = Vec::new();
        for a in args.iter().filter(|a| a.name.is_none()) {
            out.push(match &a.value {
                Some(e) => self.eval(f, e)?,
                None => V::Missing,
            });
        }
        for a in args.iter().filter(|a| a.name.is_some()) {
            let n = a.name.as_deref().unwrap_or_default();
            let Some(pos) = names.iter().position(|p| p.eq_ignore_ascii_case(n)) else {
                return Err(RtError::new(448, format!("Named argument not found: {n}")));
            };
            if out.len() <= pos {
                out.resize(pos + 1, V::Missing);
            }
            out[pos] = match &a.value {
                Some(e) => self.eval(f, e)?,
                None => V::Missing,
            };
        }
        Ok(out)
    }

    /// Evaluates a call, as a statement (`stmt`) or for its value.
    fn call_expr(&mut self, f: &mut Frame, target: &Expr, args: &[Arg], stmt: bool) -> R<V> {
        match target {
            Expr::Name(n) => self.call_name(f, n, args, stmt),
            Expr::Member(obj, m) => {
                // `Module1.Proc`.
                if let Expr::Name(mn) = &**obj
                    && self.find_var(f, mn).is_none()
                    && let Some(mi) = self
                        .modules
                        .iter()
                        .position(|x| x.name.eq_ignore_ascii_case(mn))
                    && self.doc_sheets[mi].is_none()
                    && let Some(&p) = self
                        .procs
                        .get(m)
                        .and_then(|v| v.iter().find(|(x, _)| *x == mi))
                {
                    let slots = self.arg_slots(f, args)?;
                    return self.call_proc(p, slots);
                }
                let o = self.eval_member_object(f, obj)?;
                let names = super::excel::param_names(&o, m);
                let a = self.named_args(f, args, names)?;
                super::excel::get(self, &o, m, a)
            }
            Expr::WithMember(m) => {
                let o = f
                    .with
                    .last()
                    .cloned()
                    .ok_or_else(|| RtError::new(91, "`.member` outside With"))?;
                let o = need_obj(&o)?.clone();
                let names = super::excel::param_names(&o, m);
                let a = self.named_args(f, args, names)?;
                super::excel::get(self, &o, m, a)
            }
            other => {
                let v = self.eval(f, other)?;
                let a = self.arg_values(f, args)?;
                self.index(v, a)
            }
        }
    }

    /// Arguments as variables for a procedure: a plain variable is passed
    /// by reference.
    fn arg_slots(&mut self, f: &mut Frame, args: &[Arg]) -> R<Vec<Slot>> {
        args.iter()
            .map(|a| match &a.value {
                Some(Expr::Name(n)) if self.find_var(f, n).is_some() => Ok(self.var_slot(f, n)),
                Some(e) => Ok(slot(self.eval(f, e)?)),
                None => Ok(slot(V::Missing)),
            })
            .collect()
    }

    fn call_name(&mut self, f: &mut Frame, n: &str, args: &[Arg], stmt: bool) -> R<V> {
        // Inside a function its name is its result, but with arguments a
        // recursive call.
        let recursive = !args.is_empty()
            && f.result.as_ref().is_some_and(|(r, _)| r == n)
            && !f.locals.contains_key(n);
        if !recursive && let Some(s) = self.find_var(f, n) {
            let v = s.borrow().clone();
            if args.is_empty() && stmt {
                // A variable used as a statement: an object's default method is not a thing.
                return Err(RtError::fatal(format!(
                    "`{n}` is a variable, not a procedure"
                )));
            }
            let a = self.arg_values(f, args)?;
            return self.index(v, a);
        }
        if let Some(cands) = self.procs.get(n) {
            // The caller's module first.
            let p = cands
                .iter()
                .find(|(mi, _)| *mi == f.module)
                .or(cands.first())
                .copied()
                .expect("non-empty");
            let slots = self.arg_slots(f, args)?;
            return self.call_proc(p, slots);
        }
        if let Some(v) = self.intrinsic(f, n, args)? {
            return Ok(v);
        }
        if builtins::known(n) {
            let a = self.arg_values(f, args)?;
            return builtins::call(n, &a).expect("known");
        }
        if let Some(o) = self.global_object(f, n) {
            let names = super::excel::param_names(&o, "");
            let a = self.named_args(f, args, names)?;
            if a.is_empty() {
                return Ok(V::Obj(o));
            }
            return super::excel::get_default(self, &o, a);
        }
        Err(RtError::fatal(format!("Sub or Function not defined: {n}")))
    }

    /// Functions that need the host or the workbook.
    fn intrinsic(&mut self, f: &mut Frame, n: &str, args: &[Arg]) -> R<Option<V>> {
        Ok(Some(match n {
            "msgbox" => {
                let a = self.named_args(f, args, &["prompt", "buttons", "title"])?;
                let prompt = to_str(a.first().unwrap_or(&V::Empty))?;
                let buttons = a
                    .get(1)
                    .filter(|v| !matches!(v, V::Missing))
                    .map_or(Ok(0), to_int)?;
                let title = match a.get(2) {
                    Some(V::Missing) | None => "Microsoft Excel".to_owned(),
                    Some(v) => to_str(v)?,
                };
                let answer = self.host.msg_box(&prompt, buttons, &title);
                self.check_stopped()?;
                self.report.messages.push(prompt.clone());
                V::Int(answer)
            }
            "inputbox" => {
                let a = self.named_args(f, args, &["prompt", "title", "default"])?;
                let s = |i: usize| -> R<String> {
                    match a.get(i) {
                        Some(V::Missing) | None => Ok(String::new()),
                        Some(v) => to_str(v),
                    }
                };
                let answer = self.host.input_box(&s(0)?, &s(1)?, &s(2)?);
                self.check_stopped()?;
                V::Str(answer.unwrap_or_default())
            }
            "createobject" => {
                let a = self.arg_values(f, args)?;
                let class = to_str(a.first().unwrap_or(&V::Empty))?;
                return self.new_object(&class).map(Some);
            }
            "new" => {
                let a = self.arg_values(f, args)?;
                return self
                    .new_object(&to_str(a.first().unwrap_or(&V::Empty))?)
                    .map(Some);
            }
            "getobject" | "shell" | "environ" | "dir" | "kill" | "filecopy" | "mkdir" | "rmdir"
            | "chdir" | "curdir" | "filelen" | "filedatetime" | "getattr" | "setattr"
            | "sendkeys" | "appactivate" | "callbyname" | "freefile" | "loadpicture"
            | "savesetting" | "getsetting" | "deletesetting" => {
                return Err(RtError::fatal(format!(
                    "`{n}` reaches outside the workbook and is not available to macros in Kalem"
                )));
            }
            "evaluate" => {
                let a = self.arg_values(f, args)?;
                let text = to_str(a.first().unwrap_or(&V::Empty))?;
                super::excel::evaluate(self, self.active_sheet, &text)?
            }
            "doevents" => V::Int(0),
            _ => return Ok(None),
        }))
    }

    /// Names that stand for objects without a variable: `ActiveSheet`,
    /// `Range` used bare, a sheet's code name.
    fn global_object(&self, f: &Frame, n: &str) -> Option<Obj> {
        let doc = self.doc_sheets.get(f.module).copied().flatten();
        Some(match n {
            "application" => Obj::Application,
            "thisworkbook" | "activeworkbook" => Obj::Workbook,
            "workbooks" => Obj::Workbooks,
            "worksheets" | "sheets" => Obj::Sheets,
            "activesheet" => Obj::Sheet(self.active_sheet),
            "me" => doc.map_or(Obj::Workbook, Obj::Sheet),
            "activecell" | "selection" => {
                let (r, c) = self.active_cell;
                Obj::Range(RangeRef {
                    sheet: self.active_sheet,
                    r0: r,
                    c0: c,
                    r1: r,
                    c1: c,
                })
            }
            // Unqualified: a sheet module's own sheet, the active one elsewhere.
            "range" => Obj::Sheet(doc.unwrap_or(self.active_sheet)),
            "cells" => Obj::Range(super::excel::sheet_range(doc.unwrap_or(self.active_sheet))),
            "rows" => Obj::RowsOf(super::excel::sheet_range(doc.unwrap_or(self.active_sheet))),
            "columns" => Obj::ColsOf(super::excel::sheet_range(doc.unwrap_or(self.active_sheet))),
            "worksheetfunction" => Obj::WorksheetFunction,
            "err" => Obj::ErrObject,
            "debug" => Obj::Debug,
            _ => {
                let mi = self
                    .modules
                    .iter()
                    .position(|m| m.name.eq_ignore_ascii_case(n))?;
                return self.doc_sheets[mi].map(Obj::Sheet);
            }
        })
    }

    /// `v(args)`: an array element or an object's default member.
    fn index(&mut self, v: V, args: Vec<V>) -> R<V> {
        if args.is_empty() {
            return Ok(v);
        }
        match v {
            V::Arr(a) => {
                let ix: Vec<i64> = args.iter().map(to_int).collect::<R<_>>()?;
                let o = a
                    .offset(&ix)
                    .ok_or_else(|| RtError::new(9, "Subscript out of range"))?;
                Ok(a.data[o].clone())
            }
            V::Obj(o) => super::excel::get_default(self, &o, args),
            _ => Err(RtError::mismatch()),
        }
    }

    /// An object's default value (`Range` → its value).
    pub(crate) fn default_value(&mut self, o: &Obj) -> R<V> {
        super::excel::default_value(self, o)
    }

    pub(crate) fn eval(&mut self, f: &mut Frame, e: &Expr) -> R<V> {
        Ok(match e {
            Expr::Number(n) => V::Num(*n),
            Expr::Integer(n) => V::Int(*n),
            Expr::Str(s) => V::Str(s.clone()),
            Expr::Date(d) => V::Date(*d),
            Expr::Bool(b) => V::Bool(*b),
            Expr::Nothing => V::Nothing,
            Expr::Empty => V::Empty,
            Expr::Null => V::Null,
            Expr::Bracket(text) => super::excel::evaluate(self, self.active_sheet, text)?,
            Expr::Name(n) => {
                if let Some(s) = self.find_var(f, n) {
                    return Ok(s.borrow().clone());
                }
                if let Some(v) = self.consts.get(n) {
                    return Ok(v.clone());
                }
                if let Some(v) = super::constants::get(n) {
                    return Ok(v);
                }
                return self.call_name(f, n, &[], false);
            }
            Expr::WithMember(m) => {
                let o = f
                    .with
                    .last()
                    .cloned()
                    .ok_or_else(|| RtError::new(91, "`.member` outside With"))?;
                super::excel::get(self, need_obj(&o)?, m, Vec::new())?
            }
            Expr::Member(..) => self.call_expr(f, e, &[], false)?,
            Expr::Call(target, args) => self.call_expr(f, target, args, false)?,
            Expr::Unary(op, x) => {
                let v = self.eval(f, x)?;
                let v = self.scalar(v)?;
                match *op {
                    "-" => match v {
                        V::Null => V::Null,
                        V::Int(i) => V::Int(-i),
                        V::Date(d) => V::Date(-d),
                        v => V::Num(-to_num(&v)?),
                    },
                    _ => match v {
                        V::Null => V::Null,
                        V::Bool(b) => V::Bool(!b),
                        v => V::Int(!to_int(&v)?),
                    },
                }
            }
            Expr::Binary(op, a, b) => {
                let x = self.eval(f, a)?;
                if *op == "is" {
                    let y = self.eval(f, b)?;
                    return Ok(V::Bool(match (&x, &y) {
                        (V::Nothing, V::Nothing) => true,
                        (V::Obj(p), V::Obj(q)) => p == q,
                        _ => false,
                    }));
                }
                let x = self.scalar(x)?;
                let y = self.eval(f, b)?;
                let y = self.scalar(y)?;
                binary(op, &x, &y)?
            }
        })
    }

    /// An object in an expression is its default value.
    fn scalar(&mut self, v: V) -> R<V> {
        match v {
            V::Obj(o) => self.default_value(&o),
            v => Ok(v),
        }
    }
}

fn member_parts(e: &Expr) -> Option<(&Expr, &String)> {
    match e {
        Expr::Member(o, m) => Some((o, m)),
        _ => None,
    }
}

/// Compares two values as VBA's comparison operators do; `None` with Null.
pub(crate) fn compare(a: &V, b: &V) -> R<Option<std::cmp::Ordering>> {
    if matches!(a, V::Null) || matches!(b, V::Null) {
        return Ok(None);
    }
    let numeric = |v: &V| {
        matches!(
            v,
            V::Int(_) | V::Num(_) | V::Date(_) | V::Bool(_) | V::Empty
        )
    };
    match (a, b) {
        (V::Str(x), V::Str(y)) => Ok(Some(x.cmp(y))),
        // Empty is the empty string beside a string (`cell.Value = ""`),
        // zero beside a number.
        (V::Empty, V::Str(y)) => Ok(Some("".cmp(y.as_str()))),
        (V::Str(x), V::Empty) => Ok(Some(x.as_str().cmp(""))),
        (V::Str(s), n) | (n, V::Str(s)) if numeric(n) => {
            let flip = matches!(a, V::Str(_));
            match parse_number(s) {
                Some(v) => {
                    let o = to_num(n)?.partial_cmp(&v);
                    Ok(if flip {
                        o.map(std::cmp::Ordering::reverse)
                    } else {
                        o
                    })
                }
                // A number is less than any text that is not one.
                None => Ok(Some(if flip {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                })),
            }
        }
        (V::Error(x), V::Error(y)) => Ok(Some(x.cmp(y))),
        _ => Ok(to_num(a)?.partial_cmp(&to_num(b)?)),
    }
}

fn cmp_op(op: &str, o: Option<std::cmp::Ordering>) -> bool {
    let Some(o) = o else { return false };
    match op {
        "=" => o.is_eq(),
        "<>" => o.is_ne(),
        "<" => o.is_lt(),
        ">" => o.is_gt(),
        "<=" => o.is_le(),
        _ => o.is_ge(),
    }
}

fn int_result(n: f64, both_int: bool) -> V {
    if both_int && n.fract() == 0.0 && n.abs() < 9e15 {
        V::Int(n as i64)
    } else {
        V::Num(n)
    }
}

/// A binary operator on two values.
pub(crate) fn binary(op: &str, x: &V, y: &V) -> R<V> {
    let both_int = matches!(x, V::Int(_) | V::Bool(_) | V::Empty)
        && matches!(y, V::Int(_) | V::Bool(_) | V::Empty);
    if matches!(op, "=" | "<>" | "<" | ">" | "<=" | ">=") {
        return Ok(match compare(x, y)? {
            None => V::Null,
            o => V::Bool(cmp_op(op, o)),
        });
    }
    if op == "&" {
        let s = |v: &V| {
            if matches!(v, V::Null) {
                Ok(String::new())
            } else {
                to_str(v)
            }
        };
        return Ok(V::Str(s(x)? + &s(y)?));
    }
    if op == "like" {
        return Ok(V::Bool(builtins::like(&to_str(x)?, &to_str(y)?)));
    }
    if matches!(x, V::Null) || matches!(y, V::Null) {
        return Ok(V::Null);
    }
    if let (V::Error(e), _) | (_, V::Error(e)) = (x, y) {
        return Err(RtError::new(
            13,
            format!("Type mismatch (an error value {e} in arithmetic)"),
        ));
    }
    match op {
        "+" => {
            if let (V::Str(a), V::Str(b)) = (x, y) {
                return Ok(V::Str(format!("{a}{b}")));
            }
            let r = to_num(x)? + to_num(y)?;
            Ok(if matches!(x, V::Date(_)) || matches!(y, V::Date(_)) {
                V::Date(r)
            } else {
                int_result(r, both_int)
            })
        }
        "-" => {
            let r = to_num(x)? - to_num(y)?;
            Ok(if matches!(x, V::Date(_)) && !matches!(y, V::Date(_)) {
                V::Date(r)
            } else {
                int_result(r, both_int)
            })
        }
        "*" => Ok(int_result(to_num(x)? * to_num(y)?, both_int)),
        "/" => {
            let d = to_num(y)?;
            if d == 0.0 {
                return Err(RtError::new(11, "Division by zero"));
            }
            Ok(V::Num(to_num(x)? / d))
        }
        "\\" => {
            let d = to_int(y)?;
            if d == 0 {
                return Err(RtError::new(11, "Division by zero"));
            }
            Ok(V::Int(to_int(x)? / d))
        }
        "mod" => {
            let d = to_int(y)?;
            if d == 0 {
                return Err(RtError::new(11, "Division by zero"));
            }
            Ok(V::Int(to_int(x)? % d))
        }
        "^" => Ok(V::Num(to_num(x)?.powf(to_num(y)?))),
        "and" | "or" | "xor" | "eqv" | "imp" => {
            if let (V::Bool(a), V::Bool(b)) = (x, y) {
                return Ok(V::Bool(match op {
                    "and" => *a && *b,
                    "or" => *a || *b,
                    "xor" => a != b,
                    "eqv" => a == b,
                    _ => !*a || *b,
                }));
            }
            let (a, b) = (to_int(x)?, to_int(y)?);
            Ok(V::Int(match op {
                "and" => a & b,
                "or" => a | b,
                "xor" => a ^ b,
                "eqv" => !(a ^ b),
                _ => !a | b,
            }))
        }
        _ => Err(RtError::fatal(format!("operator {op} is not supported"))),
    }
}
