//! The edit-mode preview proxy: a tiny HTTP/1.1 reverse proxy on
//! `127.0.0.1:<random>` in front of the workspace's **loopback** dev server.
//!
//! It exists so the studio can add one script, the element→source bridge
//! (`bridge.js`), to the running app without changing the project. It:
//! - connects only to the one [`PreviewTarget`] it was started with (never to
//!   a host named in a request: absolute-form targets are refused);
//! - answers only requests whose `Host` is its own address (DNS-rebinding
//!   guard) and refuses `CONNECT`;
//! - injects `<script src="/__next_agent/bridge.js">` into HTML pages, drops
//!   `X-Frame-Options` and a CSP `frame-ancestors` so the studio may frame it,
//!   and rewrites redirects to the dev server into relative ones;
//! - tunnels WebSocket upgrades (Vite HMR) byte for byte;
//! - streams everything else unchanged.
//!
//! Transport: `std::net` only (see `docs/architecture/Module Boundaries.md`).

use crate::ssrf::PreviewTarget;
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Path the bridge script is served at.
pub const BRIDGE_PATH: &str = "/__next_agent/bridge.js";
const MAX_HEAD: usize = 64 * 1024;
const MAX_HTML: usize = 16 * 1024 * 1024;
const MAX_REQUEST_BODY: u64 = 64 * 1024 * 1024;

/// The bridge script with the studio's origin filled in. The origin is
/// checked to be `scheme://host[:port]` so it can't break out of the JS
/// string it is placed in.
///
/// # Errors
/// [`crate::DesignError::Blocked`] for anything that isn't a plain origin.
pub fn bridge_script(parent_origin: &str) -> Result<String, crate::DesignError> {
    let bad = || crate::DesignError::Blocked(format!("studio origin `{parent_origin}`"));
    let (scheme, rest) = parent_origin.split_once("://").ok_or_else(bad)?;
    let (host, port) = rest
        .split_once(':')
        .map_or((rest, None), |(h, p)| (h, Some(p)));
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    let port_ok =
        port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()));
    if !matches!(scheme, "http" | "https" | "tauri") || !host_ok || !port_ok {
        return Err(bad());
    }
    Ok(include_str!("bridge.js").replace("__NA_PARENT_ORIGIN__", parent_origin))
}

/// A running proxy. Dropping it stops accepting connections.
pub struct PreviewProxy {
    addr: SocketAddr,
    target: PreviewTarget,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct Config {
    target: PreviewTarget,
    own_hosts: [String; 2],
    bridge: Arc<String>,
}

impl PreviewProxy {
    /// Bind `127.0.0.1:0` and serve `target`. `bridge_js` is served at
    /// [`BRIDGE_PATH`].
    ///
    /// # Errors
    /// When the listener can't bind.
    pub fn start(target: PreviewTarget, bridge_js: String) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let config = Config {
            target: target.clone(),
            own_hosts: [
                format!("127.0.0.1:{}", addr.port()),
                format!("localhost:{}", addr.port()),
            ],
            bridge: Arc::new(bridge_js),
        };
        let flag = Arc::clone(&stop);
        let join = std::thread::Builder::new()
            .name("design-preview-proxy".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let config = config.clone();
                    let _ = std::thread::Builder::new()
                        .name("design-preview-conn".into())
                        .spawn(move || {
                            let _ = handle(stream, &config);
                        });
                }
            })?;
        Ok(Self {
            addr,
            target,
            stop,
            join: Some(join),
        })
    }

    /// `http://127.0.0.1:<port>`.
    #[must_use]
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    /// The URL to load in the preview iframe (the target's start path).
    #[must_use]
    pub fn url(&self) -> String {
        format!("{}{}", self.origin(), self.target.path)
    }

    #[must_use]
    pub fn target(&self) -> &PreviewTarget {
        &self.target
    }

    /// Stop accepting connections (open ones finish on their own).
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for PreviewProxy {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A parsed request or response head.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Head {
    first: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    fn remove(&mut self, name: &str) {
        self.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    }
    fn set(&mut self, name: &str, value: String) {
        self.remove(name);
        self.headers.push((name.to_string(), value));
    }
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!("{}\r\n", self.first);
        for (k, v) in &self.headers {
            let _ = write!(out, "{k}: {v}\r\n");
        }
        out.push_str("\r\n");
        out.into_bytes()
    }
}

/// Read a head up to the blank line.
fn read_head<R: BufRead>(reader: &mut R) -> io::Result<Head> {
    let mut lines = Vec::new();
    let mut total = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        total += n;
        if total > MAX_HEAD {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "head too large"));
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            break;
        }
        lines.push(line);
    }
    let mut it = lines.into_iter();
    let first = it
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty head"))?;
    let headers = it
        .filter_map(|l| {
            l.split_once(':')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    Ok(Head { first, headers })
}

fn respond(client: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    client.write_all(head.as_bytes())?;
    client.write_all(body)?;
    client.flush()
}

#[allow(clippy::too_many_lines)] // one linear request → response path
fn handle(client: TcpStream, config: &Config) -> io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut reader = BufReader::new(client.try_clone()?);
    let mut client = client;
    let mut head = read_head(&mut reader)?;
    let mut parts = head.first.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let host = head.get("host").unwrap_or_default().to_ascii_lowercase();
    if !config.own_hosts.contains(&host) {
        return respond(
            &mut client,
            "421 Misdirected Request",
            "text/plain",
            b"wrong host",
        );
    }
    if method.eq_ignore_ascii_case("CONNECT") || !target.starts_with('/') {
        return respond(&mut client, "400 Bad Request", "text/plain", b"not a proxy");
    }
    if target == BRIDGE_PATH || target.starts_with(&format!("{BRIDGE_PATH}?")) {
        return respond(
            &mut client,
            "200 OK",
            "text/javascript; charset=utf-8",
            config.bridge.as_bytes(),
        );
    }
    let upgrade = head
        .get("upgrade")
        .is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    let body_len: u64 = head
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if head.get("transfer-encoding").is_some() {
        return respond(
            &mut client,
            "411 Length Required",
            "text/plain",
            b"chunked request bodies are not supported",
        );
    }
    if body_len > MAX_REQUEST_BODY {
        return respond(
            &mut client,
            "413 Payload Too Large",
            "text/plain",
            b"too large",
        );
    }

    let authority = config.target.authority();
    head.set("Host", authority.clone());
    for name in ["origin", "referer"] {
        if let Some(value) = head.get(name).map(str::to_string) {
            let rewritten = config.own_hosts.iter().fold(value, |v, h| {
                v.replacen(&format!("http://{h}"), &format!("http://{authority}"), 1)
            });
            head.set(name, rewritten);
        }
    }
    head.set("Accept-Encoding", "identity".into());
    head.remove("proxy-connection");
    head.remove("proxy-authorization");
    if !upgrade {
        head.set("Connection", "close".into());
    }

    let Some(mut upstream) = connect_upstream(&config.target) else {
        return respond(
            &mut client,
            "502 Bad Gateway",
            "text/html; charset=utf-8",
            b"<!doctype html><meta charset=utf-8><body style=\"font:14px system-ui;padding:24px\"><p>The dev server is not answering yet. It may still be starting; this page reloads when you press Reload.</p>",
        );
    };
    upstream.write_all(&head.to_bytes())?;
    // Forward the request body (what BufReader already holds, then the rest).
    let mut body = (&mut reader).take(body_len);
    io::copy(&mut body, &mut upstream)?;
    upstream.flush()?;

    if upgrade {
        return tunnel(reader, &client, upstream);
    }

    upstream.set_read_timeout(Some(Duration::from_secs(120)))?;
    let mut up_reader = BufReader::new(upstream);
    let mut response = read_head(&mut up_reader)?;
    response.remove("x-frame-options");
    if let Some(csp) = response.get("content-security-policy").map(str::to_string) {
        let kept: Vec<&str> = csp
            .split(';')
            .map(str::trim)
            .filter(|d| !d.to_ascii_lowercase().starts_with("frame-ancestors") && !d.is_empty())
            .collect();
        response.set("Content-Security-Policy", kept.join("; "));
    }
    if let Some(location) = response.get("location").map(str::to_string) {
        for prefix in [
            format!("http://{authority}"),
            format!("http://127.0.0.1:{}", config.target.port),
        ] {
            if let Some(rest) = location.strip_prefix(&prefix) {
                let rest = if rest.is_empty() { "/" } else { rest };
                response.set("Location", rest.to_string());
            }
        }
    }
    response.set("Connection", "close".into());
    let is_html = response
        .get("content-type")
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("text/html"));
    let has_body = !method.eq_ignore_ascii_case("HEAD")
        && !response.first.contains(" 204 ")
        && !response.first.contains(" 304 ");
    if is_html && has_body {
        let chunked = response
            .get("transfer-encoding")
            .is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
        let length: Option<usize> = response.get("content-length").and_then(|v| v.parse().ok());
        let raw = read_body(&mut up_reader, chunked, length)?;
        let html = inject_bridge(&String::from_utf8_lossy(&raw));
        response.remove("transfer-encoding");
        response.set("Content-Length", html.len().to_string());
        client.write_all(&response.to_bytes())?;
        client.write_all(html.as_bytes())?;
    } else {
        client.write_all(&response.to_bytes())?;
        io::copy(&mut up_reader, &mut client)?;
    }
    client.flush()?;
    let _ = client.shutdown(Shutdown::Write);
    Ok(())
}

fn read_body<R: BufRead>(
    reader: &mut R,
    chunked: bool,
    length: Option<usize>,
) -> io::Result<Vec<u8>> {
    let too_big = || io::Error::new(io::ErrorKind::InvalidData, "page too large");
    let mut out = Vec::new();
    if chunked {
        loop {
            let mut size_line = String::new();
            reader.read_line(&mut size_line)?;
            let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk"))?;
            if size == 0 {
                // Trailers until the blank line.
                loop {
                    let mut l = String::new();
                    if reader.read_line(&mut l)? == 0 || l.trim().is_empty() {
                        break;
                    }
                }
                return Ok(out);
            }
            if out.len() + size > MAX_HTML {
                return Err(too_big());
            }
            let start = out.len();
            out.resize(start + size, 0);
            reader.read_exact(&mut out[start..])?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?;
        }
    }
    match length {
        Some(n) if n > MAX_HTML => Err(too_big()),
        Some(n) => {
            out.resize(n, 0);
            reader.read_exact(&mut out)?;
            Ok(out)
        }
        None => {
            reader.take(MAX_HTML as u64 + 1).read_to_end(&mut out)?;
            if out.len() > MAX_HTML {
                return Err(too_big());
            }
            Ok(out)
        }
    }
}

/// Connect to the dev server. `localhost` dev servers often listen on IPv6
/// only (Vite on macOS binds `::1`), so 127.0.0.1 falls back to `[::1]`.
fn connect_upstream(target: &PreviewTarget) -> Option<TcpStream> {
    let mut candidates = vec![target.ip];
    if target.ip == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        candidates.push(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
    }
    candidates.into_iter().find_map(|ip| {
        TcpStream::connect_timeout(&SocketAddr::new(ip, target.port), Duration::from_secs(5)).ok()
    })
}

/// Add the bridge `<script>` right after `<head …>` (else `<html …>`, else
/// at the start), so it runs before the app's own scripts.
#[must_use]
pub fn inject_bridge(html: &str) -> String {
    let tag = format!("<script src=\"{BRIDGE_PATH}\"></script>");
    let lower = html.to_ascii_lowercase();
    let after_open = |name: &str| -> Option<usize> {
        let mut from = 0;
        while let Some(i) = lower[from..].find(name) {
            let at = from + i;
            let next = lower.as_bytes().get(at + name.len()).copied();
            if matches!(next, Some(b'>' | b' ' | b'\n' | b'\t' | b'\r')) {
                return lower[at..].find('>').map(|e| at + e + 1);
            }
            from = at + name.len();
        }
        None
    };
    let at = after_open("<head")
        .or_else(|| after_open("<html"))
        .unwrap_or(0);
    let mut out = String::with_capacity(html.len() + tag.len());
    out.push_str(&html[..at]);
    out.push_str(&tag);
    out.push_str(&html[at..]);
    out
}

/// Copy bytes both ways until either side closes (WebSocket upgrade).
fn tunnel(
    client_reader: BufReader<TcpStream>,
    client: &TcpStream,
    upstream: TcpStream,
) -> io::Result<()> {
    client.set_read_timeout(None)?;
    let mut up_read = upstream.try_clone()?;
    let mut client_write = client.try_clone()?;
    let down = std::thread::spawn(move || {
        let _ = io::copy(&mut up_read, &mut client_write);
        let _ = client_write.shutdown(Shutdown::Write);
    });
    let mut client_reader = client_reader;
    let mut up_write = upstream;
    let _ = io::copy(&mut client_reader, &mut up_write);
    let _ = up_write.shutdown(Shutdown::Write);
    let _ = down.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    /// A fake dev server answering each connection with `respond(request)`.
    fn upstream(respond: fn(&str) -> Vec<u8>) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let Ok(head) = read_head(&mut reader) else {
                        return;
                    };
                    let request = String::from_utf8(head.to_bytes()).expect("utf8");
                    let mut stream = stream;
                    if request.to_ascii_lowercase().contains("upgrade: websocket") {
                        stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").expect("w");
                        let mut buf = [0u8; 5];
                        reader.read_exact(&mut buf).expect("r");
                        stream.write_all(&buf).expect("echo");
                        return;
                    }
                    let _ = stream.write_all(&respond(&request));
                });
            }
        });
        port
    }

    fn proxy_for(port: u16) -> PreviewProxy {
        let target = PreviewTarget {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            path: "/start".into(),
        };
        PreviewProxy::start(target, "window.__bridge = 1;".into()).expect("proxy")
    }

    fn get(proxy: &PreviewProxy, path: &str, host: Option<&str>, extra: &str) -> String {
        let port = proxy.addr.port();
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let host = host.map_or_else(|| format!("127.0.0.1:{port}"), str::to_string);
        write!(
            s,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept-Encoding: gzip\r\n{extra}\r\n"
        )
        .expect("w");
        let mut out = String::new();
        s.read_to_string(&mut out).expect("read");
        out
    }

    #[test]
    fn injects_bridge_into_html_and_rewrites_headers() {
        let port = upstream(|req| {
            assert!(req.contains("Accept-Encoding: identity"), "{req}");
            assert!(req.contains("Host: localhost:"), "{req}");
            let body = "<!doctype html><html><head><title>x</title></head><body>hi</body></html>";
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'self'; frame-ancestors 'none'\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len()).into_bytes()
        });
        let proxy = proxy_for(port);
        assert!(proxy.url().ends_with("/start"));
        let out = get(&proxy, "/", None, "");
        assert!(out.starts_with("HTTP/1.1 200 OK"), "{out}");
        assert!(
            out.contains("<head><script src=\"/__next_agent/bridge.js\"></script><title>"),
            "{out}"
        );
        assert!(!out.to_ascii_lowercase().contains("x-frame-options"));
        assert!(
            out.contains("Content-Security-Policy: default-src 'self'\r\n"),
            "{out}"
        );
        assert!(!out.contains("chunked"));
    }

    #[test]
    fn serves_bridge_and_refuses_foreign_hosts() {
        let port = upstream(|_| b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec());
        let proxy = proxy_for(port);
        let js = get(&proxy, BRIDGE_PATH, None, "");
        assert!(
            js.contains("text/javascript") && js.ends_with("window.__bridge = 1;"),
            "{js}"
        );
        assert!(get(&proxy, "/", Some("evil.example:80"), "").starts_with("HTTP/1.1 421"));
        assert!(get(&proxy, "http://169.254.169.254/", None, "").starts_with("HTTP/1.1 400"));
    }

    #[test]
    fn streams_other_responses_and_rewrites_redirects() {
        let port = upstream(|req| {
            if req.starts_with("GET /go") {
                b"HTTP/1.1 302 Found\r\nLocation: http://localhost:PORT/next\r\nContent-Length: 0\r\n\r\n".to_vec()
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: 4\r\n\r\nx=1;".to_vec()
            }
        });
        let proxy = proxy_for(port);
        let js = get(&proxy, "/app.js", None, "");
        assert!(
            js.ends_with("\r\n\r\nx=1;") && !js.contains("bridge"),
            "{js}"
        );
        // The fake can't know its port at compile time; check pass-through of foreign Location.
        let redirect = get(&proxy, "/go", None, "");
        assert!(
            redirect.contains("Location: http://localhost:PORT/next"),
            "{redirect}"
        );
    }

    #[test]
    fn tunnels_websocket_upgrades() {
        let port = upstream(|_| Vec::new());
        let proxy = proxy_for(port);
        let mut s = TcpStream::connect(("127.0.0.1", proxy.addr.port())).expect("connect");
        write!(s, "GET /hmr HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nhello", proxy.addr.port()).expect("w");
        let mut out = String::new();
        s.read_to_string(&mut out).expect("read");
        assert!(
            out.starts_with("HTTP/1.1 101") && out.ends_with("hello"),
            "{out}"
        );
    }

    #[test]
    fn localhost_falls_back_to_ipv6() {
        let Ok(listener) = TcpListener::bind(("::1", 0)) else {
            return;
        }; // no IPv6 here
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                let mut reader = BufReader::new(s.try_clone().expect("clone"));
                let _ = read_head(&mut reader);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nv6");
            }
        });
        let proxy = proxy_for(port);
        assert!(get(&proxy, "/", None, "").ends_with("v6"));
    }

    #[test]
    fn dead_upstream_gives_a_friendly_page() {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .expect("b")
            .local_addr()
            .expect("a")
            .port();
        let proxy = proxy_for(port); // nothing listens there once the probe drops
        assert!(get(&proxy, "/", None, "").starts_with("HTTP/1.1 502"));
    }

    #[test]
    fn bridge_origin_is_validated() {
        let js = bridge_script("tauri://localhost").expect("ok");
        assert!(js.contains("const PARENT = \"tauri://localhost\";"));
        assert!(bridge_script("http://localhost:1420").is_ok());
        assert!(bridge_script("http://tauri.localhost").is_ok());
        for bad in [
            "*",
            "http://x\";alert(1);//",
            "javascript://x",
            "http://",
            "http://a:b",
            "http://A.com",
            "file:///x",
        ] {
            assert!(bridge_script(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn injection_positions() {
        assert_eq!(
            inject_bridge("<html lang=en><body>"),
            "<html lang=en><script src=\"/__next_agent/bridge.js\"></script><body>"
        );
        assert!(inject_bridge("<p>frag</p>").starts_with("<script"));
        assert!(inject_bridge("<header></header><HEAD>").contains("<HEAD><script"));
    }
}
