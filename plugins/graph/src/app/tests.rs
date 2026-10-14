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
    // Open Link: the graph's references by its rules, anything else by
    // the mode.
    let out = app.command(
        &m,
        "graph.openLink",
        &ctx("/w/notes/pages/Other.md", text, 10),
    );
    assert_eq!(opens(&out), [("/w/notes/pages/Kalem.md".to_string(), 1)]);
    let out = app.command(
        &m,
        "graph.openLink",
        &ctx("/w/notes/pages/Other.md", text, 1),
    );
    assert!(
        matches!(&out[..], [Effect::Run { id, .. }] if id == "markdown.openLink"),
        "{out:?}"
    );
    let org = "* see [[https://x.com][web]]\n";
    let out = app.command(
        &m,
        "graph.openLink",
        &ctx("/w/notes/pages/Web.org", org, 10),
    );
    assert!(
        matches!(&out[..], [Effect::Run { id, .. }] if id == "org.dwim"),
        "{out:?}"
    );
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
fn a_page_opened_before_its_file_is_written_has_its_backlinks() {
    // Follow Reference on `[[Org]]` opens `pages/Org.md`, not written yet.
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let org = "/w/notes/pages/Org.md";
    app.opened(&m, org);
    let (_, _, _, content) =
        document(&app.command(&m, "graph.backlinksDocument", &ctx(org, "", 0)));
    assert!(
        content.text.starts_with("Backlinks: Org"),
        "{}",
        content.text
    );
    assert!(
        content.text.contains("An editor of [[Org]] files"),
        "{}",
        content.text
    );
    let out = app.command(&m, "graph.localGraph", &ctx(org, "", 0));
    let (_, _, _, content) = document(&out);
    assert_eq!(content.drawing.unwrap().focus.as_deref(), Some("org"));
    // A file no link names is no page yet.
    let out = app.command(
        &m,
        "graph.backlinksDocument",
        &ctx("/w/notes/pages/Nothing.md", "", 0),
    );
    assert!(matches!(&out[..], [Effect::Notify(..)]), "{out:?}");
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
            ("/a".to_string(), None),
            ("/b".to_string(), Some(Kind::Obsidian))
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
    // Enter makes a block as Logseq does; off a block's first line it is
    // Markdown's own Enter.
    let c4 = ctx("/w/notes/pages/Other.md", text, 7);
    assert_eq!(
        crate::edit::apply(text, &edits_of(&app.command(&m, "graph.newBlock", &c4))),
        "- write\n\t- \n\t- child\n- read\n"
    );
    let prose = "- write\n  more\n";
    let out = app.command(
        &m,
        "graph.newBlock",
        &ctx("/w/notes/pages/Other.md", prose, 14),
    );
    assert!(
        matches!(&out[0], Effect::Run { id, .. } if id == "edit.newline"),
        "{out:?}"
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
    // Its file renamed, as Logseq renames a page named by its file: no
    // `title::` written.
    assert!(
        !out.iter().any(|e| matches!(e, Effect::Edits { .. })),
        "{out:?}"
    );
    assert_eq!(m.get("/w/notes/pages/Kalem.md"), None);
    assert_eq!(m.get("/w/notes/pages/Kalem Editor.md"), Some(text.clone()));
    assert!(
        app.index("/w/notes")
            .unwrap()
            .page("kalem editor")
            .is_some_and(|p| p.path.as_deref() == Some("pages/Kalem Editor.md"))
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
    // An advanced query: its results, or why it does not run.
    let at = line_start(note, 5) + 3;
    let out = app.command(&m, "graph.follow", &ctx("/w/notes/pages/Plan.md", note, at));
    let (id, _, _, content) = document(&out);
    assert_eq!(id, "graph.advancedQuery");
    assert!(
        content
            .text
            .contains("Not run: ?b is bound by no clause of :where"),
        "{}",
        content.text
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
    // Its count; its results under its line.
    assert!(shown.contains(&"⌕ 1 block"), "{shown:?}");
    let under: Vec<String> = o
        .below
        .iter()
        .flat_map(|b| b.rows.iter().map(crate::rows::Row::text))
        .collect();
    assert!(under.iter().any(|t| t == "• TODO ship Kalem"), "{under:?}");
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

#[test]
fn folders_named_in_the_setting_graphs() {
    // Neither folder has a marker; the setting names them without a kind.
    let m = Memory::new(&[
        (
            "/w/plain/Intro.md",
            "# Intro\n\nSee [[Rules]] and [[sub/Extra]].\n",
        ),
        ("/w/plain/Rules.md", "- [[Intro]]\n"),
        ("/w/plain/sub/Extra.md", "# Extra\n"),
        ("/w/lq/pages/A.md", "- [[B]]\n"),
        ("/w/lq/pages/B.md", "- b\n"),
    ]);
    let mut app = App::new(Settings::default());
    let out = app.command(&m, "graph.pages", &ctx("/w/plain/Intro.md", "", 0));
    assert!(
        matches!(&out[..], [Effect::Notify(t, Level::Warning)] if t.contains("setting graphs")),
        "{out:?}"
    );
    let settings = Settings {
        graphs: vec![("/w/plain".into(), None), ("/w/lq".into(), None)],
        ..Settings::default()
    };
    assert!(app.settings(&m, settings.clone()).is_empty());
    // The same settings again change nothing.
    assert!(app.settings(&m, settings).is_empty());
    // A folder of notes is a vault, its notes anywhere in it.
    let out = app.command(&m, "graph.pages", &ctx("/w/plain/Intro.md", "", 0));
    assert!(
        !out.iter().any(|e| matches!(e, Effect::Notify(..))),
        "{out:?}"
    );
    let (_, key, _, content) = document(&out);
    assert_eq!(key, "/w/plain");
    for page in ["Intro", "Rules", "Extra"] {
        assert!(content.text.contains(page), "{page}: {}", content.text);
    }
    // One with Logseq's `pages/` is a Logseq graph, its defaults used
    // without a word of the missing `logseq/config.edn`.
    let out = app.command(&m, "graph.pages", &ctx("/w/lq/pages/A.md", "", 0));
    assert!(
        !out.iter().any(|e| matches!(e, Effect::Notify(..))),
        "{out:?}"
    );
    let (_, key, _, content) = document(&out);
    assert_eq!(key, "/w/lq");
    assert!(content.text.contains("A") && content.text.contains("B"));
}

#[test]
fn the_panels_unlinked_references_searched_when_opened() {
    let m = logseq();
    m.write("/w/notes/pages/Mention.md", "- kalem is named here\n")
        .unwrap();
    let mut app = App::new(Settings::default());
    let panel = |out: &[Effect]| {
        out.iter()
            .find_map(|e| match e {
                Effect::Panel(p) => Some(p.clone()),
                _ => None,
            })
            .expect("the panel")
    };
    let p = panel(&app.opened(&m, "/w/notes/pages/Kalem.md"));
    assert_eq!(p[2].expanded, Some(false));
    // Opened by its arrow or its entry: searched.
    let p = panel(&app.panel_expanded("2", true));
    assert_eq!(p[2].detail.as_deref(), Some("1"));
    assert_eq!(p[2].expanded, Some(true));
    // Kept open on the same page, closed on another.
    let p = panel(&app.saved(&m, "/w/notes/pages/Mention.md"));
    assert_eq!(p[2].expanded, Some(true));
    let p = panel(&app.opened(&m, "/w/notes/pages/Other.md"));
    assert_eq!(p[2].expanded, Some(false));
    let p = panel(&app.panel_clicked(views::UNLINKED_SEARCH));
    assert_eq!(p[2].expanded, Some(true));
    assert!(app.panel_expanded("1", false).is_empty());
}

#[test]
fn a_mirror_is_never_written() {
    // A Logseq database graph's Markdown Mirror: read, never written.
    let m = Memory::new(&[
        ("/db/mirror/markdown/.index.edn", "{}"),
        (
            "/db/mirror/markdown/pages/Plan.md",
            "id:: 11111111-1111-4111-8111-111111111111\n\n- TODO write [[Kalem]]\n",
        ),
    ]);
    let mut app = App::new(Settings::default());
    let out = app.opened(&m, "/db/mirror/markdown/pages/Plan.md");
    assert!(
        out.contains(&Effect::Status(Some("⌬ db · 1 page".into()))),
        "{out:?}"
    );
    let text = "id:: 11111111-1111-4111-8111-111111111111\n\n- TODO write [[Kalem]]\n";
    let c = ctx("/db/mirror/markdown/pages/Plan.md", text, 46);
    for id in [
        "graph.cycleTodo",
        "graph.toggleFold",
        "graph.insertLink",
        "graph.renamePage",
    ] {
        let out = app.command(&m, id, &c);
        assert!(
            matches!(&out[..], [Effect::Notify(t, _)] if t.starts_with("This is the Markdown Mirror")),
            "{id}: {out:?}"
        );
    }
    // A page only referenced is not made.
    let out = app.command(
        &m,
        "graph.follow",
        &ctx("/db/mirror/markdown/pages/Plan.md", text, 60),
    );
    assert!(
        matches!(&out[..], [Effect::Notify(t, _)] if t.starts_with("This is the Markdown Mirror")),
        "{out:?}"
    );
    assert!(m.read("/db/mirror/markdown/pages/Kalem.md").is_err());
    // Reading works: the pages.
    let out = app.command(&m, "graph.pages", &c);
    assert!(matches!(&out[0], Effect::Document { .. }));
}

#[test]
fn unsaved_typing_is_seen_by_commands() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Other.md");
    app.track_open(3, "/w/notes/pages/Other.md");
    app.track_changed(3);
    // A link typed, not saved: the next command in the note sees it.
    let typed = "- uses [[Kalem]] and ((11111111-2222-3333-4444-555555555555)) #idea\n- typed now: [[Kalem]] again\n";
    let out = app.command(
        &m,
        "graph.backlinksDocument",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    let (_, _, _, before) = document(&out);
    assert!(!before.text.contains("typed now"));
    app.command(&m, "graph.tasks", &ctx("/w/notes/pages/Other.md", typed, 0));
    let out = app.command(
        &m,
        "graph.backlinksDocument",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    let (_, _, _, after) = document(&out);
    assert!(
        after.text.contains("typed now: [[Kalem]] again"),
        "{}",
        after.text
    );
    // Closed without saving: the file's text again.
    let out = app.closed_document(&m, 3);
    assert!(out.contains(&Effect::RefreshLayers));
    let out = app.command(
        &m,
        "graph.backlinksDocument",
        &ctx("/w/notes/pages/Kalem.md", "", 0),
    );
    let (_, _, _, closed) = document(&out);
    assert!(!closed.text.contains("typed now"), "{}", closed.text);
    // A file of a folder the graph hides is not taken.
    let mut i = app.index("/w/notes").unwrap().clone();
    assert!(!i.update_unsaved("logseq/bak/x.md", "- [[Kalem]]"));
    assert!(!i.update_unsaved(".hidden/x.md", "- [[Kalem]]"));
}

#[test]
fn pages_sorted_and_a_blocks_references() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let c = ctx("/w/notes/pages/Kalem.md", "", 0);
    let (_, key, _, content) = document(&app.command(&m, "graph.pages", &c));
    let order = |content: &Content| -> Vec<String> {
        content
            .text
            .lines()
            .skip_while(|l| !l.starts_with("Pages ("))
            .skip(1)
            .take_while(|l| !l.is_empty())
            .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
            .collect()
    };
    assert_eq!(order(&content), ["Kalem", "Other", "Templates"]);
    assert!(content.text.contains("by title; s sorts by links"));
    // `s` in the document: by links, then by blocks.
    let mut d = c.clone();
    d.doc = Some(("graph.pages".into(), key.clone()));
    let (_, _, _, content) = document(&app.command(&m, "graph.sortPages", &d));
    assert_eq!(order(&content), ["Kalem", "Other", "Templates"]);
    assert!(content.text.contains("by links; s sorts by blocks"));
    let (_, _, _, content) = document(&app.command(&m, "graph.sortPages", &d));
    assert_eq!(order(&content), ["Templates", "Kalem", "Other"]);
    // A block's references: from the block at the cursor.
    let text = m.get("/w/notes/pages/Kalem.md").unwrap();
    let at = text.find("An editor").unwrap();
    let out = app.command(
        &m,
        "graph.blockReferences",
        &ctx("/w/notes/pages/Kalem.md", &text, at),
    );
    let (id, _, kind, content) = document(&out);
    assert_eq!(
        (id.as_str(), kind.as_str()),
        ("graph.blockReferences", "graph-backlinks")
    );
    assert!(
        matches!(&out[0], Effect::Document { title, .. } if title == "References: An editor of [[Org]] files"),
        "{out:?}"
    );
    assert!(content.text.contains("uses [[Kalem]]"), "{}", content.text);
    // A block nothing refers to.
    let other = m.get("/w/notes/pages/Other.md").unwrap();
    let out = app.command(
        &m,
        "graph.blockReferences",
        &ctx("/w/notes/pages/Other.md", &other, 4),
    );
    assert!(
        matches!(&out[0], Effect::Notify(m, _) if m == "Nothing refers to this block"),
        "{out:?}"
    );
}

/// Kalem's own keys under `SPC m`, `SPC n` and `SPC o`, the plugin's
/// groups, in the Vim profile, by the mode their when-clause names (`""`
/// for every mode), as `crates/kalem-core/keymaps/vim.json` of Kalem
/// 0.6.12 binds them, its `leader` written `space`. No key of Kalem's
/// starts with one of the plugin's single keys (Enter, Tab, `alt+[` …).
/// To be refreshed when Kalem's change.
const KALEM_VIM_KEYS: &[(&str, &str)] = &[
    (
        "",
        "space n l, space n s, space n shift+f, space n y, space n shift+y, \
         space o p, space o o, space o shift+o, space o i, space o shift+i, \
         space o t, space o shift+t, space o b, space o l, space o x, \
         space o f, space o -, space o shift+p",
    ),
    (
        "markdown",
        "space m e, space m i b, space m i i, space m i e, space m i s, \
         space m i c, space m i l, space m i u, space m i w, space m t e, \
         space m t l, space m t m, space m t w, space m x, space m t x, \
         space m b a, space m b s, space o l",
    ),
    (
        "org",
        "space m t, space m q, space m o, space m x, space m ., space m /, \
         space m shift+a, space m e, space m f, space m k, space m j, \
         space m n, space m @, space m ,, space m +, space m h, space m *, \
         space m i, space m l d, space m b -, space m b a, space m b c, \
         space m b f, space m b r, space m b s, space m b i c, \
         space m b i r, space m b i h, space m b i shift+h, space m b d c, \
         space m b d r, space m c e, space m c shift+e, space m d d, \
         space m d s, space m d t, space m d shift+t, space m g g, \
         space m g shift+g, space m g x, space m l i, space m l l, \
         space m l shift+l, space m l s, space m l shift+s, space m l t, \
         space m p p, space m p u, space m p d, space m r r, space m r ., \
         space m s a, space m s shift+a, space m s d, space m s h, \
         space m s l, space m s j, space m s k, space m s n, \
         space m s shift+n, space m s r, space m s s, space m s shift+s",
    ),
];

/// The modes of a graph's notes a when-clause lets its binding apply in:
/// those it names, else both (none for the plugin's documents).
fn note_modes(when: &str) -> Vec<&'static str> {
    let names_a_type = when.contains("editorMode == ") || when.contains("textType == ");
    ["markdown", "org"]
        .into_iter()
        .filter(|m| {
            !names_a_type
                || when.contains(&format!("editorMode == {m}"))
                || when.contains(&format!("textType == {m}"))
        })
        .collect()
}

/// Every key of the plugin runs its command in a graph's notes with Vim
/// keys. Kalem waits for the next key after keys that a longer binding
/// starts with (`Keymap::lookup`), whatever the order of the bindings:
/// Cycle Task on `SPC m t` never ran in Markdown, where Kalem has Doom's
/// toggles `SPC m t e` and `SPC m t x`, nor Set Priority on `SPC m p` in
/// Org, beside `SPC m p u`. A key of the plugin that one of Kalem's
/// starts would take that key from its notes the same way. A key equal
/// to one of Kalem's is the plugin's in its notes once the keys of a
/// layer's notes come after the profile's (Kalem's ed4a3dd7, after
/// 0.6.12); in 0.6.12 Kalem's own runs.
#[test]
fn note_keys_are_reachable_in_the_vim_profile() {
    // A longer key sequence starting with a shorter one, chord by chord.
    let starts = |long: &str, short: &str| {
        long.strip_prefix(short)
            .is_some_and(|rest| rest.starts_with(' '))
    };
    let mut bound = Vec::new();
    for c in COMMANDS {
        let leader = c.keys.iter().map(|k| (*k, LEADER_WHEN));
        for (keys, when) in leader.chain(c.note_keys.iter().copied()) {
            for mode in note_modes(when) {
                bound.push((mode, keys, c.id));
            }
        }
    }
    let mut clashes = Vec::new();
    for &(mode, keys, id) in &bound {
        let kalem = KALEM_VIM_KEYS
            .iter()
            .filter(|(m, _)| m.is_empty() || *m == mode)
            .flat_map(|(_, list)| list.split(", "));
        for theirs in kalem {
            if starts(theirs, keys) {
                clashes.push(format!(
                    "{id}: `{keys}` never runs in {mode}, Kalem's `{theirs}` starts with it"
                ));
            }
            if starts(keys, theirs) {
                clashes.push(format!(
                    "{id}: `{keys}` takes Kalem's `{theirs}` from {mode} notes"
                ));
            }
        }
        for &(_, other, other_id) in bound.iter().filter(|b| b.0 == mode) {
            if starts(other, keys) {
                clashes.push(format!(
                    "{id}: `{keys}` never runs in {mode}, {other_id}'s `{other}` starts with it"
                ));
            }
        }
    }
    assert!(clashes.is_empty(), "{}", clashes.join("\n"));
}

#[test]
fn completion_of_pages_blocks_tags_and_blocks_of_text() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    let other = "/w/notes/pages/Other.md";
    // Before the graph is indexed: nothing, Kalem's own completers answer.
    assert!(app.complete("refs", Some(other), "- [[Ka", 0, 6).is_empty());
    app.opened(&m, other);
    let complete = |text: &str| {
        let (text, after) = text.split_once('|').unwrap_or((text, ""));
        let whole = format!("{text}{after}");
        app.complete("refs", Some(other), &whole, 0, text.len())
    };
    // `[[`: pages by title, the link replacing the `[[`.
    let found = complete("- see [[Ka");
    assert_eq!(
        (
            found[0].label.as_str(),
            found[0].insert.as_str(),
            found[0].start
        ),
        ("Kalem", "[[Kalem]]", 6)
    );
    assert_eq!(found[0].detail, "pages/Kalem.md");
    // A closing `]]` after the cursor is kept.
    assert_eq!(complete("- see [[Ka|]] more")[0].insert, "[[Kalem");
    // Aliases, written as typed.
    let found = complete("- [[edi");
    assert_eq!(
        (
            found[0].label.as_str(),
            found[0].insert.as_str(),
            found[0].detail.as_str()
        ),
        ("Editor", "[[Editor]]", "alias of Kalem")
    );
    // `((`: blocks with an id by their text.
    let found = complete("- see ((an ed");
    assert_eq!(found[0].label, "An editor of [[Org]] files");
    assert_eq!(found[0].insert, "11111111-2222-3333-4444-555555555555))");
    assert_eq!((found[0].start, found[0].detail.as_str()), (8, "Kalem"));
    // `#`: tags, a title with a space in `[[…]]`.
    let found = complete("- an #id");
    assert_eq!(
        (
            found[0].label.as_str(),
            found[0].insert.as_str(),
            found[0].start
        ),
        ("idea", "idea", 6)
    );
    assert!(complete("- C#").is_empty(), "no tag inside a word");
    // `<`: Logseq's blocks of text, indented as the block's lines.
    let found = complete("\t- <no");
    assert_eq!(found[0].label, "Note");
    assert_eq!(found[0].insert, "#+BEGIN_NOTE\n\t  \n\t  #+END_NOTE");
    assert_eq!((found[0].start, found[0].cursor), (3, Some(16)));
    // Elsewhere, or another completer: nothing.
    assert!(complete("- plain words").is_empty());
    assert!(
        app.complete("other", Some(other), "- [[Ka", 0, 6)
            .is_empty()
    );
}

#[test]
fn today_from_the_local_clock() {
    // 2026-10-09 at 23:00 in UTC, 2026-10-10 at 02:00 in Istanbul.
    fn utc() -> i64 {
        1_791_590_400_000 - 3_600_000
    }
    fn local() -> i64 {
        utc() + 3 * 3_600_000
    }
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.now = Some(utc);
    app.local_now = Some(local);
    app.opened(&m, "/w/notes/pages/Kalem.md");
    // No question: the local day's journal, made from the template.
    let out = app.command(&m, "graph.today", &ctx("/w/notes/pages/Kalem.md", "", 0));
    assert_eq!(
        opens(&out),
        [("/w/notes/journals/2026_10_10.md".to_string(), 1)]
    );
    assert_eq!(
        m.get("/w/notes/journals/2026_10_10.md").as_deref(),
        Some("- TODO plan [[Oct 10th, 2026]]\n")
    );
}

#[test]
fn completion_names_notes_of_the_same_name_by_their_link() {
    let m = Memory::new(&[
        ("/v/.obsidian/app.json", r#"{"newLinkFormat":"shortest"}"#),
        ("/v/Home.md", "x\n"),
        ("/v/Note.md", "x"),
        ("/v/Deep/Note.md", "y"),
    ]);
    let mut app = App::new(Settings::default());
    app.opened(&m, "/v/Home.md");
    let found = app.complete("refs", Some("/v/Home.md"), "x [[No", 0, 6);
    let shown: Vec<(&str, &str)> = found
        .iter()
        .map(|s| (s.label.as_str(), s.insert.as_str()))
        .collect();
    assert_eq!(
        shown,
        [("Note", "[[Note]]"), ("Deep/Note", "[[Deep/Note]]")]
    );
}

#[test]
fn completion_of_properties_and_their_values() {
    let m = Memory::new(&[
        ("/w/notes/logseq/config.edn", "{}"),
        (
            "/w/notes/pages/A.md",
            "type:: book\ntags:: reading, later\n\n- one\n  status:: open\n",
        ),
        (
            "/w/notes/pages/B.md",
            "type:: book\n\n- two\n  status:: done\n  id:: 11111111-2222-3333-4444-555555555555\n",
        ),
        ("/w/notes/pages/C.md", "type:: person\ntags:: people\n"),
    ]);
    let mut app = App::new(Settings::default());
    let c = "/w/notes/pages/C.md";
    app.opened(&m, c);
    let complete = |text: &str| app.complete("refs", Some(c), text, 0, text.len());
    let shown = |found: Vec<crate::app::complete::Suggestion>| -> Vec<(String, String, usize)> {
        found
            .into_iter()
            .map(|s| (s.label, s.insert, s.start))
            .collect()
    };
    // `::` at a block's text: the keys, the most used first; `id` is
    // Logseq's own, hidden.
    let keys = shown(complete("- x\n  ::"));
    let labels: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
    assert_eq!(labels, ["type", "status", "tags"]);
    assert_eq!((keys[0].1.as_str(), keys[0].2), ("type:: ", 6));
    assert_eq!(
        shown(complete("- ::st")),
        [("status".into(), "status:: ".into(), 2)]
    );
    // `key::`: its values; a value just typed is not offered again.
    assert_eq!(
        shown(complete("- status:: op")),
        [("open".into(), "open".into(), 11)]
    );
    let values = shown(complete("type::"));
    assert_eq!(values[0], ("book".into(), " book".into(), 6));
    // A property separated by commas: the value after the last comma.
    assert_eq!(
        shown(complete("tags:: people, rea")),
        [("reading".into(), "reading".into(), 15)]
    );
    // Not a property line.
    assert!(complete("- some text:: here").is_empty());
}

#[test]
fn completion_of_logseqs_slash_commands() {
    fn local() -> i64 {
        // 2026-10-11 at 09:05, local.
        1_791_590_400_000 + 86_400_000 + (9 * 60 + 5) * 60_000
    }
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.local_now = Some(local);
    let other = "/w/notes/pages/Other.md";
    app.opened(&m, other);
    let complete = |text: &str| app.complete("refs", Some(other), text, 0, text.len());
    // At a block's start: keywords, priorities and headings too.
    let found = complete("- /to");
    let names: Vec<&str> = found.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(names, ["TODO", "Tomorrow", "Today"]);
    assert_eq!((found[0].insert.as_str(), found[0].start), ("TODO ", 2));
    // Inside a block's text: no keywords; dates and the time as written.
    let found = complete("- see /to");
    assert_eq!(
        (found[0].label.as_str(), found[0].insert.as_str()),
        ("Tomorrow", "[[Oct 12th, 2026]]")
    );
    let found = complete("- at /current");
    assert_eq!(found[0].insert, "09:05");
    // The cursor inside what is written.
    let found = complete("- /page emb");
    assert_eq!(
        (found[0].insert.as_str(), found[0].cursor),
        ("{{embed [[]]}}", Some(10))
    );
    let found = complete("\t- /code");
    assert_eq!(
        (found[0].insert.as_str(), found[0].cursor),
        ("```\n\t  ```", Some(3))
    );
    // A `/` inside a word, a path or a URL: nothing.
    assert!(complete("- see a/b").is_empty());
    assert!(complete("- https://x").is_empty());
}

#[test]
fn completion_of_a_vaults_headings_and_blocks() {
    let m = Memory::new(&[
        ("/v/.obsidian/app.json", "{}"),
        ("/v/Home.md", "x\n"),
        (
            "/v/Note.md",
            "# Intro\n\nA paragraph ^para1\n\n## Usage\n\n## Use cases\n",
        ),
    ]);
    let mut app = App::new(Settings::default());
    app.opened(&m, "/v/Home.md");
    let complete = |text: &str, after: &str| {
        app.complete(
            "refs",
            Some("/v/Home.md"),
            &format!("{text}{after}"),
            0,
            text.len(),
        )
    };
    let found = complete("see [[Note#us", "");
    let shown: Vec<(&str, &str, usize)> = found
        .iter()
        .map(|s| (s.label.as_str(), s.insert.as_str(), s.start))
        .collect();
    assert_eq!(
        shown,
        [("Usage", "Usage]]", 11), ("Use cases", "Use cases]]", 11)]
    );
    // A closing `]]` after the cursor is kept.
    assert_eq!(complete("[[Note#In", "]]")[0].insert, "Intro");
    // `#^`: the note's blocks with an id.
    let found = complete("[[Note#^", "");
    assert_eq!(
        (found[0].label.as_str(), found[0].insert.as_str()),
        ("A paragraph", "^para1]]")
    );
}

#[test]
fn the_graph_drawn_whole_and_around_a_page() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    let other = "/w/notes/pages/Other.md";
    app.opened(&m, other);
    let c = ctx(other, "", 0);
    let (id, key, kind, content) = document(&app.command(&m, "graph.graphView", &c));
    assert_eq!(
        (id.as_str(), kind.as_str()),
        ("graph.graphView", "graph-view")
    );
    let d = content.drawing.expect("a drawing");
    let ids: Vec<&str> = d.nodes.iter().map(|n| n.id.as_str()).collect();
    // The pages, the most linked first; the journal left out.
    assert_eq!(ids[0], "kalem");
    assert!(ids.contains(&"other") && ids.contains(&"org") && ids.contains(&"idea"));
    assert!(!ids.iter().any(|i| i.starts_with("oct")), "{ids:?}");
    assert!(
        d.edges.contains(&("kalem".into(), "other".into())),
        "{:?}",
        d.edges
    );
    assert_eq!(d.command, "graph.openNode");
    assert!(content.text.contains("Kalem  "));
    // Around a page: it in the middle, the pages two links away; Kalem's
    // alias a node of its own, as in Logseq, the journal linking to it
    // three links away.
    let (_, _, _, content) = document(&app.command(&m, "graph.localGraph", &c));
    let d = content.drawing.unwrap();
    assert_eq!(d.focus.as_deref(), Some("other"));
    let ids: Vec<&str> = d.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"kalem") && ids.contains(&"editor"), "{ids:?}");
    assert!(!ids.iter().any(|i| i.starts_with("oct")), "{ids:?}");
    // A node opened: its page.
    let mut o = c.clone();
    o.doc = Some(("graph.graphView".into(), key));
    o.args = r#"{"node":"kalem"}"#.into();
    let out = app.command(&m, "graph.openNode", &o);
    assert_eq!(opens(&out), [("/w/notes/pages/Kalem.md".to_string(), 1)]);
}

#[test]
fn a_page_with_a_title_keeps_its_file_and_a_vaults_note_is_renamed_with_it() {
    // Logseq: a page whose `title::` names it keeps its file.
    let m = Memory::new(&[
        ("/w/notes/logseq/config.edn", "{}"),
        ("/w/notes/pages/kalem_page.md", "title:: Kalem\n\n- x\n"),
        ("/w/notes/pages/Other.md", "- [[Kalem]]\n"),
    ]);
    let mut app = App::new(Settings::default());
    let text = m.get("/w/notes/pages/kalem_page.md").unwrap();
    app.opened(&m, "/w/notes/pages/kalem_page.md");
    let mut c = ctx("/w/notes/pages/kalem_page.md", &text, 0);
    c.args = r#"{"root":"/w/notes","key":"kalem","title":"Pen"}"#.into();
    let out = app.command(&m, "graph.renameNow", &c);
    assert_eq!(
        crate::edit::apply(&text, &edits_of(&out)),
        "title:: Pen\n\n- x\n"
    );
    assert!(m.get("/w/notes/pages/kalem_page.md").is_some());
    assert_eq!(m.get("/w/notes/pages/Other.md").unwrap(), "- [[Pen]]\n");
    // A vault's note: renamed with its file, every link to it rewritten.
    let m = Memory::new(&[
        ("/v/.obsidian/app.json", "{}"),
        (
            "/v/Home.md",
            "[[Note]], [[Note|shown]], [[Deep/Note#Part]] and ![[Note#^b1]]\n",
        ),
        ("/v/Deep/Note.md", "# Part\ntext ^b1\n"),
        ("/v/Other.md", "[x](Deep/Note.md) [[Elsewhere]]\n"),
    ]);
    let mut app = App::new(Settings::default());
    app.opened(&m, "/v/Home.md");
    let mut c = ctx("/v/Home.md", &m.get("/v/Home.md").unwrap(), 0);
    c.path = Some("/v/Other.md".into());
    c.text = m.get("/v/Other.md");
    c.args = r#"{"root":"/v","key":"deep/note","title":"Big Idea"}"#.into();
    let out = app.command(&m, "graph.renameNow", &c);
    assert_eq!(m.get("/v/Deep/Note.md"), None);
    assert_eq!(
        m.get("/v/Deep/Big Idea.md").as_deref(),
        Some("# Part\ntext ^b1\n")
    );
    assert_eq!(
        m.get("/v/Home.md").unwrap(),
        "[[Big Idea]], [[Big Idea|shown]], [[Deep/Big Idea#Part]] and ![[Big Idea#^b1]]\n"
    );
    // The current document by the editor.
    assert_eq!(
        crate::edit::apply(&m.get("/v/Other.md").unwrap(), &edits_of(&out)),
        "[x](Deep/Big%20Idea.md) [[Elsewhere]]\n"
    );
    // A name with a slash is a move, not a rename.
    c.args = r#"{"root":"/v","key":"deep/big idea","title":"a/b"}"#.into();
    let out = app.command(&m, "graph.renameNow", &c);
    assert!(
        matches!(&out[0], Effect::Notify(_, Level::Warning)),
        "{out:?}"
    );
}

#[test]
fn files_changed_on_disk_are_read_together() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    // Written and removed by another program (Logseq, a `git pull`):
    // noted, the first asking for `graph.filesChanged`.
    m.write("/w/notes/pages/A.md", "- about [[Kalem]]\n")
        .unwrap();
    m.write("/w/notes/pages/B.md", "- also [[Kalem]]\n")
        .unwrap();
    m.remove("/w/notes/pages/Other.md");
    assert!(app.file_changed("/w/notes/pages/A.md"));
    assert!(!app.file_changed("/w/notes/pages/B.md"));
    assert!(!app.file_changed("/w/notes/pages/Other.md"));
    // A file of no graph is not noted.
    assert!(!app.file_changed("/w/other/readme.md"));
    let none = Ctx {
        path: None,
        text: None,
        cursor: 0,
        doc: None,
        args: "{}".into(),
    };
    // Read together: the graph worked out and drawn again once.
    let out = app.command(&m, "graph.filesChanged", &none);
    let refreshed = out
        .iter()
        .filter(|e| matches!(e, Effect::RefreshLayers))
        .count();
    assert_eq!(refreshed, 1, "{out:?}");
    let index = &app.indexes["/w/notes"];
    let from: BTreeSet<&str> = index.backlinks("kalem").iter().map(|b| &*b.path).collect();
    // The journal names Kalem by its alias.
    assert_eq!(
        from,
        BTreeSet::from(["journals/2026_10_03.md", "pages/A.md", "pages/B.md"]),
        "{from:?}"
    );
    // Nothing left: the next change asks again.
    assert!(app.command(&m, "graph.filesChanged", &none).is_empty());
    m.write("/w/notes/pages/A.md", "- no link now\n").unwrap();
    assert!(app.file_changed("/w/notes/pages/A.md"));
    // Any command reads what is noted first.
    app.command(&m, "graph.pages", &ctx("/w/notes/pages/Kalem.md", "", 0));
    let from: BTreeSet<&str> = app.indexes["/w/notes"]
        .backlinks("kalem")
        .iter()
        .map(|b| &*b.path)
        .collect();
    assert_eq!(
        from,
        BTreeSet::from(["journals/2026_10_03.md", "pages/B.md"])
    );
}

#[test]
fn an_advanced_query_followed_from_any_of_its_lines() {
    let m = logseq();
    // `ı` before it and in it: its capital `I` is a byte shorter, so a
    // copy in capitals has its places elsewhere.
    let note = "- Bugün ıııı\n- #+BEGIN_QUERY\n  {:title \"Yapılacaklar ıııı\" :query [:find (pull ?b [*]) :where [?b :block/marker \"TODO\"]]}\n  #+END_QUERY\n- sonra\n";
    m.write("/w/notes/pages/Plan.md", note).unwrap();
    m.write("/w/notes/pages/Work.md", "- TODO ship [[Kalem]]\n")
        .unwrap();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Plan.md");
    let follow = |app: &mut App, line: usize| {
        let at = line_start(note, line) + 1;
        app.command(&m, "graph.follow", &ctx("/w/notes/pages/Plan.md", note, at))
    };
    // From its first line (the one showing its summary), its body and its
    // last: its results.
    for line in [1, 2, 3] {
        let (id, _, _, content) = document(&follow(&mut app, line));
        assert_eq!(id, "graph.advancedQuery", "line {line}");
        assert!(
            content.text.contains("TODO ship [[Kalem]]"),
            "line {line}: {}",
            content.text
        );
    }
    // Before it and after it: not it.
    for line in [0, 4] {
        let out = follow(&mut app, line);
        assert!(
            !out.iter()
                .any(|e| matches!(e, Effect::Document { id, .. } if id == "graph.advancedQuery")),
            "line {line}: {out:?}"
        );
    }
    // In the note, run on the whole query (the template's TODO too).
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
        shown.contains(&"⌕ Yapılacaklar ıııı · 2 blocks"),
        "{shown:?}"
    );
}

#[test]
fn the_graphs_menu() {
    let m = logseq();
    m.write(
        "/w/notes/logseq/config.edn",
        "{:file/name-format :triple-lowbar :favorites [\"Kalem\" \"Not Written\"]}",
    )
    .unwrap();
    m.write("/w/vault/.obsidian/app.json", "{}").unwrap();
    m.write("/w/vault/Note.md", "a note\n").unwrap();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/vault/Note.md");
    app.opened(&m, "/w/notes/pages/Other.md");
    let out = app.command(&m, "graph.home", &ctx("/w/notes/pages/Other.md", "", 0));
    let (id, key, kind, content) = document(&out);
    assert_eq!(
        (id.as_str(), key.as_str(), kind.as_str()),
        ("graph.home", "/w/notes", "graph-home")
    );
    let text = &content.text;
    for line in [
        "⌬ notes",
        "  Journals",
        "  Graph view",
        "Favorites",
        "  • Kalem",
        "  • Not Written",
        "Recent",
        "  • Other",
        "Other graphs",
        "  ⌬ vault  /w/vault",
    ] {
        assert!(
            text.lines().any(|l| l.starts_with(line)),
            "{line:?} in\n{text}"
        );
    }
    let enter = |app: &mut App, starts: &str| {
        let n = text.lines().position(|l| l.starts_with(starts)).unwrap();
        let c = Ctx {
            path: None,
            text: Some(text.clone()),
            cursor: line_start(text, n),
            doc: Some((id.clone(), key.clone())),
            args: "null".into(),
        };
        app.command(&m, "graph.open", &c)
    };
    // Enter on an entry runs its command; on a page, opens it (one not
    // written yet made); on another graph, that graph's menu.
    let run = |out: Vec<Effect>| match out.as_slice() {
        [Effect::Run { id, args }] => (id.clone(), args.clone()),
        o => panic!("{o:?}"),
    };
    assert_eq!(
        run(enter(&mut app, "  Journals")),
        ("graph.journals".into(), "{}".into())
    );
    assert_eq!(
        run(enter(&mut app, "  • Not Written")),
        ("graph.openNode".into(), r#"{"node":"not written"}"#.into())
    );
    let (cmd, args) = run(enter(&mut app, "  ⌬ vault"));
    assert_eq!(cmd, "graph.home");
    let c = Ctx {
        args,
        ..ctx("/w/notes/pages/Other.md", "", 0)
    };
    let (_, key, _, content) = document(&app.command(&m, "graph.home", &c));
    assert_eq!(key, "/w/vault");
    assert!(content.text.contains("Bookmarks"), "{}", content.text);
}

#[test]
fn logseqs_record_of_changes_sorts_pages_and_is_followed() {
    let m = logseq();
    // Logseq keeps when each page was made and last changed.
    m.write(
        "/w/notes/logseq/pages-metadata.edn",
        "[{:block/name \"other\" :block/created-at 1759000000000 :block/updated-at 1760000000000}\n {:block/name \"kalem\" :block/created-at 1758000000000 :block/updated-at 1759500000000}]",
    )
    .unwrap();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let index = &app.indexes["/w/notes"];
    assert_eq!(
        index.times("kalem"),
        Some((1_758_000_000_000, 1_759_500_000_000))
    );
    assert_eq!(index.times("templates"), None);
    // All pages, the page changed last first, with its day.
    let c = ctx("/w/notes/pages/Kalem.md", "", 0);
    let (_, key, _, _) = document(&app.command(&m, "graph.pages", &c));
    let mut d = c.clone();
    d.doc = Some(("graph.pages".into(), key));
    for _ in 0..3 {
        app.command(&m, "graph.sortPages", &d);
    }
    let (_, _, _, content) = document(&app.command(&m, "graph.pages", &c));
    assert!(
        content.text.contains("by changed; s sorts by title"),
        "{}",
        content.text
    );
    let lines: Vec<&str> = content
        .text
        .lines()
        .skip_while(|l| !l.starts_with("Pages ("))
        .skip(1)
        .take(3)
        .collect();
    assert!(
        lines[0].starts_with("Other") && lines[0].ends_with("changed 2025-10-09"),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("Kalem") && lines[1].ends_with("changed 2025-10-03"),
        "{lines:?}"
    );
    assert!(lines[2].starts_with("Templates"), "{lines:?}");
    // A query sorted by when its pages were made.
    let q = crate::query::parse("(and (page-property type) (sort-by created-at))");
    assert!(q.is_ok());
    // Logseq writes the record again: read again, alone.
    m.write(
        "/w/notes/logseq/pages-metadata.edn",
        "[{:block/name \"kalem\" :block/created-at 1758000000000 :block/updated-at 1761000000000}]",
    )
    .unwrap();
    assert!(app.file_changed("/w/notes/logseq/pages-metadata.edn"));
    let none = Ctx {
        path: None,
        text: None,
        cursor: 0,
        doc: None,
        args: "{}".into(),
    };
    app.command(&m, "graph.filesChanged", &none);
    assert_eq!(
        app.indexes["/w/notes"].times("kalem"),
        Some((1_758_000_000_000, 1_761_000_000_000))
    );
    assert_eq!(app.indexes["/w/notes"].times("other"), None);
}

#[test]
fn the_graphs_settings_changed_by_its_app_are_followed() {
    let m = logseq();
    let mut app = App::new(Settings::default());
    app.opened(&m, "/w/notes/pages/Kalem.md");
    let c = ctx("/w/notes/pages/Kalem.md", "", 0);
    let (_, _, _, content) = document(&app.command(&m, "graph.home", &c));
    assert!(!content.text.contains("• Other"), "{}", content.text);
    // Logseq writes its configuration: a favorite added.
    m.write(
        "/w/notes/logseq/config.edn",
        "{:file/name-format :triple-lowbar :default-templates {:journals \"daily\"} :favorites [\"Other\"]}",
    )
    .unwrap();
    assert!(app.file_changed("/w/notes/logseq/config.edn"));
    let none = Ctx {
        path: None,
        text: None,
        cursor: 0,
        doc: None,
        args: "{}".into(),
    };
    app.command(&m, "graph.filesChanged", &none);
    let (_, _, _, content) = document(&app.command(&m, "graph.home", &c));
    assert!(
        content.text.contains("Favorites\n  • Other"),
        "{}",
        content.text
    );
    // Obsidian's windows' layout is not its settings.
    assert!(config::is_settings(".obsidian/graph.json"));
    assert!(!config::is_settings(".obsidian/workspace.json"));
    assert!(!config::is_settings(".obsidian/plugins/x/data.json"));
}
