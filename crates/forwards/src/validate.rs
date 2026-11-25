// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Forward spec validation.

use labonair_ssh::{ForwardKind, ForwardSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyListenHost,
    EmptyTargetHost,
    ZeroTargetPort,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::EmptyListenHost => "Listen address must not be empty",
            Self::EmptyTargetHost => "Target host must not be empty",
            Self::ZeroTargetPort => "Target port must be between 1 and 65535",
        })
    }
}

impl std::error::Error for ValidationError {}

/// Ports are `u16`, so only 0 needs checking (`listen_port == 0` is allowed).
pub fn validate_spec(spec: &ForwardSpec) -> Result<(), ValidationError> {
    if spec.listen_host.trim().is_empty() {
        return Err(ValidationError::EmptyListenHost);
    }
    if spec.kind != ForwardKind::Dynamic {
        if spec.target_host.trim().is_empty() {
            return Err(ValidationError::EmptyTargetHost);
        }
        if spec.target_port == 0 {
            return Err(ValidationError::ZeroTargetPort);
        }
    }
    Ok(())
}

/// True when binding `host` exposes the port beyond the loopback interface.
pub fn is_public_bind(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    if h.eq_ignore_ascii_case("localhost") {
        return false;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(ip) => !ip.is_loopback(),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labonair_ssh::ForwardId;

    fn spec(kind: ForwardKind, lh: &str, th: &str, tp: u16) -> ForwardSpec {
        ForwardSpec {
            id: ForwardId("x".into()),
            host_id: "h".into(),
            kind,
            name: None,
            listen_host: lh.into(),
            listen_port: 0,
            target_host: th.into(),
            target_port: tp,
        }
    }

    #[test]
    fn validates() {
        assert!(validate_spec(&spec(ForwardKind::Local, "127.0.0.1", "db", 5432)).is_ok());
        assert!(validate_spec(&spec(ForwardKind::Dynamic, "127.0.0.1", "", 0)).is_ok());
        assert_eq!(
            validate_spec(&spec(ForwardKind::Local, " ", "db", 1)),
            Err(ValidationError::EmptyListenHost)
        );
        assert_eq!(
            validate_spec(&spec(ForwardKind::Remote, "0.0.0.0", "", 1)),
            Err(ValidationError::EmptyTargetHost)
        );
        assert_eq!(
            validate_spec(&spec(ForwardKind::Local, "127.0.0.1", "db", 0)),
            Err(ValidationError::ZeroTargetPort)
        );
    }

    #[test]
    fn public_bind() {
        for h in ["127.0.0.1", "127.0.0.5", "::1", "[::1]", "localhost"] {
            assert!(!is_public_bind(h), "{h}");
        }
        for h in ["0.0.0.0", "::", "192.168.1.2", "example.com", ""] {
            assert!(is_public_bind(h), "{h}");
        }
    }
}
