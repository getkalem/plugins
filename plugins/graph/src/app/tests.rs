use super::*;
use crate::files::Memory;

fn logseq() -> Memory {
    Memory::new(&[
        (
            "/w/notes/logseq/config.edn",
            "{:file/name-format :triple-lowbar :default-templates {:journals \"daily\"}}",
        ),
        (
            "/w/notes/pages/Kalem.md",
            "alias:: Editor\n\n- An editor of [[Org]] files\n  id:: 11111111-2222-3333-4444-555555555555\n## Usage\n",
        ),
        (
            "/w/notes/pages/Other.md",
            "- uses [[Kalem]] and ((11111111-2222-3333-4444-555555555555)) #idea\n",
        ),
        (
            "/w/notes/pages/Templates.md",
            "- Daily\n  template:: daily\n\t- TODO plan <% today %>\n",
        ),
        ("/w/notes/journals/2026_10_03.md", "- met [[Editor]]\n"),
        ("/w/other/readme.md", "x"),
    ])
}

fn ctx(path: &str, text: &str, cursor: usize) -> Ctx {
    Ctx {
        path: Some(path.into()),
        text: Some(text.into()),
        cursor,
        doc: None,
        args: "null".into(),
    }
}

fn opens(effects: &[Effect]) -> Vec<(String, u32)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Open { path, line } => Some((path.clone(), *line)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_note_of_a_graph_opened() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    let out = app.opened(&m, "/w/notes/pages/Kalem.md");
    assert!(
        out.contains(&Effect::Status(Some("⌬ notes · 4 pages".into()))),
        "{out:?}"
    );
    let panel = out.iter().find_map(|e| match e {
        Effect::Panel(p) => Some(p.clone()),
        _ => None,
    });
    let panel = panel.expect("the panel");
    assert_eq!(panel[0].label, "Kalem");
    assert_eq!(panel[1].detail.as_deref(), Some("2"), "{panel:?}");
    // A file of no graph: no status.
    let out = app.opened(&m, "/w/other/readme.md");
    assert!(out.contains(&Effect::Status(None)), "{out:?}");
}

#[test]
fn following_references() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Other.md");
    let text = "- uses [[Kalem]] and ((11111111-2222-3333-4444-555555555555)) #idea\n";
    let out = app.command(
        &m,
        "graph.follow",
        &ctx("/w/notes/pages/Other.md", text, 10),
    );
    assert_eq!(opens(&out), [("/w/notes/pages/Kalem.md".to_string(), 1)]);
    let out = app.command(
        &m,
        "graph.follow",
        &ctx("/w/notes/pages/Other.md", text, 25),
    );
    assert_eq!(
        opens(&out),
        [("/w/notes/pages/Kalem.md".to_string(), 3)],
        "the block's line"
    );
    // A tag whose page is not written: a new file, opened empty.
    let out = app.command(
        &m,
        "graph.follow",
        &ctx("/w/notes/pages/Other.md", text, 64),
    );
    assert_eq!(opens(&out), [("/w/notes/pages/idea.md".to_string(), 1)]);
    assert!(
        m.get("/w/notes/pages/idea.md").is_none(),
        "nothing written for a page"
    );
    let out = app.command(&m, "graph.follow", &ctx("/w/notes/pages/Other.md", text, 1));
    assert!(matches!(&out[0], Effect::Notify(t, Level::Info) if t.contains("No reference")));
}

#[test]
fn journals_today_asked_once_then_made_from_the_template() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let out = app.command(&m, "graph.today", &ctx("/w/notes/pages/Kalem.md", "", 0));
    let (token, value) = match &out[0] {
        Effect::Prompt { token, value, .. } => (*token, value.clone()),
        e => panic!("{e:?}"),
    };
    assert_eq!(
        value.as_deref(),
        Some("2026-10-03"),
        "the newest journal as a guess"
    );
    let out = app.answer(&m, token, Answer::Text(Some("2026-10-10".into())));
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_10.md".to_string(), 1)]
    );
    assert_eq!(
        m.get("/w/notes/journals/2026_10_10.md").as_deref(),
        Some("- TODO plan [[Oct 10th, 2026]]\n")
    );
    // Asked once: yesterday opens at once, an existing journal is not
    // written again.
    let out = app.command(
        &m,
        "graph.yesterday",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_09.md".to_string(), 1)]
    );
    m.write("/w/notes/journals/2026_10_11.md", "- kept\n")
        .unwrap();
    let out = app.command(&m, "graph.tomorrow", &ctx("/w/notes/pages/Kalem.md", "", 0));
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_11.md".to_string(), 1)]
    );
    assert_eq!(
        m.get("/w/notes/journals/2026_10_11.md").as_deref(),
        Some("- kept\n")
    );
    // Moving between journals.
    // Yesterday's was written from the template too.
    let out = app.command(
        &m,
        "graph.previousJournal",
        &ctx("/w/notes/journals/2026_10_10.md", "", 0),
    );
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_09.md".to_string(), 1)]
    );
    let out = app.command(
        &m,
        "graph.previousJournal",
        &ctx("/w/notes/journals/2026_10_09.md", "", 0),
    );
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_03.md".to_string(), 1)]
    );
    // A date typed.
    let out = app.command(
        &m,
        "graph.journalOn",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    let Effect::Prompt { token, .. } = &out[0] else {
        panic!()
    };
    let out = app.answer(&m, *token, Answer::Text(Some("oct 3".into())));
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_03.md".to_string(), 1)]
    );
}

#[test]
fn finding_and_inserting() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Other.md");
    let out = app.command(
        &m,
        "graph.insertLink",
        &ctx("/w/notes/pages/Other.md", "", 0),
    );
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!("{out:?}")
    };
    let i = items.iter().position(|(l, _)| l == "Kalem").unwrap();
    assert_eq!(items[i].1.as_deref(), Some("also Editor"));
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![i as u32])));
    let Effect::Run { id, args } = &out[0] else {
        panic!("{out:?}")
    };
    assert_eq!(id, "graph.insertText");
    let mut c = ctx("/w/notes/pages/Other.md", "", 0);
    c.args = args.clone();
    assert_eq!(
        app.command(&m, id, &c),
        [Effect::Insert("[[Kalem]]".into())]
    );
    // A block reference.
    let out = app.command(
        &m,
        "graph.insertBlockRef",
        &ctx("/w/notes/pages/Other.md", "", 0),
    );
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!("{out:?}")
    };
    assert_eq!(
        items[0],
        ("An editor of [[Org]] files".into(), Some("Kalem".into()))
    );
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![0])));
    assert!(
        matches!(&out[0], Effect::Run { args, .. } if args.contains("((11111111-2222-3333-4444-555555555555))"))
    );
    // Find Page, then a new page.
    let out = app.command(&m, "graph.findPage", &ctx("/w/notes/pages/Other.md", "", 0));
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!()
    };
    assert_eq!(items[0].0, "Create a page…");
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![0])));
    let Effect::Prompt { token, .. } = &out[0] else {
        panic!()
    };
    let out = app.answer(&m, *token, Answer::Text(Some("Project/New".into())));
    assert_eq!(
        opens(&out),
        [("/w/notes/pages/Project___New.md".to_string(), 1)]
    );
    let out = app.command(&m, "graph.findPage", &ctx("/w/notes/pages/Other.md", "", 0));
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!()
    };
    let i = items.iter().position(|(l, _)| l == "Editor");
    assert!(i.is_none(), "an alias is the page's, not a page");
    let i = items.iter().position(|(l, _)| l == "Kalem").unwrap();
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![i as u32])));
    assert_eq!(opens(&out), [("/w/notes/pages/Kalem.md".to_string(), 1)]);
}

#[test]
fn documents_and_enter() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let out = app.command(
        &m,
        "graph.backlinksDocument",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    let Effect::Document {
        id,
        key,
        title,
        kind,
        content,
        show,
    } = &out[0]
    else {
        panic!("{out:?}")
    };
    assert_eq!(
        (id.as_str(), title.as_str(), kind.as_str(), *show),
        (
            "graph.backlinks",
            "Backlinks: Kalem",
            "graph-backlinks",
            true
        )
    );
    // Enter on a reference's line.
    let n = content
        .text
        .lines()
        .position(|l| l.contains("met [[Editor]]"))
        .unwrap();
    let cursor = content
        .text
        .lines()
        .take(n)
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + 3;
    let c = Ctx {
        path: None,
        text: Some(content.text.clone()),
        cursor,
        doc: Some((id.clone(), key.clone())),
        args: "null".into(),
    };
    let out = app.command(&m, "graph.open", &c);
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_03.md".to_string(), 1)]
    );
    // Saving a file writes the open document anew.
    m.write("/w/notes/pages/Third.md", "- [[Kalem]]\n").unwrap();
    let out = app.saved(&m, "/w/notes/pages/Third.md");
    let doc = out.iter().find_map(|e| match e {
        Effect::Document {
            content,
            show: false,
            ..
        } => Some(content.text.clone()),
        _ => None,
    });
    assert!(doc.unwrap().contains("Third"));
    // The other documents.
    for id in [
        "graph.pages",
        "graph.journals",
        "graph.tags",
        "graph.tasks",
        "graph.graph",
    ] {
        let out = app.command(&m, id, &ctx("/w/notes/pages/Kalem.md", "", 0));
        assert!(
            matches!(&out[0], Effect::Document { show: true, .. }),
            "{id}: {out:?}"
        );
    }
    // A click in the panel: a file's entry, then the page's title.
    assert_eq!(
        app.panel_clicked("1.0.0"),
        [Effect::Open {
            path: "/w/notes/journals/2026_10_03.md".into(),
            line: 1
        }]
    );
    assert!(
        matches!(&app.panel_clicked("page")[0], Effect::Document { title, .. } if title == "Backlinks: Kalem")
    );
    assert!(app.panel_clicked("nothing").is_empty());
}

#[test]
fn obsidian_links_as_the_vault_writes_them() {
    let m = Memory::new(&[
        (
            "/v/.obsidian/app.json",
            r#"{"newLinkFormat":"shortest","newFileLocation":"folder","newFileFolderPath":"Inbox"}"#,
        ),
        ("/v/.obsidian/daily-notes.json", r#"{"folder":"Daily"}"#),
        ("/v/Home.md", "[[Note]] #tag\n"),
        ("/v/Note.md", "x"),
        ("/v/Deep/Note.md", "y"),
        ("/v/Deep/Unique.md", "z ^blk"),
    ]);
    let mut app = App::new(Settings::default());
    app.opened(&m, "/v/Home.md");
    let link = |app: &App, key: &str| app.link_text("/v", key, Some("Home.md")).unwrap();
    assert_eq!(link(&app, "deep/unique"), "[[Unique]]");
    assert_eq!(
        link(&app, "deep/note"),
        "[[Deep/Note]]",
        "two notes are called Note"
    );
    let out = app.command(
        &m,
        "graph.follow",
        &ctx("/v/Home.md", "[[Note]] #tag\n", 11),
    );
    assert!(
        matches!(&out[0], Effect::Document { title, .. } if title == "Tag: #tag"),
        "{out:?}"
    );
    let out = app.command(&m, "graph.findPage", &ctx("/v/Home.md", "", 0));
    let Effect::Pick { token, .. } = &out[0] else {
        panic!()
    };
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![0])));
    let Effect::Prompt { token, .. } = &out[0] else {
        panic!()
    };
    let out = app.answer(&m, *token, Answer::Text(Some("Fresh".into())));
    assert_eq!(opens(&out), [("/v/Inbox/Fresh.md".to_string(), 1)]);
    let out = app.command(&m, "graph.insertBlockRef", &ctx("/v/Home.md", "", 0));
    let Effect::Pick { token, .. } = &out[0] else {
        panic!("{out:?}")
    };
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![0])));
    assert!(
        matches!(&out[0], Effect::Run { args, .. } if args.contains("[[Unique#^blk]]")),
        "{out:?}"
    );
}

#[test]
fn settings_and_paths() {
    let s = Settings::read(|k| match k {
        "graphs" => Some(r#"["/a", {"path": "/b/", "kind": "obsidian"}]"#.into()),
        "glyphs" => Some("\"ascii\"".into()),
        "journals_shown" => Some("5".into()),
        "obsidian.daily_notes" => Some("\"Days\"".into()),
        _ => None,
    });
    assert_eq!(
        s.graphs,
        [
            ("/a".to_string(), Kind::Logseq),
            ("/b".to_string(), Kind::Obsidian)
        ]
    );
    assert_eq!(s.glyphs, "ascii");
    assert_eq!(s.journals_shown, 5);
    assert_eq!(s.fallbacks.daily_notes.as_deref(), Some("Days"));
    assert_eq!(relative_path("pages/a.org", "pages/b.org"), "./b.org");
    assert_eq!(
        relative_path("journals/x.org", "pages/b.org"),
        "../pages/b.org"
    );
    assert_eq!(relative_path("Home.md", "Deep/Note"), "./Deep/Note");
}
