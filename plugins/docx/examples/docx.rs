//! A command line over the plugin's library, for trying it before Kalem
//! lays documents out:
//!
//! ```sh
//! cargo run -p kalem-plugin-docx --example docx -- new     report.docx
//! cargo run -p kalem-plugin-docx --example docx -- info    report.docx
//! cargo run -p kalem-plugin-docx --example docx -- show    report.docx
//! cargo run -p kalem-plugin-docx --example docx -- text    report.docx
//! cargo run -p kalem-plugin-docx --example docx -- outline report.docx
//! cargo run -p kalem-plugin-docx --example docx -- styles  report.docx
//! cargo run -p kalem-plugin-docx --example docx -- paras   report.docx
//! cargo run -p kalem-plugin-docx --example docx -- set     report.docx 3 'New text' [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- type    report.docx 3 0 'Typed ' [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- split   report.docx 3 5 [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- join    report.docx 3 [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- track   report.docx on|off [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- comment report.docx 3 0 5 'Text' [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- reply   report.docx 0 'Text' [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- edit    report.docx 0 'Text' [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- resolve report.docx 0 on|off [-o out.docx]
//! cargo run -p kalem-plugin-docx --example docx -- delete  report.docx 0 [-o out.docx]
//! ```
//!
//! `new` writes a blank document of the file's kind (`.docx`, `.docm`,
//! `.dotx`, `.dotm`), as Kalem's New command makes one; it does not
//! write over a file.
//!
//! In a document that tracks changes, edits are written as tracked
//! changes by the author `KALEM_AUTHOR` names (else "Kalem"); comments
//! and answers are by that author too. `comment` puts a comment on bytes
//! FROM to TO of a paragraph's edit text; `reply` answers the comment of
//! that `w:id`, `edit` gives it new text, `resolve` marks its thread done or open, `delete` takes
//! it away with its answers. Each prints the new comment's `w:id`.
//!
//! Paragraphs are numbered as `paras` lists them: the body's, from 0,
//! tables' included. An edit without `-o` writes the file in place,
//! atomically.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kalem_plugin_docx::flow::{Piece, VBlock, VPara, Vert};
use kalem_plugin_docx::numbering::Suffix;
use kalem_plugin_docx::styles::StyleKind;
use kalem_plugin_docx::{Document, ParaAt, StoryId};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn open(path: &Path) -> Res<Document> {
    let mut doc = Document::open(std::fs::read(path)?)?;
    if let Ok(author) = std::env::var("KALEM_AUTHOR") {
        doc.set_revision_author(&author, None);
    }
    Ok(doc)
}

/// A paragraph with its look in marks: `**bold**`, `_italic_`,
/// `{+inserted+}`, `[-deleted-]`, `^raised^`, `<link|text>`.
fn marked(p: &VPara) -> String {
    let mut s = String::new();
    if let Some((label, suffix, _)) = &p.label {
        s.push_str(label);
        s.push_str(match suffix {
            Suffix::Tab => "\t",
            Suffix::Space => " ",
            Suffix::Nothing => "",
        });
    }
    for r in &p.runs {
        if r.look.hidden {
            continue;
        }
        let mut t = if r.look.caps {
            r.text.to_uppercase()
        } else {
            r.text.clone()
        };
        if matches!(r.piece, Piece::Text) && !t.trim().is_empty() {
            if r.look.bold {
                t = format!("**{t}**");
            }
            if r.look.italic {
                t = format!("_{t}_");
            }
        }
        if r.look.vert == Vert::Super {
            t = format!("^{t}^");
        }
        if let Some(l) = &r.link {
            t = format!("<{l}|{t}>");
        }
        if r.deleted.is_some() {
            t = format!("[-{t}-]");
        } else if r.inserted.is_some() {
            t = format!("{{+{t}+}}");
        }
        s.push_str(&t);
    }
    s
}

fn show_blocks(blocks: &[VBlock], indent: &str) {
    for b in blocks {
        match b {
            VBlock::Para(p) => {
                let at =
                    p.at.as_ref()
                        .map_or("  ".to_owned(), |a| format!("{:>3}", a.index));
                println!(
                    "{indent}{at} {:<16} {}",
                    p.style,
                    marked(p).replace('\n', "⏎")
                );
            }
            VBlock::Table(t) => {
                println!(
                    "{indent}    ┌ table{}",
                    t.style
                        .as_ref()
                        .map_or(String::new(), |s| format!(" ({s})"))
                );
                for (i, row) in t.rows.iter().enumerate() {
                    for (j, c) in row.cells.iter().enumerate() {
                        let fill = c.fill.map_or(String::new(), |f| format!(" #{f:06X}"));
                        let merge = match c.merge {
                            Some(kalem_plugin_docx::props::VMerge::Restart) => " merged↓",
                            Some(kalem_plugin_docx::props::VMerge::Continue) => " (merged)",
                            None => "",
                        };
                        let span = if c.columns > 1 {
                            format!(" ×{}", c.columns)
                        } else {
                            String::new()
                        };
                        println!("{indent}    │ [{i},{j}]{span}{merge}{fill}");
                        show_blocks(&c.blocks, &format!("{indent}    │   "));
                    }
                }
                println!("{indent}    └");
            }
            VBlock::Frame(f) => {
                println!("{indent}    ┌ text box");
                show_blocks(f, &format!("{indent}    │ "));
                println!("{indent}    └");
            }
            VBlock::Placeholder(p) => println!("{indent}    {p}"),
        }
    }
}

fn show(path: &Path) -> Res<()> {
    let doc = open(path)?;
    let v = doc.view();
    if !v.header.is_empty() {
        println!("── header");
        show_blocks(&v.header, "");
    }
    println!("── body");
    show_blocks(&v.body, "");
    if !v.footer.is_empty() {
        println!("── footer");
        show_blocks(&v.footer, "");
    }
    for (title, notes) in [("footnotes", &v.footnotes), ("endnotes", &v.endnotes)] {
        if !notes.is_empty() {
            println!("── {title}");
            for n in notes {
                println!("[{}]", n.mark);
                show_blocks(&n.blocks, "  ");
            }
        }
    }
    if !v.comments.is_empty() {
        println!("── comments");
        for c in &v.comments {
            println!(
                "{} ({}, {}):",
                c.id,
                c.author,
                c.date.as_deref().unwrap_or("no date")
            );
            show_blocks(&c.blocks, "  ");
        }
    }
    Ok(())
}

fn info(path: &Path) -> Res<()> {
    let doc = open(path)?;
    let p = doc.properties();
    println!(
        "{}: {:?}, main part {}",
        path.display(),
        doc.kind(),
        doc.main_part()
    );
    if let Some(t) = &p.title {
        println!("  title: {t}");
    }
    if let Some(c) = &p.creator {
        println!("  author: {c}");
    }
    let v = doc.view();
    let text = v.text();
    println!(
        "  {} paragraphs, {} words, {} sections, {} footnotes, {} endnotes, {} comments",
        v.body_paragraphs().len(),
        text.split_whitespace().count(),
        doc.sections().len(),
        v.footnotes.len(),
        v.endnotes.len(),
        v.comments.len()
    );
    if doc.tracks_changes() {
        println!("  tracks changes");
    }
    if doc.has_vba() {
        println!("  a VBA project, kept");
    }
    println!("  parts:");
    for n in doc.parts() {
        println!("    {n}");
    }
    Ok(())
}

fn styles(path: &Path) -> Res<()> {
    let doc = open(path)?;
    for s in &doc.styles().styles {
        let kind = match s.kind {
            StyleKind::Paragraph => "paragraph",
            StyleKind::Character => "character",
            StyleKind::Table => "table",
            StyleKind::Numbering => "numbering",
        };
        let based = s
            .based_on
            .as_deref()
            .map_or(String::new(), |b| format!(" ← {b}"));
        let def = if s.default { " (default)" } else { "" };
        println!("{:<10} {:<24} {}{based}{def}", kind, s.id, s.name);
    }
    Ok(())
}

fn paras(path: &Path) -> Res<()> {
    let doc = open(path)?;
    // The story with its edit coordinates, which the view leaves out.
    fn walk(blocks: &[VBlock]) {
        for b in blocks {
            match b {
                VBlock::Para(p) => {
                    if let Some(at) = &p.at {
                        println!("{:>4} {:?}", at.index, p.layout.text);
                    }
                }
                VBlock::Table(t) => {
                    for row in &t.rows {
                        for c in &row.cells {
                            walk(&c.blocks);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    walk(&doc.story_view(&StoryId::Body));
    Ok(())
}

fn write_out(doc: &mut Document, path: &Path, out: Option<PathBuf>) -> Res<()> {
    let bytes = doc.save()?;
    let target = out.unwrap_or_else(|| path.to_owned());
    let tmp = target.with_extension("kalem-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &target)?;
    eprintln!(
        "wrote {} (changed: {})",
        target.display(),
        doc.changed_parts().join(", ")
    );
    Ok(())
}

fn at(index: &str) -> Res<ParaAt> {
    Ok(ParaAt {
        story: StoryId::Body,
        index: index.parse()?,
    })
}

fn run(args: &[String]) -> Res<()> {
    let mut args: Vec<String> = args.to_vec();
    let mut out = None;
    if let Some(i) = args.iter().position(|a| a == "-o") {
        out = args.get(i + 1).map(PathBuf::from);
        args.drain(i..(i + 2).min(args.len()));
    }
    let usage = "usage: docx new|info|show|text|outline|styles|paras|set|type|split|join|track|comment|reply|edit|resolve|delete FILE …";
    let cmd = args.first().ok_or(usage)?;
    let path = PathBuf::from(args.get(1).ok_or(usage)?);
    match cmd.as_str() {
        "new" => {
            if path.exists() {
                return Err(format!("{} exists", path.display()).into());
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("docx")
                .to_ascii_lowercase();
            std::fs::write(&path, kalem_plugin_docx::blank::new_document(&ext, &[])?)?;
            Ok(())
        }
        "info" => info(&path),
        "show" => show(&path),
        "text" => {
            print!("{}", open(&path)?.view().text());
            Ok(())
        }
        "outline" => {
            for (t, level) in open(&path)?.view().outline() {
                println!("{}{t}", "  ".repeat(usize::from(level) - 1));
            }
            Ok(())
        }
        "styles" => styles(&path),
        "paras" => paras(&path),
        "set" => {
            let mut doc = open(&path)?;
            let at = at(args.get(2).ok_or("set FILE PARAGRAPH TEXT")?)?;
            let text = args.get(3).ok_or("set FILE PARAGRAPH TEXT")?;
            let len = doc.layout(&at)?.text.len();
            doc.replace(&at, 0..len, text)?;
            write_out(&mut doc, &path, out)
        }
        "type" => {
            let mut doc = open(&path)?;
            let at = at(args.get(2).ok_or("type FILE PARAGRAPH OFFSET TEXT")?)?;
            let off: usize = args
                .get(3)
                .ok_or("type FILE PARAGRAPH OFFSET TEXT")?
                .parse()?;
            doc.replace(
                &at,
                off..off,
                args.get(4).ok_or("type FILE PARAGRAPH OFFSET TEXT")?,
            )?;
            write_out(&mut doc, &path, out)
        }
        "split" => {
            let mut doc = open(&path)?;
            let at = at(args.get(2).ok_or("split FILE PARAGRAPH OFFSET")?)?;
            doc.split(
                &at,
                args.get(3).ok_or("split FILE PARAGRAPH OFFSET")?.parse()?,
            )?;
            write_out(&mut doc, &path, out)
        }
        "track" => {
            let mut doc = open(&path)?;
            let on = match args.get(2).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => return Err("track FILE on|off".into()),
            };
            doc.set_track_changes(on)?;
            write_out(&mut doc, &path, out)
        }
        "join" => {
            let mut doc = open(&path)?;
            doc.join(&at(args.get(2).ok_or("join FILE PARAGRAPH")?)?)?;
            write_out(&mut doc, &path, out)
        }
        "comment" => {
            let use_ = "comment FILE PARAGRAPH FROM TO TEXT";
            let mut doc = open(&path)?;
            let at = at(args.get(2).ok_or(use_)?)?;
            let from: usize = args.get(3).ok_or(use_)?.parse()?;
            let to: usize = args.get(4).ok_or(use_)?.parse()?;
            let id = doc.add_comment((&at, from), (&at, to), args.get(5).ok_or(use_)?)?;
            println!("{id}");
            write_out(&mut doc, &path, out)
        }
        "reply" => {
            let use_ = "reply FILE COMMENT TEXT";
            let mut doc = open(&path)?;
            let id = doc.reply_comment(args.get(2).ok_or(use_)?, args.get(3).ok_or(use_)?)?;
            println!("{id}");
            write_out(&mut doc, &path, out)
        }
        "edit" => {
            let use_ = "edit FILE COMMENT TEXT";
            let mut doc = open(&path)?;
            doc.set_comment_text(args.get(2).ok_or(use_)?, args.get(3).ok_or(use_)?)?;
            write_out(&mut doc, &path, out)
        }
        "resolve" => {
            let use_ = "resolve FILE COMMENT on|off";
            let mut doc = open(&path)?;
            let done = match args.get(3).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => return Err(use_.into()),
            };
            doc.resolve_comment(args.get(2).ok_or(use_)?, done)?;
            write_out(&mut doc, &path, out)
        }
        "delete" => {
            let mut doc = open(&path)?;
            doc.remove_comment(args.get(2).ok_or("delete FILE COMMENT")?)?;
            write_out(&mut doc, &path, out)
        }
        _ => Err(usage.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("docx: {e}");
            ExitCode::FAILURE
        }
    }
}
