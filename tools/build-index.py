#!/usr/bin/env python3
"""Build index.json from the manifests under plugins/.

For every plugins/NAME/plugin.json the entry carries the manifest's id, name,
version, description, api and permissions, the extensions a viewer opens
(`opens`), the download URL of the component of that version, and its
SHA-256 when releases/NAME-vVERSION.sha256 exists (the release workflow
writes that file). A declarative plugin (a language
plugin: a manifest and syntax files, no `main`) is published as an archive of
its folder, NAME-vVERSION.tar.gz, and its entry lists its languages. `--check` fails when index.json is
not what this script would write, so CI keeps the index current.
"""
import json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = "https://github.com/getkalem/plugins"


def build():
    entries = []
    plugins = os.path.join(ROOT, "plugins")
    for name in sorted(os.listdir(plugins)) if os.path.isdir(plugins) else []:
        manifest = os.path.join(plugins, name, "plugin.json")
        if not os.path.isfile(manifest):
            continue
        with open(manifest, encoding="utf-8") as f:
            m = json.load(f)
        tag = f"{name}-v{m['version']}"
        declarative = "main" not in m
        asset = f"{tag}.tar.gz" if declarative else os.path.basename(m["main"])
        sha = None
        sha_file = os.path.join(ROOT, "releases", f"{tag}.sha256")
        if os.path.isfile(sha_file):
            with open(sha_file, encoding="utf-8") as f:
                sha = f.read().split()[0]
        entry = {
            "id": m["id"],
            "name": m["name"],
            "version": m["version"],
            "description": m.get("description", ""),
            "api": m["api"],
            "permissions": m.get("permissions", []),
            "source": f"{REPO}/tree/main/plugins/{name}",
            "download": f"{REPO}/releases/download/{tag}/{asset}" if sha else None,
            "sha256": sha,
        }
        # The extensions a viewer opens, so that Kalem can name the plugin
        # for a file it cannot open yet, without downloading it.
        if m.get("opens"):
            entry["opens"] = list(m["opens"])
        if declarative:
            entry["kind"] = "declarative"
            entry["languages"] = [
                {"id": l["id"], "extensions": l.get("extensions", [])}
                for l in m.get("languages", [])
            ]
        entries.append(entry)
    return {"schema": 1, "plugins": entries}


def main():
    index = json.dumps(build(), indent=2, ensure_ascii=False) + "\n"
    path = os.path.join(ROOT, "index.json")
    if "--check" in sys.argv:
        with open(path, encoding="utf-8") as f:
            current = f.read()
        if current != index:
            sys.stderr.write("index.json is out of date: run tools/build-index.py\n")
            sys.exit(1)
        return
    with open(path, "w", encoding="utf-8") as f:
        f.write(index)


if __name__ == "__main__":
    main()
