//! A block's line as its note draws it away from the cursor, for the lists
//! that show it out of its note (the backlinks panel): a link without its
//! brackets (Obsidian's and Org's by their label), a tag without the
//! brackets of `#[[two words]]`, a block reference as the text of its
//! block, an embed as what it embeds (`↳ page`), and the marks of bold,
//! highlighted, struck and code text left out, as Logseq's linked
//! references read.

use crate::index::Index;
use crate::scan::{self, Flavor, RefKind};

/// How many block references deep a line is drawn: a reference in the
/// text of a block referred to is drawn too, one in that block's text
/// left as written (its links drawn).
const DEPTH: usize = 2;

/// The most characters an entry's text keeps; Kalem's panel ends a
/// longer one in an ellipsis sooner.
const MOST: usize = 240;

/// The first line of block `block` of file `rel` (else of the block at
/// line `at`), its keyword before it, as its note draws it: an entry of
/// the backlinks panel. A page's properties, before its first block, are
/// named so.
pub fn block_line(index: &Index, rel: &str, block: Option<usize>, at: u32) -> String {
    let Some(d) = index.file(rel) else {
        return String::new();
    };
    let b = match block {
        Some(i) => d.scanned.blocks.get(i),
        None => d.scanned.blocks.iter().find(|b| b.line <= at && at < b.end),
    };
    let text = b.map_or(String::new(), |b| {
        let shown = line(index, rel, b.text.trim());
        match b.marker {
            Some(m) => format!("{m} {shown}"),
            None => shown,
        }
    });
    let text = text.trim();
    if text.is_empty() {
        return "(the page's properties)".into();
    }
    if text.chars().count() <= MOST {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(MOST - 1).collect();
    cut.push('…');
    cut
}

/// Line `text` of file `rel` (a block's first line) as its note draws it.
pub fn line(index: &Index, rel: &str, text: &str) -> String {
    let flavor = index.file(rel).map_or(Flavor::LogseqMarkdown, |d| d.flavor);
    shown(index, rel, text, flavor, DEPTH)
}

fn shown(index: &Index, rel: &str, text: &str, flavor: Flavor, depth: usize) -> String {
    let mut out = String::new();
    let mut at = 0;
    for r in scan::references(text, flavor) {
        let (start, end) = (r.start as usize, r.end as usize);
        if start < at || end > text.len() {
            continue;
        }
        out.push_str(&marks(&text[at..start], flavor));
        let raw = &text[start..end];
        // The text of the block `id` names, drawn; `None` when unknown,
        // or deeper than drawn.
        let block = |id: &str| {
            index
                .block(id)
                .filter(|_| depth > 0)
                .map(|(_, b)| b.text.trim().to_string())
                .filter(|t| !t.is_empty())
                .map(|t| shown(index, rel, &t, flavor, depth - 1))
        };
        let drawn = match r.kind {
            RefKind::Tag => format!("#{}", r.target),
            RefKind::Block if flavor != Flavor::Obsidian => {
                block(&r.target).unwrap_or_else(|| raw.to_string())
            }
            RefKind::EmbedBlock => {
                let id = match flavor {
                    Flavor::Obsidian => format!(
                        "{}#^{}",
                        index.resolve(&r.target, rel),
                        r.anchor.as_deref().unwrap_or("").to_lowercase()
                    ),
                    _ => r.target.clone(),
                };
                format!("↳ {}", block(&id).unwrap_or_else(|| r.target.clone()))
            }
            RefKind::EmbedPage => match &r.anchor {
                Some(a) => format!("↳ {} › {a}", r.target),
                None => format!("↳ {}", r.target),
            },
            RefKind::Page | RefKind::Block | RefKind::Property => link_text(raw, flavor),
        };
        out.push_str(&drawn);
        at = end;
    }
    out.push_str(&marks(&text[at..], flavor));
    out
}

/// A link as its note draws it: `[[page]]` as `page`, Obsidian's
/// `[[note|alias]]` as `alias`, Org's `[[target][label]]` as `label`, a
/// Markdown link `[text](note.md)` as `text`.
fn link_text(raw: &str, flavor: Flavor) -> String {
    if let Some(inner) = raw.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")) {
        return match flavor {
            Flavor::LogseqOrg => match inner.split_once("][") {
                Some((_, label)) => label.to_string(),
                None => inner.to_string(),
            },
            Flavor::Obsidian => match inner.split_once('|') {
                Some((_, alias)) => alias.to_string(),
                None => inner.to_string(),
            },
            Flavor::LogseqMarkdown => inner.to_string(),
        };
    }
    if raw.starts_with('[')
        && let Some(close) = raw.find("](")
    {
        return raw[1..close].to_string();
    }
    raw.to_string()
}

/// `text` (no references in it) without the marks its note hides:
/// `**bold**`, `~~struck~~`, Logseq's `^^highlight^^`, Obsidian's
/// `==highlight==`, code between backticks, a link `[text](url)` as its
/// text; in Org, a link `[[url][label]]` as its label.
fn marks(text: &str, flavor: Flavor) -> String {
    if flavor == Flavor::LogseqOrg {
        return org_links(text);
    }
    let mut out = String::new();
    // Code first: what is inside stays as written.
    let mut rest = text;
    while let Some(p) = rest.find('`') {
        let n = rest[p..].bytes().take_while(|&b| b == b'`').count();
        let fence = &rest[p..p + n];
        match rest[p + n..].find(fence) {
            Some(q) => {
                out.push_str(&outside_code(&rest[..p], flavor));
                out.push_str(&rest[p + n..p + n + q]);
                rest = &rest[p + n + q + n..];
            }
            None => break,
        }
    }
    out.push_str(&outside_code(rest, flavor));
    out
}

/// Markdown `text` outside code without its emphasis marks and with its
/// links as their text.
fn outside_code(text: &str, flavor: Flavor) -> String {
    let mut t = markdown_links(text);
    let highlight = if flavor == Flavor::Obsidian {
        "=="
    } else {
        "^^"
    };
    for mark in ["**", "~~", highlight] {
        t = unpair(&t, mark);
    }
    t
}

/// `text` with each pair of `mark` around a text that starts and ends
/// with no space left out.
fn unpair(text: &str, mark: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find(mark) {
        let inner = &rest[p + mark.len()..];
        match inner.find(mark) {
            Some(q) if q > 0 && !inner.starts_with(' ') && !inner[..q].ends_with(' ') => {
                out.push_str(&rest[..p]);
                out.push_str(&inner[..q]);
                rest = &inner[q + mark.len()..];
            }
            _ => {
                out.push_str(&rest[..p + mark.len()]);
                rest = inner;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Markdown links `[text](url)` and pictures `![text](url)` in `text` as
/// their text.
fn markdown_links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find('[') {
        let after = &rest[p + 1..];
        let link = after.find("](").and_then(|c| {
            let url = &after[c + 2..];
            url.find(')').map(|e| (c, c + 2 + e + 1))
        });
        match link {
            Some((close, end)) if !after[..close].contains('[') => {
                let start = if rest[..p].ends_with('!') { p - 1 } else { p };
                out.push_str(&rest[..start]);
                out.push_str(&after[..close]);
                rest = &after[end..];
            }
            _ => {
                out.push_str(&rest[..p + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Org's links in `text` (those to the web, which name no page) as their
/// labels, or their targets without one.
fn org_links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("[[") {
        let after = &rest[p + 2..];
        match after.find("]]") {
            Some(e) => {
                out.push_str(&rest[..p]);
                let inner = &after[..e];
                out.push_str(inner.split_once("][").map_or(inner, |(_, l)| l));
                rest = &after[e + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallbacks, Graph, Kind};
    use crate::files::Memory;

    fn logseq() -> Index {
        let m = Memory::new(&[
            ("/g/logseq/config.edn", "{}"),
            (
                "/g/pages/Kalem.md",
                "- Kalem opens Org and Markdown files as they are.\n  id:: 6512c0de-aaaa-4000-8000-000000000001\n- It refers to ((6512c0de-aaaa-4000-8000-000000000002))\n  id:: 6512c0de-aaaa-4000-8000-000000000003\n",
            ),
            (
                "/g/pages/Other.md",
                "- Its own [[Kalem]] reference\n  id:: 6512c0de-aaaa-4000-8000-000000000002\n",
            ),
            ("/g/pages/Notes.org", "* About [[Kalem][the editor]]\n"),
        ]);
        Index::build(
            &m,
            Graph::load(&m, "/g", Kind::Logseq, &Fallbacks::default()),
        )
    }

    #[test]
    fn a_logseq_line_reads_as_its_note_draws_it() {
        let i = logseq();
        let l = |t: &str| line(&i, "pages/Other.md", t);
        assert_eq!(
            l("First try of [[Editor]] on the [[Book]]'s plugins chapter"),
            "First try of Editor on the Book's plugins chapter"
        );
        assert_eq!(
            l("[[Kalem]] meeting #meeting #[[two words]]"),
            "Kalem meeting #meeting #two words"
        );
        // A block reference as its block's text, references in it drawn.
        assert_eq!(
            l("Kalem in one line: ((6512c0de-aaaa-4000-8000-000000000001))"),
            "Kalem in one line: Kalem opens Org and Markdown files as they are."
        );
        assert_eq!(
            l("Twice: ((6512c0de-aaaa-4000-8000-000000000003))"),
            "Twice: It refers to Its own Kalem reference"
        );
        // One nobody wrote stays as written.
        assert_eq!(
            l("Lost: ((6512c0de-aaaa-4000-8000-00000000ffff))"),
            "Lost: ((6512c0de-aaaa-4000-8000-00000000ffff))"
        );
        // Embeds as what they embed.
        assert_eq!(l("{{embed [[Kalem]]}}"), "↳ Kalem");
        assert_eq!(
            l("{{embed ((6512c0de-aaaa-4000-8000-000000000001))}}"),
            "↳ Kalem opens Org and Markdown files as they are."
        );
        // Emphasis and links' marks left out; code kept as written.
        assert_eq!(
            l(
                "**Bold** and ^^marked^^ and ~~gone~~, `[[not a link]]` and [the site](https://x.org)"
            ),
            "Bold and marked and gone, [[not a link]] and the site"
        );
        assert_eq!(l("2 ** 3 and a ** b"), "2 ** 3 and a ** b");
    }

    #[test]
    fn an_org_line_reads_by_its_links_labels() {
        let i = logseq();
        assert_eq!(
            line(
                &i,
                "pages/Notes.org",
                "About [[Kalem][the editor]], see [[https://x.org][the site]]"
            ),
            "About the editor, see the site"
        );
    }

    #[test]
    fn an_obsidian_line_reads_by_its_links_aliases() {
        let m = Memory::new(&[
            ("/v/.obsidian/app.json", "{}"),
            ("/v/Kalem.md", "An editor. ^first\n"),
            ("/v/Other.md", "See [[Kalem|the editor]].\n"),
        ]);
        let i = Index::build(
            &m,
            Graph::load(&m, "/v", Kind::Obsidian, &Fallbacks::default()),
        );
        let l = |t: &str| line(&i, "Other.md", t);
        assert_eq!(
            l("See [[Kalem|the editor]] and [[Kalem#Use]]."),
            "See the editor and Kalem#Use."
        );
        assert_eq!(
            l("Read [the note](Kalem.md), ==this== #tag"),
            "Read the note, this #tag"
        );
        assert_eq!(l("![[Kalem#^first]]"), "↳ An editor.");
    }
}
