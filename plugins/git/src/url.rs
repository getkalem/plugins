//! Links to a file, its lines or a commit on the remote's web site
//! (DESIGN.md, 2.6 and appendix E), from the remote's URL as git has it.

/// The kind of web site a remote is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    /// GitHub, and GitHub Enterprise by the setting `remote_hosts`.
    GitHub,
    /// GitLab, its own or a company's.
    GitLab,
    /// Bitbucket Cloud.
    Bitbucket,
    /// Gitea, Forgejo, Codeberg.
    Gitea,
    /// sourcehut.
    SourceHut,
    /// Azure DevOps.
    Azure,
}

impl Forge {
    /// A kind as the setting `remote_hosts` names it.
    pub fn parse(s: &str) -> Option<Forge> {
        Some(match s.to_ascii_lowercase().as_str() {
            "github" => Forge::GitHub,
            "gitlab" => Forge::GitLab,
            "bitbucket" => Forge::Bitbucket,
            "gitea" | "forgejo" | "codeberg" => Forge::Gitea,
            "sourcehut" | "srht" => Forge::SourceHut,
            "azure" => Forge::Azure,
            _ => return None,
        })
    }
}

/// A remote's place: the web host, and the repository's path on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The host the web site is on (`github.com`, `git.example.com:8443`).
    pub host: String,
    /// The repository: `owner/repo`, `group/sub/repo`, `~user/repo`.
    pub path: String,
}

/// Reads a remote's URL: `https://host/owner/repo.git`,
/// `ssh://git@host:22/owner/repo.git`, `git@host:owner/repo.git`.
pub fn parse_remote(url: &str) -> Option<Remote> {
    let url = url.trim();
    let (host, path, web_port) = if let Some((scheme, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        };
        // An SSH port is not the web site's; an HTTPS one is.
        let web = matches!(scheme, "http" | "https").then_some(port).flatten();
        (host.to_string(), path.to_string(), web)
    } else {
        // `user@host:path`, as scp writes it.
        let (authority, path) = url.split_once(':')?;
        if path.starts_with("//") || authority.contains('/') {
            return None;
        }
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        (host.to_string(), path.to_string(), None)
    };
    let mut host = host.to_ascii_lowercase();
    let mut path = path.trim_matches('/').trim_end_matches(".git").to_string();
    // Hosts that serve SSH elsewhere than their web site.
    if host == "ssh.github.com" {
        host = "github.com".into();
    }
    if host == "ssh.dev.azure.com" || host.ends_with(".vs-ssh.visualstudio.com") {
        // `v3/org/project/repo` over SSH is `org/project/_git/repo` on the web.
        let parts: Vec<&str> = path.split('/').collect();
        if let ["v3", org, project, repo] = parts[..] {
            path = format!("{org}/{project}/_git/{repo}");
        }
        host = "dev.azure.com".into();
    }
    if host.is_empty() || path.is_empty() {
        return None;
    }
    if let Some(p) = web_port {
        host = format!("{host}:{p}");
    }
    Some(Remote { host, path })
}

/// The kind of a host: the user's setting first, then the well-known
/// hosts.
pub fn forge(host: &str, overrides: &[(String, Forge)]) -> Option<Forge> {
    let bare = host.split(':').next().unwrap_or(host);
    if let Some((_, f)) = overrides.iter().find(|(h, _)| h == host || h == bare) {
        return Some(*f);
    }
    Some(match bare {
        "github.com" => Forge::GitHub,
        "gitlab.com" => Forge::GitLab,
        "bitbucket.org" => Forge::Bitbucket,
        "codeberg.org" | "gitea.com" => Forge::Gitea,
        "git.sr.ht" => Forge::SourceHut,
        "dev.azure.com" => Forge::Azure,
        h if h.ends_with(".visualstudio.com") => Forge::Azure,
        h if h.starts_with("gitlab.") => Forge::GitLab,
        h if h.starts_with("github.") => Forge::GitHub,
        _ => return None,
    })
}

/// What a link shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link<'a> {
    /// A file at a branch or a commit, perhaps some of its lines.
    File {
        /// A branch name or a commit's hash.
        rev: &'a str,
        /// Whether `rev` is a hash (Gitea writes those differently).
        is_commit: bool,
        /// The file's path from the repository's root.
        path: &'a str,
        /// The first and last line, from 1.
        lines: Option<(u32, u32)>,
    },
    /// A commit.
    Commit(&'a str),
}

/// The web address of `link` on `remote`.
pub fn web_url(remote: &Remote, forge: Forge, link: &Link<'_>) -> String {
    let base = format!("https://{}/{}", remote.host, remote.path);
    match *link {
        Link::Commit(hash) => match forge {
            Forge::GitLab => format!("{base}/-/commit/{hash}"),
            Forge::Bitbucket => format!("{base}/commits/{hash}"),
            Forge::GitHub | Forge::Gitea | Forge::SourceHut | Forge::Azure => {
                format!("{base}/commit/{hash}")
            }
        },
        Link::File {
            rev,
            is_commit,
            path,
            lines,
        } => {
            let p = encode(path);
            let r = encode(rev);
            match forge {
                Forge::GitHub => format!("{base}/blob/{r}/{p}{}", anchor(lines, "#L", "-L")),
                Forge::GitLab => format!("{base}/-/blob/{r}/{p}{}", anchor(lines, "#L", "-")),
                Forge::Bitbucket => format!("{base}/src/{r}/{p}{}", anchor(lines, "#lines-", ":")),
                Forge::Gitea => {
                    let kind = if is_commit { "commit" } else { "branch" };
                    format!("{base}/src/{kind}/{r}/{p}{}", anchor(lines, "#L", "-L"))
                }
                Forge::SourceHut => format!("{base}/tree/{r}/item/{p}{}", anchor(lines, "#L", "-")),
                Forge::Azure => {
                    let version = if is_commit {
                        format!("GC{rev}")
                    } else {
                        format!("GB{rev}")
                    };
                    let mut u = format!("{base}?path=/{p}&version={}", encode(&version));
                    if let Some((a, b)) = lines {
                        u.push_str(&format!(
                            "&line={a}&lineEnd={b}&lineStartColumn=1&lineEndColumn=1"
                        ));
                    }
                    u
                }
            }
        }
    }
}

fn anchor(lines: Option<(u32, u32)>, start: &str, range: &str) -> String {
    match lines {
        None => String::new(),
        Some((a, b)) if a == b => format!("{start}{a}"),
        Some((a, b)) => format!("{start}{a}{range}{b}"),
    }
}

/// A path for a URL: each part percent-encoded, the slashes kept.
fn encode(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(host: &str, path: &str) -> Option<Remote> {
        Some(Remote {
            host: host.into(),
            path: path.into(),
        })
    }

    #[test]
    fn remote_urls() {
        let gh = remote("github.com", "getkalem/plugins");
        assert_eq!(parse_remote("https://github.com/getkalem/plugins.git"), gh);
        assert_eq!(parse_remote("https://user@github.com/getkalem/plugins"), gh);
        assert_eq!(parse_remote("git@github.com:getkalem/plugins.git"), gh);
        assert_eq!(
            parse_remote("ssh://git@ssh.github.com:443/getkalem/plugins.git"),
            gh
        );
        assert_eq!(
            parse_remote("https://gitlab.example.com:8443/a/b/c.git"),
            remote("gitlab.example.com:8443", "a/b/c")
        );
        assert_eq!(
            parse_remote("git@ssh.dev.azure.com:v3/org/proj/repo"),
            remote("dev.azure.com", "org/proj/_git/repo")
        );
        assert_eq!(parse_remote("/srv/git/repo.git"), None);
        assert_eq!(parse_remote("file:///srv/git/repo.git"), None);
    }

    #[test]
    fn links_by_forge() {
        let r = parse_remote("git@github.com:o/r.git").unwrap();
        let f = forge(&r.host, &[]).unwrap();
        let file = Link::File {
            rev: "main",
            is_commit: false,
            path: "src/a b.rs",
            lines: Some((10, 20)),
        };
        assert_eq!(
            web_url(&r, f, &file),
            "https://github.com/o/r/blob/main/src/a%20b.rs#L10-L20"
        );
        assert_eq!(
            web_url(&r, f, &Link::Commit("abc")),
            "https://github.com/o/r/commit/abc"
        );
        let gl = Remote {
            host: "gitlab.com".into(),
            path: "g/s/r".into(),
        };
        let one = Link::File {
            rev: "abc",
            is_commit: true,
            path: "x",
            lines: Some((3, 3)),
        };
        assert_eq!(
            web_url(&gl, Forge::GitLab, &one),
            "https://gitlab.com/g/s/r/-/blob/abc/x#L3"
        );
        assert_eq!(
            web_url(&gl, Forge::Bitbucket, &file),
            "https://gitlab.com/g/s/r/src/main/src/a%20b.rs#lines-10:20"
        );
        assert_eq!(
            web_url(&gl, Forge::Gitea, &one),
            "https://gitlab.com/g/s/r/src/commit/abc/x#L3"
        );
        assert!(web_url(&gl, Forge::Azure, &file).ends_with("?path=/src/a%20b.rs&version=GBmain&line=10&lineEnd=20&lineStartColumn=1&lineEndColumn=1"));
    }

    #[test]
    fn hosts() {
        assert_eq!(forge("github.com", &[]), Some(Forge::GitHub));
        assert_eq!(forge("gitlab.example.com:8443", &[]), Some(Forge::GitLab));
        assert_eq!(forge("git.example.com", &[]), None);
        assert_eq!(
            forge(
                "git.example.com",
                &[("git.example.com".into(), Forge::Gitea)]
            ),
            Some(Forge::Gitea)
        );
    }
}
