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
    // The graph indexed just now: its notes' layers asked again.
    assert!(out.contains(&Effect::RefreshLayers), "{out:?}");
    assert!(
        !app.opened(&m, "/w/notes/pages/Other.md")
            .contains(&Effect::RefreshLayers)
    );
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

fn edits_of(out: &[Effect]) -> Vec<crate::edit::Edit> {
    out.iter()
        .find_map(|e| match e {
            Effect::Edits { edits, .. } => Some(edits.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no edits: {out:?}"))
}

#[test]
fn outliner_commands_edit_the_note() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Other.md");
    let text = "- write\n\t- child\n- read\n";
    let c = ctx("/w/notes/pages/Other.md", text, 2);
    let e = edits_of(&app.command(&m, "graph.cycleTodo", &c));
    assert_eq!(
        crate::edit::apply(text, &e),
        "- LATER write\n\t- child\n- read\n"
    );
    let out = app.command(&m, "graph.moveBlockDown", &c);
    assert_eq!(
        crate::edit::apply(text, &edits_of(&out)),
        "- read\n- write\n\t- child\n"
    );
    assert!(matches!(&out[0], Effect::Edits { line: Some(1), .. }));
    let c2 = ctx("/w/notes/pages/Other.md", text, 18);
    assert_eq!(
        crate::edit::apply(text, &edits_of(&app.command(&m, "graph.indent", &c2))),
        "- write\n\t- child\n\t- read\n"
    );
    // A priority and a date go through a question, then a command run in
    // the same document.
    let out = app.command(&m, "graph.setPriority", &c);
    let Effect::Pick { token, .. } = &out[0] else {
        panic!()
    };
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![0])));
    let Effect::Run { id, args } = &out[0] else {
        panic!("{out:?}")
    };
    let mut c3 = c.clone();
    c3.args = args.clone();
    assert_eq!(
        crate::edit::apply(text, &edits_of(&app.command(&m, id, &c3))),
        "- [#A] write\n\t- child\n- read\n"
    );
    let out = app.command(&m, "graph.schedule", &c);
    let Effect::Prompt { token, .. } = &out[0] else {
        panic!()
    };
    let out = app.answer(&m, *token, Answer::Text(Some("2026-10-12".into())));
    let Effect::Run { id, args } = &out[0] else {
        panic!("{out:?}")
    };
    c3.args = args.clone();
    assert_eq!(
        crate::edit::apply(text, &edits_of(&app.command(&m, id, &c3))),
        "- write\n  SCHEDULED: <2026-10-12 Mon>\n\t- child\n- read\n"
    );
    // Folding writes collapsed:: true.
    assert_eq!(
        crate::edit::apply(text, &edits_of(&app.command(&m, "graph.toggleFold", &c))),
        "- write\n  collapsed:: true\n\t- child\n- read\n"
    );
}

#[test]
fn a_block_gets_its_id_when_first_referred_to() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Other.md");
    let out = app.command(
        &m,
        "graph.insertBlockRef",
        &ctx("/w/notes/pages/Other.md", "", 0),
    );
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!("{out:?}")
    };
    // The block with an id first, then the others.
    assert_eq!(items[0].0, "An editor of [[Org]] files");
    let i = items
        .iter()
        .position(|(l, _)| l == "met [[Editor]]")
        .unwrap();
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![i as u32])));
    let inserted = out
        .iter()
        .find_map(|e| match e {
            Effect::Run { id, args } if id == "graph.insertText" => Some(args.clone()),
            _ => None,
        })
        .expect("the reference inserted");
    let journal = m.get("/w/notes/journals/2026_10_03.md").unwrap();
    let id = journal.split("id:: ").nth(1).unwrap().trim().to_string();
    assert!(crate::scan::is_uuid(&id), "{journal}");
    assert_eq!(journal, format!("- met [[Editor]]\n  id:: {id}\n"));
    assert!(inserted.contains(&format!("(({id}))")));
    // The index knows the new id.
    assert!(app.index("/w/notes").unwrap().block(&id).is_some());
    // A block of the current document: the editor writes both.
    let out = app.command(
        &m,
        "graph.insertBlockRef",
        &ctx("/w/notes/pages/Other.md", "", 0),
    );
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!()
    };
    let i = items
        .iter()
        .position(|(l, _)| l.starts_with("uses [[Kalem]]"))
        .unwrap();
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![i as u32])));
    let Effect::Run { id, args } = &out[0] else {
        panic!("{out:?}")
    };
    assert_eq!(id, "graph.refBlockNow");
    let text = m.get("/w/notes/pages/Other.md").unwrap();
    let mut c = ctx("/w/notes/pages/Other.md", &text, text.len());
    c.args = args.clone();
    let after = crate::edit::apply(&text, &edits_of(&app.command(&m, id, &c)));
    assert!(
        after.starts_with(
            "- uses [[Kalem]] and ((11111111-2222-3333-4444-555555555555)) #idea\n  id:: "
        ),
        "{after}"
    );
    assert!(after.trim_end().ends_with("))"), "{after}");
    // A file open with unsaved changes is not written.
    m.write("/w/notes/pages/Third.md", "- third block\n")
        .unwrap();
    app.saved(&m, "/w/notes/pages/Third.md");
    app.track_open(7, "/w/notes/pages/Third.md");
    app.track_changed(7);
    let pick = |app: &mut App| {
        let out = app.command(
            &m,
            "graph.insertBlockRef",
            &ctx("/w/notes/pages/Other.md", "", 0),
        );
        let Effect::Pick { token, items, .. } = &out[0] else {
            panic!()
        };
        let i = items.iter().position(|(l, _)| l == "third block").unwrap();
        (*token, i as u32)
    };
    let (token, i) = pick(&mut app);
    let out = app.answer(&m, token, Answer::Picked(Some(vec![i])));
    assert!(
        matches!(&out[0], Effect::Notify(t, Level::Warning) if t.contains("unsaved")),
        "{out:?}"
    );
    assert_eq!(
        m.get("/w/notes/pages/Third.md").as_deref(),
        Some("- third block\n")
    );
    app.saved(&m, "/w/notes/pages/Third.md");
    let (token, i) = pick(&mut app);
    app.answer(&m, token, Answer::Picked(Some(vec![i])));
    assert!(
        m.get("/w/notes/pages/Third.md")
            .unwrap()
            .starts_with("- third block\n  id:: ")
    );
}

#[test]
fn a_page_renamed_everywhere() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let text = m.get("/w/notes/pages/Kalem.md").unwrap();
    let out = app.command(
        &m,
        "graph.renamePage",
        &ctx("/w/notes/pages/Kalem.md", &text, 0),
    );
    let Effect::Prompt { token, value, .. } = &out[0] else {
        panic!("{out:?}")
    };
    assert_eq!(value.as_deref(), Some("Kalem"));
    let out = app.answer(&m, *token, Answer::Text(Some("Kalem Editor".into())));
    let Effect::Run { id, args } = &out[0] else {
        panic!()
    };
    let mut c = ctx("/w/notes/pages/Kalem.md", &text, 0);
    c.args = args.clone();
    let out = app.command(&m, id, &c);
    // The current document by the editor: its title written.
    assert_eq!(
        crate::edit::apply(&text, &edits_of(&out)),
        format!("title:: Kalem Editor\n{text}")
    );
    // The other file by the plugin; the alias left as it was.
    assert_eq!(
        m.get("/w/notes/pages/Other.md").unwrap(),
        "- uses [[Kalem Editor]] and ((11111111-2222-3333-4444-555555555555)) #idea\n"
    );
    assert_eq!(
        m.get("/w/notes/journals/2026_10_03.md").unwrap(),
        "- met [[Editor]]\n"
    );
}

/// The document an effect list shows or writes anew.
fn document(out: &[Effect]) -> (String, String, String, Content) {
    out.iter()
        .find_map(|e| match e {
            Effect::Document {
                id,
                key,
                kind,
                content,
                ..
            } => Some((id.clone(), key.clone(), kind.clone(), content.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no document in {out:?}"))
}

/// The byte where line `n` of `text` starts.
fn line_start(text: &str, n: usize) -> usize {
    text.lines().take(n).map(|l| l.len() + 1).sum()
}

#[test]
fn queries_followed_run_and_shown() {
    let m = logseq();
    let note = "- Tasks\n\t- {{query (and [[Kalem]] (task TODO))}}\n\t  query-table:: true\n\t  query-properties:: [:block :page]\n- #+BEGIN_QUERY\n  {:query [:find ?b]}\n  #+END_QUERY\n";
    m.write("/w/notes/pages/Plan.md", note).unwrap();
    m.write(
        "/w/notes/pages/Work.md",
        "- TODO ship [[Kalem]]\n- DONE start [[Kalem]]\n",
    )
    .unwrap();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Plan.md");
    // Followed anywhere on its line: the Query document, a table as the
    // block's properties ask.
    let out = app.command(&m, "graph.follow", &ctx("/w/notes/pages/Plan.md", note, 10));
    let (id, key, kind, content) = document(&out);
    assert_eq!((id.as_str(), kind.as_str()), ("graph.query", "graph-query"));
    assert!(key.ends_with("table=block,page"), "{key:?}");
    assert!(
        content.text.contains("TODO ship [[Kalem]] │ Work"),
        "{}",
        content.text
    );
    // An advanced query is explained.
    let at = line_start(note, 5) + 3;
    let out = app.command(&m, "graph.follow", &ctx("/w/notes/pages/Plan.md", note, at));
    assert!(
        matches!(&out[0], Effect::Notify(t, _) if t.contains("not run by Kalem")),
        "{out:?}"
    );
    // Run from the palette: a list.
    let out = app.command(
        &m,
        "graph.runQuery",
        &ctx("/w/notes/pages/Plan.md", note, 0),
    );
    let Effect::Prompt { token, .. } = &out[0] else {
        panic!("{out:?}")
    };
    let out = app.answer(
        &m,
        *token,
        Answer::Text(Some("{{query (task DONE)}}".into())),
    );
    let (_, _, _, content) = document(&out);
    assert!(content.text.contains("1 block"), "{}", content.text);
    assert!(content.text.contains("DONE start [[Kalem]]"));
    // In the note, the summary in place of the macro.
    let o = app.overlays("logseq", Some("/w/notes/pages/Plan.md"), note);
    let shown: Vec<&str> = o
        .spans
        .iter()
        .filter_map(|s| match &s.effect {
            crate::layer::Effect::Replace(t, _) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        shown.contains(&"⌕ 1 block: TODO ship [[Kalem]]"),
        "{shown:?}"
    );
    assert!(
        shown.iter().any(|t| t.contains("advanced query")),
        "{shown:?}"
    );
    // The summary follows the index.
    m.write(
        "/w/notes/pages/Work.md",
        "- TODO ship [[Kalem]]\n- TODO test [[Kalem]]\n",
    )
    .unwrap();
    let out = app.saved(&m, "/w/notes/pages/Work.md");
    assert!(out.contains(&Effect::RefreshLayers));
    let o = app.overlays("logseq", Some("/w/notes/pages/Plan.md"), note);
    assert!(o.spans.iter().any(
        |s| matches!(&s.effect, crate::layer::Effect::Replace(t, _) if t.starts_with("⌕ 2 blocks"))
    ));
}

#[test]
fn a_task_cycled_from_the_tasks_document() {
    let m = logseq();
    m.write(
        "/w/notes/pages/Work.md",
        "- TODO ship [[Kalem]]\n  SCHEDULED: <2026-10-12 Mon>\n- LATER test\n",
    )
    .unwrap();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Work.md");
    let out = app.command(&m, "graph.tasks", &ctx("/w/notes/pages/Work.md", "", 0));
    let (id, key, _, content) = document(&out);
    let n = content
        .text
        .lines()
        .position(|l| l.contains("ship"))
        .unwrap();
    let c = Ctx {
        path: None,
        text: Some(content.text.clone()),
        cursor: line_start(&content.text, n) + 2,
        doc: Some((id.clone(), key.clone())),
        args: "null".into(),
    };
    // TODO, DOING, DONE: Markdown's TODO workflow.
    let out = app.command(&m, "graph.cycleTodo", &c);
    assert_eq!(
        m.read("/w/notes/pages/Work.md").unwrap(),
        "- DOING ship [[Kalem]]\n  SCHEDULED: <2026-10-12 Mon>\n- LATER test\n"
    );
    let (_, _, _, again) = document(&out);
    assert!(again.text.contains("DOING ship"), "{}", again.text);
    // A heading's line holds no task.
    let c0 = Ctx {
        cursor: 0,
        ..c.clone()
    };
    let out = app.command(&m, "graph.cycleTodo", &c0);
    assert!(matches!(&out[0], Effect::Notify(t, _) if t == "No block on this line"));
    // A file Kalem holds with unsaved changes is not written.
    app.track_open(7, "/w/notes/pages/Work.md");
    app.track_changed(7);
    let n = again.text.lines().position(|l| l.contains("ship")).unwrap();
    let c = Ctx {
        text: Some(again.text.clone()),
        cursor: line_start(&again.text, n) + 2,
        ..c
    };
    let out = app.command(&m, "graph.cycleTodo", &c);
    assert!(
        matches!(&out[0], Effect::Notify(t, Level::Warning) if t.contains("unsaved")),
        "{out:?}"
    );
    assert!(
        m.read("/w/notes/pages/Work.md")
            .unwrap()
            .starts_with("- DOING")
    );
}

#[test]
fn searching_headings_recent_pages_and_whiteboards() {
    let m = logseq();
    m.write("/w/notes/whiteboards/Plan.edn", "{:blocks () :pages ()}")
        .unwrap();
    m.write("/w/notes/pages/Board.md", "- see [[Plan]]\n")
        .unwrap();
    m.write(
        "/w/notes/pages/Guide.md",
        "- Intro\n- ## Usage\n\t- run it\n",
    )
    .unwrap();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    // The graph's folder searched by Kalem, Logseq's own folder left out.
    let out = app.command(&m, "graph.search", &ctx("/w/notes/pages/Kalem.md", "", 0));
    let [Effect::Run { id, args }] = &out[..] else {
        panic!("{out:?}")
    };
    assert_eq!(id, "search.folder");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(args).unwrap(),
        serde_json::json!({ "path": "/w/notes", "ignore": ["/logseq/"] })
    );
    // Headings and the top-level blocks of pages, by page.
    let out = app.command(
        &m,
        "graph.findHeading",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!("{out:?}")
    };
    let n = items
        .iter()
        .position(|(t, page)| t == "Usage" && page.as_deref() == Some("Guide"))
        .unwrap_or_else(|| panic!("{items:?}"));
    assert!(items.iter().any(|(t, _)| t == "An editor of [[Org]] files"));
    // A journal's top-level blocks are its entries, not headings.
    assert!(!items.iter().any(|(t, _)| t == "met [[Editor]]"));
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![n as u32])));
    assert_eq!(opens(&out), [("/w/notes/pages/Guide.md".to_string(), 2)]);
    assert!(!items.iter().any(|(t, _)| t == "run it"));
    // The pages opened last, the current one left out.
    app.opened(&m, "/w/notes/pages/Other.md");
    app.opened(&m, "/w/notes/pages/Board.md");
    let out = app.command(&m, "graph.recent", &ctx("/w/notes/pages/Board.md", "", 0));
    let Effect::Pick { token, items, .. } = &out[0] else {
        panic!("{out:?}")
    };
    let titles: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(titles, ["Other", "Kalem"]);
    let out = app.answer(&m, *token, Answer::Picked(Some(vec![1])));
    assert_eq!(opens(&out), [("/w/notes/pages/Kalem.md".to_string(), 1)]);
    // A whiteboard followed: shown in the file manager, Logseq draws it.
    let out = app.command(
        &m,
        "graph.follow",
        &ctx("/w/notes/pages/Board.md", "- see [[Plan]]\n", 9),
    );
    assert_eq!(
        out,
        [Effect::Reveal {
            path: "/w/notes/whiteboards/Plan.edn".into(),
            app: "Logseq"
        }]
    );
}

#[test]
fn a_graphs_folder_is_its_root() {
    // As `kalem run graph graph.pages FOLDER` and Kalem's listing of the
    // folder give it.
    let m = logseq();
    let mut app = App::new(Settings::default());
    let out = app.command(&m, "graph.pages", &ctx("/w/notes", "", 0));
    let (_, key, kind, content) = document(&out);
    assert_eq!((key.as_str(), kind.as_str()), ("/w/notes", "graph-pages"));
    assert!(content.text.contains("Kalem"));
    // A page at random, by the clock's bits.
    app.random = Some(|| 1);
    let out = app.command(&m, "graph.random", &ctx("/w/notes", "", 0));
    assert_eq!(opens(&out), [("/w/notes/pages/Kalem.md".to_string(), 1)]);
    app.random = None;
    let out = app.command(&m, "graph.random", &ctx("/w/notes", "", 0));
    assert_eq!(opens(&out).len(), 1);
}
