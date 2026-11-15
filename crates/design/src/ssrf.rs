// Adapted from Open Design (Apache-2.0, © 2026 Open Design contributors):
// apps/daemon/src/public-asset-fetch.ts and
// packages/contracts/src/api/connectionTest.ts (isBlockedIpv4 / IPv6 rules)
// @ 59e4f2a — modified: Rust, no DNS (we never resolve names), and inverted
// for the preview: the only network target the studio ever connects to is a
// **loopback** dev server; every other address class is refused by name.
//! Address classification for the studio's only network use, the preview.

use crate::DesignError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// What kind of address a host is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    Loopback,
    /// RFC 1918, CGNAT (100.64/10), unique-local IPv6 (`fc00::/7`).
    Private,
    /// 169.254/16, `fe80::/10` — includes cloud metadata (169.254.169.254).
    LinkLocal,
    /// 0/8, ::, multicast and reserved (≥ 224, `ff00::/8`).
    Reserved,
    /// Metadata host names (`metadata.google.internal`, …).
    Metadata,
    /// Anything else (a public address or a DNS name we won't resolve).
    Public,
}

fn v4_kind(ip: Ipv4Addr) -> HostKind {
    let [a, b, ..] = ip.octets();
    match (a, b) {
        (127, _) => HostKind::Loopback,
        (0 | 224..=255, _) => HostKind::Reserved,
        (10, _) | (172, 16..=31) | (192, 168) | (100, 64..=127) => HostKind::Private,
        (169, 254) => HostKind::LinkLocal,
        _ => HostKind::Public,
    }
}

fn v6_kind(ip: Ipv6Addr) -> HostKind {
    if ip.is_loopback() {
        return HostKind::Loopback;
    }
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_kind(v4);
    }
    let first = ip.segments()[0];
    if ip.is_unspecified() || first & 0xff00 == 0xff00 {
        HostKind::Reserved
    } else if first & 0xfe00 == 0xfc00 {
        HostKind::Private
    } else if first & 0xffc0 == 0xfe80 {
        HostKind::LinkLocal
    } else {
        HostKind::Public
    }
}

/// Classify a host (name or literal, IPv6 with or without brackets).
#[must_use]
pub fn host_kind(host: &str) -> HostKind {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let h = h
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or(&h);
    if h == "localhost" || h.ends_with(".localhost") {
        return HostKind::Loopback;
    }
    if matches!(
        h,
        "metadata.google.internal" | "metadata" | "instance-data" | "metadata.azure.internal"
    ) {
        return HostKind::Metadata;
    }
    match h.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4_kind(v4),
        Ok(IpAddr::V6(v6)) => v6_kind(v6),
        Err(_) => HostKind::Public,
    }
}

/// A validated dev-server address the preview proxy may connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTarget {
    /// Literal loopback IP to connect to (`localhost` becomes 127.0.0.1).
    pub ip: IpAddr,
    pub port: u16,
    /// Path (and query) to open first, starting with `/`.
    pub path: String,
}

impl PreviewTarget {
    /// `host:port` for the `Host` header the dev server expects.
    #[must_use]
    pub fn authority(&self) -> String {
        match self.ip {
            IpAddr::V4(ip) if ip == Ipv4Addr::LOCALHOST => format!("localhost:{}", self.port),
            IpAddr::V4(ip) => format!("{ip}:{}", self.port),
            IpAddr::V6(ip) => format!("[{ip}]:{}", self.port),
        }
    }
}

/// Accept only `http://` URLs on a loopback host with an explicit or default
/// port: the workspace's own dev server. Remote, private, link-local,
/// metadata and reserved addresses are refused, as are user-info, other
/// schemes and DNS names other than `localhost`.
///
/// # Errors
/// [`DesignError::Blocked`] with the reason.
pub fn check_preview_url(url: &str) -> Result<PreviewTarget, DesignError> {
    let url = url.trim();
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        DesignError::Blocked(format!("{url} (only http:// dev servers can be previewed)"))
    })?;
    let (authority, path) = rest
        .find(['/', '?', '#'])
        .map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    if authority.contains('@') {
        return Err(DesignError::Blocked(format!(
            "{url} (user info is not allowed)"
        )));
    }
    let (host, port) = split_host_port(authority)
        .ok_or_else(|| DesignError::Blocked(format!("{url} (bad host or port)")))?;
    let kind = host_kind(host);
    if kind != HostKind::Loopback {
        return Err(DesignError::Blocked(format!(
            "{url} ({}; the studio previews only dev servers on this computer)",
            match kind {
                HostKind::Private => "private network address",
                HostKind::LinkLocal => "link-local / cloud metadata address",
                HostKind::Reserved => "reserved address",
                HostKind::Metadata => "cloud metadata host",
                _ => "not a local address",
            }
        )));
    }
    let ip = if host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
    {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .map_err(|_| DesignError::Blocked(url.to_string()))?
    };
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Ok(PreviewTarget { ip, port, path })
}

fn split_host_port(authority: &str) -> Option<(&str, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &authority[..end + 2];
        let port = match &rest[end + 1..] {
            "" => 80,
            p => p.strip_prefix(':')?.parse().ok()?,
        };
        return Some((host, port));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => Some((h, p.parse().ok().filter(|p| *p != 0)?)),
        Some(_) => None,
        None => Some((authority, 80)),
    }
    .filter(|(h, _)| !h.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_open_design_ranges() {
        for (host, kind) in [
            ("127.0.0.1", HostKind::Loopback),
            ("127.8.9.1", HostKind::Loopback),
            ("localhost", HostKind::Loopback),
            ("app.localhost", HostKind::Loopback),
            ("[::1]", HostKind::Loopback),
            ("10.1.2.3", HostKind::Private),
            ("172.16.0.1", HostKind::Private),
            ("172.31.255.1", HostKind::Private),
            ("172.32.0.1", HostKind::Public),
            ("192.168.1.1", HostKind::Private),
            ("100.64.0.1", HostKind::Private),
            ("100.128.0.1", HostKind::Public),
            ("169.254.169.254", HostKind::LinkLocal),
            ("0.0.0.0", HostKind::Reserved),
            ("224.0.0.1", HostKind::Reserved),
            ("255.255.255.255", HostKind::Reserved),
            ("::", HostKind::Reserved),
            ("fc00::1", HostKind::Private),
            ("fd12::1", HostKind::Private),
            ("fe80::1", HostKind::LinkLocal),
            ("ff02::1", HostKind::Reserved),
            ("::ffff:169.254.169.254", HostKind::LinkLocal),
            ("::ffff:a9fe:a9fe", HostKind::LinkLocal),
            ("::ffff:127.0.0.1", HostKind::Loopback),
            ("metadata.google.internal", HostKind::Metadata),
            ("example.com", HostKind::Public),
            ("8.8.8.8", HostKind::Public),
        ] {
            assert_eq!(host_kind(host), kind, "{host}");
        }
    }

    #[test]
    fn preview_accepts_only_loopback_http() {
        let t = check_preview_url("http://localhost:5173/dashboard?x=1").expect("ok");
        assert_eq!(
            (t.ip, t.port, t.path.as_str()),
            (IpAddr::V4(Ipv4Addr::LOCALHOST), 5173, "/dashboard?x=1")
        );
        assert_eq!(t.authority(), "localhost:5173");
        let v6 = check_preview_url("http://[::1]:3000").expect("ok");
        assert_eq!((v6.port, v6.path.as_str()), (3000, "/"));
        assert_eq!(v6.authority(), "[::1]:3000");
        assert_eq!(check_preview_url("http://127.0.0.1").expect("ok").port, 80);
        for bad in [
            "https://localhost:5173",
            "http://example.com",
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.5:8080",
            "http://user:pw@localhost:5173",
            "http://localhost:0",
            "http://localhost:99999",
            "http://localhost.evil.com:5173",
            "http://[fe80::1]:80",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://:5173",
        ] {
            assert!(
                matches!(check_preview_url(bad), Err(DesignError::Blocked(_))),
                "{bad}"
            );
        }
    }
}
