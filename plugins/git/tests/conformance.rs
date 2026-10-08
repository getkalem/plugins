//! The part of the conformance suite that needs no plugin API: the
//! manifest, its permissions and its settings.

use serde_json::Value;

const MANIFEST: &str = include_str!("../plugin.json");

/// The permission scopes of section 11.6 of Kalem's design document, with
/// `subprocess:PROGRAM` as this plugin's design proposes it (DESIGN.md,
/// 9.1).
const SCOPES: &[&str] = &[
    "fs:read:workspace",
    "fs:write:workspace",
    "fs:read:all",
    "subprocess",
];

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("plugin.json parses")
}

fn field<'a>(m: &'a Value, key: &str) -> &'a Value {
    m.get(key)
        .unwrap_or_else(|| panic!("plugin.json needs `{key}`"))
}

/// A program a `subprocess:` permission names: a bare name, no path.
fn program(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.')
}

#[test]
fn required_fields_are_present_and_typed() {
    let m = manifest();
    for key in ["id", "name", "version", "description", "main", "api"] {
        assert!(field(&m, key).is_string(), "`{key}` is a string");
    }
    assert!(field(&m, "activation").is_array(), "`activation` is a list");
    assert!(
        field(&m, "permissions").is_array(),
        "`permissions` is a list"
    );
}

#[test]
fn id_version_and_main() {
    let m = manifest();
    assert_eq!(field(&m, "id"), "org.kalem.git");
    let version = field(&m, "version").as_str().unwrap();
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 3, "`version` is MAJOR.MINOR.PATCH");
    assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    let main = field(&m, "main").as_str().unwrap();
    assert!(main.ends_with(".wasm") && !main.starts_with('/') && !main.contains(".."));
}

#[test]
fn permissions_are_known_scopes() {
    let m = manifest();
    let permissions = field(&m, "permissions").as_array().unwrap();
    for p in permissions {
        let p = p.as_str().expect("a permission is a string");
        let known = SCOPES.contains(&p)
            || p.strip_prefix("net:fetch:").is_some_and(|d| !d.is_empty())
            || p.strip_prefix("subprocess:").is_some_and(program);
        assert!(known, "unknown permission `{p}`");
    }
    // The plugin runs git and nothing else, and reads no file itself (G5).
    assert_eq!(permissions, &["subprocess:git"]);
}

#[test]
fn programs_are_bare_names() {
    assert!(program("git"));
    assert!(program("git-lfs"));
    assert!(!program(""));
    assert!(!program("/usr/bin/git"));
    assert!(!program("../git"));
    assert!(!program("git status"));
}

#[test]
fn settings_are_described_and_their_defaults_typed() {
    let m = manifest();
    let settings = field(&m, "settings")
        .as_object()
        .expect("`settings` is a table");
    assert!(!settings.is_empty());
    for (key, s) in settings {
        let kind = s["type"]
            .as_str()
            .unwrap_or_else(|| panic!("`{key}` has a type"));
        assert!(
            s["description"].as_str().is_some_and(|d| !d.is_empty()),
            "`{key}` has a description"
        );
        let default = &s["default"];
        let typed = match kind {
            "boolean" => default.is_boolean(),
            "integer" => default.is_i64(),
            "string" => default.is_string(),
            "array" => default.is_array(),
            other => panic!("`{key}` has the unknown type `{other}`"),
        };
        assert!(typed, "`{key}`'s default is a {kind}");
        if let Some(choices) = s["enum"].as_array() {
            assert!(
                choices.contains(default),
                "`{key}`'s default is one of its choices"
            );
        }
        if let (Some(min), Some(max), Some(d)) = (
            s["minimum"].as_i64(),
            s["maximum"].as_i64(),
            default.as_i64(),
        ) {
            assert!(min <= d && d <= max, "`{key}`'s default is in its range");
        }
    }
}

#[test]
fn the_library_reads_every_choice_of_its_settings() {
    use kalem_plugin_git::git::cmd::{PullMode, Untracked};
    use kalem_plugin_git::views::Glyphs;
    let m = manifest();
    let choices = |key: &str| -> Vec<String> {
        m["settings"][key]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    assert!(
        choices("untracked")
            .iter()
            .all(|c| Untracked::parse(c).is_some())
    );
    assert!(choices("pull").iter().all(|c| PullMode::parse(c).is_some()));
    assert!(choices("glyphs").iter().all(|c| Glyphs::parse(c).is_some()));
}

#[test]
fn the_menus_hold_the_plugins_commands_outside_the_status() {
    use kalem_plugin_git::app::command_info;
    let m = manifest();
    let menus = m["menus"].as_array().expect("menus");
    for menu in menus {
        assert!(menu["title"].as_str().is_some_and(|t| !t.trim().is_empty()));
        assert!(menu["when"].as_str().is_some_and(|w| w.contains("vcs == git")));
        for item in menu["items"].as_array().expect("items") {
            let item = item.as_str().expect("a command or -");
            if item == "-" {
                continue;
            }
            let c = command_info(item).unwrap_or_else(|| panic!("`{item}` is no command"));
            assert!(!c.in_status_only, "`{item}` acts in the status only");
        }
    }
}
