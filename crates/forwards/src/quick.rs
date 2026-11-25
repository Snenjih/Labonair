// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! ssh-style quick specs: `-L [bind:]port:host:hostport`, `-R ...`, `-D [bind:]port`.

use labonair_ssh::ForwardKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSpec {
    pub kind: ForwardKind,
    pub listen_host: String,
    pub listen_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

fn port(s: &str) -> Result<u16, String> {
    s.parse::<u16>().map_err(|_| format!("Invalid port: {s:?}"))
}

/// Splits on `:`, keeping `[v6]` literals together and stripping brackets.
fn split_fields(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_br = false;
    for c in s.chars() {
        match c {
            '[' if !in_br => in_br = true,
            ']' if in_br => in_br = false,
            ':' if !in_br => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    if in_br {
        return Err("Unclosed '[' in address".into());
    }
    out.push(cur);
    Ok(out)
}

pub fn parse_quick(input: &str) -> Result<QuickSpec, String> {
    let input = input.trim();
    let (flag, rest) = input
        .split_once(char::is_whitespace)
        .ok_or_else(|| "Expected -L, -R or -D followed by a spec".to_string())?;
    let rest = rest.trim();
    let kind = match flag {
        "-L" => ForwardKind::Local,
        "-R" => ForwardKind::Remote,
        "-D" => ForwardKind::Dynamic,
        _ => return Err(format!("Unknown flag {flag:?}; use -L, -R or -D")),
    };
    let f = split_fields(rest)?;
    let (bind, f) = match (kind, f.len()) {
        (ForwardKind::Dynamic, 1) | (_, 3) => (None, f),
        (ForwardKind::Dynamic, 2) | (_, 4) => (Some(f[0].clone()), f[1..].to_vec()),
        (ForwardKind::Dynamic, _) => return Err("Expected -D [bind:]port".into()),
        _ => return Err("Expected [bind:]port:host:hostport".into()),
    };
    if bind.as_deref() == Some("") {
        return Err("Bind address must not be empty".into());
    }
    let listen_host = bind.unwrap_or_else(|| {
        // ssh: -R without bind address listens on the server's loopback.
        "127.0.0.1".into()
    });
    let listen_port = port(&f[0])?;
    if kind == ForwardKind::Dynamic {
        return Ok(QuickSpec {
            kind,
            listen_host,
            listen_port,
            target_host: String::new(),
            target_port: 0,
        });
    }
    if f[1].is_empty() {
        return Err("Target host must not be empty".into());
    }
    let target_port = port(&f[2])?;
    if target_port == 0 {
        return Err("Target port must be between 1 and 65535".into());
    }
    Ok(QuickSpec {
        kind,
        listen_host,
        listen_port,
        target_host: f[1].clone(),
        target_port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(kind: ForwardKind, lh: &str, lp: u16, th: &str, tp: u16) -> QuickSpec {
        QuickSpec {
            kind,
            listen_host: lh.into(),
            listen_port: lp,
            target_host: th.into(),
            target_port: tp,
        }
    }

    #[test]
    fn all_forms() {
        assert_eq!(
            parse_quick("-L 8080:db:5432").unwrap(),
            q(ForwardKind::Local, "127.0.0.1", 8080, "db", 5432)
        );
        assert_eq!(
            parse_quick("-L 0.0.0.0:8080:db:5432").unwrap(),
            q(ForwardKind::Local, "0.0.0.0", 8080, "db", 5432)
        );
        assert_eq!(
            parse_quick("-R 9000:localhost:3000").unwrap(),
            q(ForwardKind::Remote, "127.0.0.1", 9000, "localhost", 3000)
        );
        assert_eq!(
            parse_quick("-D 1080").unwrap(),
            q(ForwardKind::Dynamic, "127.0.0.1", 1080, "", 0)
        );
        assert_eq!(
            parse_quick("-D 0.0.0.0:1080").unwrap(),
            q(ForwardKind::Dynamic, "0.0.0.0", 1080, "", 0)
        );
        assert_eq!(
            parse_quick("-L [::1]:8080:db:5432").unwrap(),
            q(ForwardKind::Local, "::1", 8080, "db", 5432)
        );
        assert_eq!(parse_quick("-L 0:db:80").unwrap().listen_port, 0);
    }

    #[test]
    fn errors() {
        for bad in [
            "",
            "-L",
            "-X 1:a:2",
            "-L 8080",
            "-L 8080:db",
            "-L a:db:5432",
            "-L 8080:db:0",
            "-L 8080::5432",
            "-L 70000:db:1",
            "-D",
            "-D x",
            "-D a:b:1080",
            "-L [::1:8080:db:1",
            "-L :8080:db:1",
        ] {
            assert!(parse_quick(bad).is_err(), "{bad:?} should fail");
        }
    }
}
