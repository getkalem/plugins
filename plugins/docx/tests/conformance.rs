//! The part of the conformance suite that needs no plugin API: the manifest.
//!
//! The rest (byte-exact round trip, incremental equals full parse, snapshots
//! in both frontends, the budget) comes with the `kalem-plugin` bindings.

use serde_json::Value;

const MANIFEST: &str = include_str!("../plugin.json");

/// The permission scopes of section 11.6 of the design document.
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
fn id_is_reverse_domain() {
    let m = manifest();
    let id = field(&m, "id").as_str().unwrap();
    let parts: Vec<&str> = id.split('.').collect();
    assert!(
        parts.len() >= 2,
        "`id` is a reverse domain name such as org.example.name"
    );
    assert!(
        parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')),
        "`id` parts are alphanumeric"
    );
}

#[test]
fn version_is_semver() {
    let m = manifest();
    let version = field(&m, "version").as_str().unwrap();
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 3, "`version` is MAJOR.MINOR.PATCH");
    assert!(
        parts.iter().all(|p| p.parse::<u64>().is_ok()),
        "`version` parts are numbers"
    );
}

#[test]
fn main_is_a_component() {
    let m = manifest();
    let main = field(&m, "main").as_str().unwrap();
    assert!(main.ends_with(".wasm"), "`main` names the component file");
    assert!(
        !main.starts_with('/') && !main.contains(".."),
        "`main` is a relative path inside the package"
    );
}

#[test]
fn permissions_are_known_scopes() {
    let m = manifest();
    for p in field(&m, "permissions").as_array().unwrap() {
        let p = p.as_str().expect("a permission is a string");
        // `subprocess:PROGRAM` names the one program a plugin runs, a
        // bare name (the git plugin's design, 9.1).
        let program = |name: &str| {
            !name.is_empty()
                && !name.starts_with('.')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        let known = SCOPES.contains(&p)
            || p.strip_prefix("net:fetch:").is_some_and(|d| !d.is_empty())
            || p.strip_prefix("subprocess:").is_some_and(program);
        assert!(
            known,
            "unknown permission `{p}`; see section 11.6 of the design document"
        );
    }
}
