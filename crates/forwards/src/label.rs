// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Display labels for forwards.

use labonair_ssh::{ForwardKind, ForwardSpec};

fn hostport(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Where the forward listens (`server:` prefix for remote forwards).
pub fn listen_label(spec: &ForwardSpec) -> String {
    match spec.kind {
        ForwardKind::Remote => format!("server:{}", spec.listen_port),
        _ => hostport(&spec.listen_host, spec.listen_port),
    }
}

pub fn route_label(spec: &ForwardSpec) -> String {
    let target = hostport(&spec.target_host, spec.target_port);
    match spec.kind {
        ForwardKind::Local => format!("{} → {target}", listen_label(spec)),
        ForwardKind::Remote => format!("{} ← {target}", listen_label(spec)),
        ForwardKind::Dynamic => format!("socks5://{}", listen_label(spec)),
    }
}

pub fn kind_glyph(kind: ForwardKind) -> &'static str {
    match kind {
        ForwardKind::Local => "→",
        ForwardKind::Remote => "←",
        ForwardKind::Dynamic => "⇄",
    }
}

pub fn kind_name(kind: ForwardKind) -> &'static str {
    match kind {
        ForwardKind::Local => "Local",
        ForwardKind::Remote => "Remote",
        ForwardKind::Dynamic => "Dynamic (SOCKS5)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labonair_ssh::ForwardId;

    fn spec(kind: ForwardKind, lh: &str, lp: u16, th: &str, tp: u16) -> ForwardSpec {
        ForwardSpec {
            id: ForwardId("x".into()),
            host_id: "h".into(),
            kind,
            name: None,
            listen_host: lh.into(),
            listen_port: lp,
            target_host: th.into(),
            target_port: tp,
        }
    }

    #[test]
    fn labels() {
        let l = spec(ForwardKind::Local, "127.0.0.1", 8080, "db", 5432);
        assert_eq!(route_label(&l), "127.0.0.1:8080 → db:5432");
        let r = spec(ForwardKind::Remote, "127.0.0.1", 9000, "localhost", 3000);
        assert_eq!(route_label(&r), "server:9000 ← localhost:3000");
        let d = spec(ForwardKind::Dynamic, "127.0.0.1", 1080, "", 0);
        assert_eq!(route_label(&d), "socks5://127.0.0.1:1080");
        assert_eq!(
            listen_label(&spec(ForwardKind::Local, "::1", 1, "a", 1)),
            "[::1]:1"
        );
        assert_eq!(kind_name(ForwardKind::Dynamic), "Dynamic (SOCKS5)");
        assert_eq!(kind_glyph(ForwardKind::Local), "→");
    }
}
