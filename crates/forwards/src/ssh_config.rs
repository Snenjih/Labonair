// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! `~/.ssh/config` `LocalForward` / `RemoteForward` / `DynamicForward` lines
//! as saved forwards (and back).

use labonair_ssh::ForwardKind;

use crate::saved::SavedForward;

/// Splits `[host:]port` (IPv6 hosts in brackets) into `(host, port)`.
fn split_endpoint(s: &str) -> Option<(Option<String>, u16)> {
    match s.rsplit_once(':') {
        Some((host, port)) => {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            Some((Some(host.to_string()), port.parse().ok()?))
        }
        None => Some((None, s.parse().ok()?)),
    }
}

fn bracket(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// Parses one config line such as `LocalForward 8080 db:5432` or
/// `DynamicForward 127.0.0.1:1080`. `None` for anything else or malformed
/// values. The id is derived from the endpoint so re-imports de-duplicate.
pub fn from_ssh_config_line(line: &str) -> Option<SavedForward> {
    let line = line.trim();
    let sep = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let kind = match line[..sep].to_ascii_lowercase().as_str() {
        "localforward" => ForwardKind::Local,
        "remoteforward" => ForwardKind::Remote,
        "dynamicforward" => ForwardKind::Dynamic,
        _ => return None,
    };
    let mut args = line[sep..]
        .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
        .split_whitespace();
    let (listen_host, listen_port) = split_endpoint(args.next()?)?;
    let listen_host = match listen_host.as_deref() {
        None | Some("") => "127.0.0.1".to_string(),
        Some("*") => "0.0.0.0".to_string(),
        Some(h) => h.to_string(),
    };
    let (target_host, target_port) = if kind == ForwardKind::Dynamic {
        (String::new(), 0)
    } else {
        match split_endpoint(args.next()?)? {
            (Some(h), p) if !h.is_empty() && p != 0 => (h, p),
            _ => return None,
        }
    };
    let tag = match kind {
        ForwardKind::Local => 'L',
        ForwardKind::Remote => 'R',
        ForwardKind::Dynamic => 'D',
    };
    Some(SavedForward {
        id: format!("cfg-{tag}-{listen_host}-{listen_port}"),
        kind,
        name: None,
        listen_host,
        listen_port,
        target_host,
        target_port,
        auto_start: true,
    })
}

/// The config line for a saved forward (without indentation or newline).
pub fn to_ssh_config_line(f: &SavedForward) -> String {
    let bind = match f.listen_host.as_str() {
        "" | "127.0.0.1" => String::new(),
        "0.0.0.0" => "*:".to_string(),
        h => format!("{}:", bracket(h)),
    };
    let listen = format!("{bind}{}", f.listen_port);
    match f.kind {
        ForwardKind::Local => format!(
            "LocalForward {listen} {}:{}",
            bracket(&f.target_host),
            f.target_port
        ),
        ForwardKind::Remote => format!(
            "RemoteForward {listen} {}:{}",
            bracket(&f.target_host),
            f.target_port
        ),
        ForwardKind::Dynamic => format!("DynamicForward {listen}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_kinds() {
        let l = from_ssh_config_line("LocalForward 8080 db.internal:5432").unwrap();
        assert_eq!(
            (l.kind, l.listen_host.as_str(), l.listen_port),
            (ForwardKind::Local, "127.0.0.1", 8080)
        );
        assert_eq!(
            (l.target_host.as_str(), l.target_port),
            ("db.internal", 5432)
        );
        let r = from_ssh_config_line("remoteforward = *:9000 localhost:3000").unwrap();
        assert_eq!(
            (r.kind, r.listen_host.as_str()),
            (ForwardKind::Remote, "0.0.0.0")
        );
        let d = from_ssh_config_line("DynamicForward [::1]:1080").unwrap();
        assert_eq!(
            (d.kind, d.listen_host.as_str(), d.listen_port),
            (ForwardKind::Dynamic, "::1", 1080)
        );
        assert_eq!(d.target_port, 0);
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "LocalForward 8080",
            "LocalForward x db:1",
            "LocalForward 1 db",
            "ForwardAgent yes",
            "DynamicForward",
            "LocalForward 1 db:0",
        ] {
            assert!(from_ssh_config_line(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn round_trips() {
        for line in [
            "LocalForward 8080 db:5432",
            "LocalForward 10.0.0.1:8080 db:5432",
            "RemoteForward *:9000 localhost:3000",
            "DynamicForward 1080",
            "LocalForward [::1]:1 [fe80::1]:2",
        ] {
            let f = from_ssh_config_line(line).unwrap();
            assert_eq!(to_ssh_config_line(&f), line);
        }
    }
}
