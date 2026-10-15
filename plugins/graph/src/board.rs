//! Whiteboards and canvases read for Kalem to draw (`graph_todo.md` K22,
//! Kalem's plugin API 0.2.16): a Logseq whiteboard (`whiteboards/*.edn`,
//! tldraw's shapes as Logseq 0.10.9 writes them in its blocks'
//! `:logseq.tldraw.shape`) and an Obsidian canvas (`*.canvas`, JSON
//! Canvas) as shapes where their file places them, and connectors. A
//! shape that shows a page, a block, a file or a link leads there
//! ([`Target`]); the plugin never writes them.

use crate::content::{Board, BoardKind, BoardLine, BoardShape};
use crate::edn::Value;
use crate::index::Index;

/// Where opening a shape leads, written as its target for Kalem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A page, by name.
    Page(String),
    /// A block, by id.
    Block(String),
    /// A file of the graph, relative to its folder.
    File(String),
    /// A web address.
    Url(String),
}

impl Target {
    /// As the canvas's shape's target: `page:…`, `block:…`, `file:…`,
    /// `url:…`.
    pub fn encode(&self) -> String {
        match self {
            Target::Page(p) => format!("page:{p}"),
            Target::Block(b) => format!("block:{b}"),
            Target::File(f) => format!("file:{f}"),
            Target::Url(u) => format!("url:{u}"),
        }
    }

    /// A target written by [`Target::encode`].
    pub fn decode(s: &str) -> Option<Target> {
        let (kind, rest) = s.split_once(':')?;
        Some(match kind {
            "page" => Target::Page(rest.into()),
            "block" => Target::Block(rest.into()),
            "file" => Target::File(rest.into()),
            "url" => Target::Url(rest.into()),
            _ => return None,
        })
    }
}

/// A Logseq color's name as Kalem's: its own names, pink as purple; a
/// color written out kept.
fn logseq_color(c: &str) -> String {
    match c.trim() {
        "pink" => "purple".into(),
        c if c.starts_with("var(") => String::new(),
        c => c.into(),
    }
}

fn number(v: Option<&Value>) -> Option<f32> {
    match v? {
        Value::Int(n) => Some(*n as f32),
        Value::Float(f) => Some(*f as f32),
        _ => None,
    }
}

fn pair(v: Option<&Value>) -> Option<[f32; 2]> {
    let items = v?.items();
    Some([number(items.first())?, number(items.get(1))?])
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A Logseq whiteboard's shapes, in tldraw's order (`:index`), and its
/// connectors, bound to the shapes their ends were dropped on; a portal
/// shows its page's title or its block's text from `index`, and leads
/// there; a picture is found in the board's assets.
pub fn logseq(edn: &str, index: &Index, rel: &str) -> Board {
    let mut board = Board::default();
    let Ok(v) = crate::edn::parse(edn) else {
        return board;
    };
    // The page's bindings (a line's end on a shape) and its assets.
    let page = v
        .get("pages")
        .and_then(|p| p.items().first())
        .and_then(|p| p.get("block/properties"))
        .and_then(|p| p.get("logseq.tldraw.page"));
    let mut bindings: Vec<(String, String)> = Vec::new();
    if let Some(Value::Map(pairs)) = page.and_then(|p| p.get("bindings")) {
        for (k, b) in pairs {
            if let (Some(id), Some(to)) = (k.as_str(), b.get("toId").and_then(Value::as_str)) {
                bindings.push((id.to_string(), to.to_string()));
            }
        }
    }
    let assets: Vec<(String, String)> = page
        .and_then(|p| p.get("assets"))
        .map(|a| {
            a.items()
                .iter()
                .map(|x| (text(x.get("id")), text(x.get("src"))))
                .collect()
        })
        .unwrap_or_default();
    let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
    let mut shapes: Vec<(String, &Value)> = v
        .get("blocks")
        .map(Value::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|b| {
            let s = b.get("block/properties")?.get("logseq.tldraw.shape")?;
            Some((text(s.get("index")), s))
        })
        .collect();
    shapes.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, s) in shapes {
        let id = text(s.get("id"));
        let kind = text(s.get("type"));
        let [x, y] = pair(s.get("point")).unwrap_or([0.0, 0.0]);
        let [w, h] = pair(s.get("size")).unwrap_or([0.0, 0.0]);
        let fill = if s.get("noFill").and_then(Value::as_bool) == Some(true) {
            String::new()
        } else {
            logseq_color(&text(s.get("fill")))
        };
        let stroke = logseq_color(&text(s.get("stroke")));
        let dashed = text(s.get("strokeType")) == "dashed";
        let label = text(s.get("label"));
        let mut shape = BoardShape {
            id: id.clone(),
            kind: BoardKind::Box(0.0),
            rect: [x, y, w, h],
            text: label,
            fill,
            stroke,
            dashed,
            target: None,
        };
        match kind.as_str() {
            "box" => {
                shape.kind = BoardKind::Box(number(s.get("borderRadius")).unwrap_or(0.0));
            }
            "ellipse" => shape.kind = BoardKind::Ellipse,
            "polygon" => shape.kind = BoardKind::Box(0.0),
            "text" => {
                shape.kind = BoardKind::Text;
                shape.text = text(s.get("text"));
                // Where its file sizes it, as tldraw keeps a text's size
                // locked to its text; at its font's size (20 pixels unless
                // it says) when the file gives none.
                if w <= 0.0 || h <= 0.0 {
                    let font = number(s.get("fontSize")).unwrap_or(20.0);
                    let longest = shape
                        .text
                        .lines()
                        .map(|l| l.chars().count())
                        .max()
                        .unwrap_or(1);
                    shape.rect[2] = longest as f32 * font * 0.55;
                    shape.rect[3] = shape.text.lines().count().max(1) as f32 * font * 1.3;
                }
            }
            "group" => shape.kind = BoardKind::Group,
            "pencil" | "highlighter" => {
                let points: Vec<[f32; 2]> = s
                    .get("points")
                    .map(Value::items)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|p| pair(Some(p)))
                    .collect();
                let (mut x1, mut y1) = (0.0_f32, 0.0_f32);
                for p in &points {
                    x1 = x1.max(p[0]);
                    y1 = y1.max(p[1]);
                }
                shape.rect = [x, y, x1, y1];
                shape.kind = BoardKind::Stroke(points);
            }
            "line" => {
                // A connector: its ends where its handles are, or on the
                // shapes they are bound to.
                let handle = |end: &str| s.get("handles").and_then(|h| h.get(end));
                let at = |end: &str| {
                    let p = pair(handle(end).and_then(|h| h.get("point"))).unwrap_or([0.0, 0.0]);
                    [x + p[0], y + p[1]]
                };
                let bound = |end: &str| {
                    let b = text(handle(end).and_then(|h| h.get("bindingId")));
                    bindings
                        .iter()
                        .find(|(id, _)| *id == b)
                        .map(|(_, to)| to.clone())
                };
                let decorations = s.get("decorations");
                let arrow = |end: &str| {
                    decorations.and_then(|d| d.get(end)).and_then(Value::as_str) == Some("arrow")
                };
                board.lines.push(BoardLine {
                    from: bound("start"),
                    to: bound("end"),
                    start: at("start"),
                    end: at("end"),
                    label: shape.text.clone(),
                    arrow_start: arrow("start"),
                    arrow_end: arrow("end"),
                    stroke: shape.stroke.clone(),
                    dashed,
                });
                continue;
            }
            "logseq-portal" => {
                let target = text(s.get("pageId"));
                if text(s.get("blockType")) == "B" {
                    shape.text = index
                        .block(&target)
                        .map_or_else(|| target.clone(), |(_, b)| b.text.clone());
                    shape.target = Some(Target::Block(target).encode());
                } else {
                    shape.text = index
                        .page(&crate::names::key(&target))
                        .map_or_else(|| target.clone(), |p| p.title.clone());
                    shape.target = Some(Target::Page(target).encode());
                }
                shape.kind = BoardKind::Box(number(s.get("borderRadius")).unwrap_or(8.0));
            }
            "image" => {
                let asset = text(s.get("assetId"));
                let src = assets
                    .iter()
                    .find(|(id, _)| *id == asset)
                    .map(|(_, src)| src.clone())
                    .unwrap_or_default();
                if src.starts_with("data:") {
                    // A picture kept in the board itself, as Logseq's first
                    // whiteboard keeps its own: Kalem draws pictures from
                    // files only, so a box says one is there.
                    shape.text = "picture".into();
                } else if src.starts_with("http://") || src.starts_with("https://") {
                    // One on the web: its address, leading there.
                    shape.text = src.clone();
                    shape.target = Some(Target::Url(src).encode());
                } else {
                    // `../assets/x.png`, from the whiteboards' folder.
                    let path = resolve(dir, &src);
                    shape.kind = BoardKind::Image(index.graph.path(&path));
                    shape.target = Some(Target::File(path).encode());
                }
            }
            "youtube" | "tweet" | "iframe" | "video" | "html" => {
                let url = text(s.get("url"));
                shape.text = if url.is_empty() {
                    kind.clone()
                } else {
                    url.clone()
                };
                if !url.is_empty() {
                    shape.target = Some(Target::Url(url).encode());
                }
            }
            _ => {}
        }
        board.shapes.push(shape);
    }
    board
}

/// `src` from folder `dir` of the graph: `../assets/x.png` from
/// `whiteboards` is `assets/x.png`.
fn resolve(dir: &str, src: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in src.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// An Obsidian canvas's color (`"1"` to `"6"`, or `#rrggbb`) as Kalem's.
fn obsidian_color(c: &str) -> String {
    match c {
        "1" => "red",
        "2" => "orange",
        "3" => "yellow",
        "4" => "green",
        "5" => "cyan",
        "6" => "purple",
        c => c,
    }
    .into()
}

/// An Obsidian canvas's nodes and edges (JSON Canvas): text as written, a
/// note leading to it, a picture drawn, a link leading out, a group's
/// label at its top.
pub fn obsidian(json: &str, index: &Index) -> Board {
    let mut board = Board::default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return board;
    };
    let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    let n = |v: &serde_json::Value, k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    let empty = Vec::new();
    for node in v["nodes"].as_array().unwrap_or(&empty) {
        let color = obsidian_color(&s(node, "color"));
        let mut shape = BoardShape {
            id: s(node, "id"),
            kind: BoardKind::Box(6.0),
            rect: [
                n(node, "x"),
                n(node, "y"),
                n(node, "width"),
                n(node, "height"),
            ],
            text: String::new(),
            fill: color.clone(),
            stroke: color,
            dashed: false,
            target: None,
        };
        match node["type"].as_str().unwrap_or_default() {
            "text" => shape.text = s(node, "text"),
            "file" => {
                let file = s(node, "file");
                let picture = matches!(
                    file.rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase())
                        .as_deref(),
                    Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg")
                );
                if picture {
                    shape.kind = BoardKind::Image(index.graph.path(&file));
                } else {
                    shape.text = file
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file)
                        .trim_end_matches(".md")
                        .to_string();
                }
                shape.target = Some(Target::File(file).encode());
            }
            "link" => {
                let url = s(node, "url");
                shape.text = url.clone();
                shape.target = Some(Target::Url(url).encode());
            }
            "group" => {
                shape.kind = BoardKind::Group;
                shape.text = s(node, "label");
                shape.fill = String::new();
            }
            _ => {}
        }
        board.shapes.push(shape);
    }
    for e in v["edges"].as_array().unwrap_or(&empty) {
        board.lines.push(BoardLine {
            from: Some(s(e, "fromNode")),
            to: Some(s(e, "toNode")),
            start: [0.0, 0.0],
            end: [0.0, 0.0],
            label: s(e, "label"),
            arrow_start: e["fromEnd"].as_str() == Some("arrow"),
            arrow_end: e["toEnd"].as_str() != Some("none"),
            stroke: obsidian_color(&s(e, "color")),
            dashed: false,
        });
    }
    board
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Kind;
    use crate::files::Memory;

    fn index(kind: Kind, files: &[(&str, &str)]) -> Index {
        let m = Memory::new(files);
        let g = crate::config::Graph::load(&m, "/g", kind, &crate::config::Fallbacks::default());
        Index::build(&m, g)
    }

    const BOARD: &str = r#"{:blocks ({:block/content "", :block/properties {:ls-type :whiteboard-shape, :logseq.tldraw.shape {:id "b1", :type "box", :index "a1", :point [10 20], :size [200 100], :label "Plans", :fill "yellow", :stroke "pink", :noFill false, :borderRadius 2, :strokeType "dashed"}}, :block/uuid #uuid "6512c0de-0005-4000-8000-000000000001"}
 {:block/properties {:ls-type :whiteboard-shape, :logseq.tldraw.shape {:id "p1", :type "logseq-portal", :index "a2", :point [400 20], :size [300 120], :pageId "Kalem", :blockType "P"}}}
 {:block/properties {:ls-type :whiteboard-shape, :logseq.tldraw.shape {:id "l1", :type "line", :index "a3", :point [210 70], :handles {:start {:id "start", :point [0 0], :bindingId "x1"}, :end {:id "end", :point [190 0], :bindingId "x2"}}, :decorations {:end "arrow"}, :label "leads to"}}}
 {:block/properties {:ls-type :whiteboard-shape, :logseq.tldraw.shape {:id "s1", :type "pencil", :index "a4", :point [10 200], :points [[0 0 0.5] [20 10 0.5] [40 0 0.5]]}}}
 {:block/properties {:ls-type :whiteboard-shape, :logseq.tldraw.shape {:id "t1", :type "text", :index "a0", :point [10 -40], :size [16 32], :text "A sketch"}}}
 {:block/properties {:ls-type :whiteboard-shape, :logseq.tldraw.shape {:id "i1", :type "image", :index "a5", :point [10 300], :size [80 60], :assetId "as1"}}}),
 :pages ({:block/name "design", :block/original-name "Design", :block/properties {:ls-type :whiteboard-page, :logseq.tldraw.page {:id "design", :name "Design", :bindings {"x1" {:id "x1", :type "line", :fromId "l1", :toId "b1", :handleId "start"}, "x2" {:id "x2", :type "line", :fromId "l1", :toId "p1", :handleId "end"}}, :assets [{:id "as1", :type "image", :src "../assets/pic.png", :size [800 600]}]}}, :block/type "whiteboard"})}"#;

    #[test]
    fn a_logseq_whiteboards_shapes_where_it_places_them() {
        let i = index(
            Kind::Logseq,
            &[
                ("/g/logseq/config.edn", "{}"),
                ("/g/pages/Kalem.md", "title:: Kalem\n\n- an editor\n"),
            ],
        );
        let b = logseq(BOARD, &i, "whiteboards/Design.edn");
        let ids: Vec<&str> = b.shapes.iter().map(|s| s.id.as_str()).collect();
        // In tldraw's order; the line a connector, not a shape.
        assert_eq!(ids, ["t1", "b1", "p1", "s1", "i1"]);
        let box_ = &b.shapes[1];
        assert_eq!(box_.kind, BoardKind::Box(2.0));
        assert_eq!(box_.rect, [10.0, 20.0, 200.0, 100.0]);
        assert_eq!(
            (box_.text.as_str(), box_.fill.as_str(), box_.stroke.as_str()),
            ("Plans", "yellow", "purple")
        );
        assert!(box_.dashed);
        let portal = &b.shapes[2];
        assert_eq!(portal.text, "Kalem");
        assert_eq!(portal.target.as_deref(), Some("page:Kalem"));
        assert_eq!(
            b.shapes[3].kind,
            BoardKind::Stroke(vec![[0.0, 0.0], [20.0, 10.0], [40.0, 0.0]])
        );
        assert_eq!(b.shapes[3].rect, [10.0, 200.0, 40.0, 10.0]);
        assert_eq!(b.shapes[0].text, "A sketch");
        assert_eq!(
            b.shapes[4].kind,
            BoardKind::Image("/g/assets/pic.png".into())
        );
        assert_eq!(b.shapes[4].target.as_deref(), Some("file:assets/pic.png"));
        let l = &b.lines[0];
        assert_eq!(
            (l.from.as_deref(), l.to.as_deref()),
            (Some("b1"), Some("p1"))
        );
        assert_eq!((l.start, l.end), ([210.0, 70.0], [400.0, 70.0]));
        assert!(l.arrow_end && !l.arrow_start);
        assert_eq!(l.label, "leads to");
        assert_eq!(
            Target::decode("page:Kalem"),
            Some(Target::Page("Kalem".into()))
        );
    }

    #[test]
    fn pictures_in_the_board_or_on_the_web_and_texts_at_their_size() {
        let i = index(Kind::Logseq, &[("/g/logseq/config.edn", "{}")]);
        let edn = r#"{:blocks ({:block/properties {:logseq.tldraw.shape {:id "i1", :type "image", :index "a1", :point [0 0], :size [80 60], :assetId "d"}}}
 {:block/properties {:logseq.tldraw.shape {:id "i2", :type "image", :index "a2", :point [100 0], :size [80 60], :assetId "w"}}}
 {:block/properties {:logseq.tldraw.shape {:id "t1", :type "text", :index "a3", :point [0 100], :size [101 22], :fontSize 10, :text "Double Click on the Canvas"}}}
 {:block/properties {:logseq.tldraw.shape {:id "t2", :type "text", :index "a4", :point [0 200], :fontSize 40, :text "Big"}}}),
 :pages ({:block/properties {:logseq.tldraw.page {:assets [{:id "d", :src "data:image/png;base64,iVBORw0KGgo="} {:id "w", :src "https://example.com/a.png"}]}}})}"#;
        let b = logseq(edn, &i, "whiteboards/W.edn");
        // Pictures are drawn from files only: one kept in the board (as
        // Logseq's own first whiteboard keeps them) is a box saying so,
        // one on the web leads there.
        let p = &b.shapes[0];
        assert_eq!(
            (&p.kind, p.text.as_str(), p.target.as_deref()),
            (&BoardKind::Box(0.0), "picture", None)
        );
        assert_eq!(
            b.shapes[1].target.as_deref(),
            Some("url:https://example.com/a.png")
        );
        // A text where its file sizes it, though its font is small (as
        // Logseq 0.10.9 wrote it); at its font's size when it is not.
        assert_eq!(b.shapes[2].rect, [0.0, 100.0, 101.0, 22.0]);
        assert_eq!(
            b.shapes[3].rect,
            [0.0, 200.0, 3.0 * 40.0 * 0.55, 1.0 * 40.0 * 1.3]
        );
    }

    #[test]
    fn an_obsidian_canvas_nodes_and_edges() {
        let i = index(Kind::Obsidian, &[("/g/.obsidian/app.json", "{}")]);
        let canvas = r##"{"nodes":[{"id":"a","type":"text","text":"The plan","x":-300,"y":-40,"width":250,"height":60,"color":"4"},{"id":"b","type":"file","file":"Projects/Graph.md","x":40,"y":-160,"width":400,"height":300},{"id":"c","type":"file","file":"pics/map.png","x":0,"y":200,"width":100,"height":80},{"id":"d","type":"link","url":"https://kalem.org","x":0,"y":300,"width":200,"height":40},{"id":"g","type":"group","label":"Ideas","x":-320,"y":-200,"width":800,"height":600}],"edges":[{"id":"e","fromNode":"a","fromSide":"right","toNode":"b","toSide":"left","label":"see","color":"#ff0000"}]}"##;
        let b = obsidian(canvas, &i);
        assert_eq!(b.shapes.len(), 5);
        assert_eq!(
            (b.shapes[0].text.as_str(), b.shapes[0].fill.as_str()),
            ("The plan", "green")
        );
        assert_eq!(b.shapes[1].text, "Graph");
        assert_eq!(
            b.shapes[1].target.as_deref(),
            Some("file:Projects/Graph.md")
        );
        assert_eq!(b.shapes[2].kind, BoardKind::Image("/g/pics/map.png".into()));
        assert_eq!(b.shapes[3].target.as_deref(), Some("url:https://kalem.org"));
        assert_eq!(
            (b.shapes[4].kind.clone(), b.shapes[4].text.as_str()),
            (BoardKind::Group, "Ideas")
        );
        let e = &b.lines[0];
        assert_eq!(
            (e.from.as_deref(), e.to.as_deref(), e.label.as_str()),
            (Some("a"), Some("b"), "see")
        );
        assert!(e.arrow_end && !e.arrow_start);
        assert_eq!(e.stroke, "#ff0000");
    }
}
