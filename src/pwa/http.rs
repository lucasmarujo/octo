use std::io::{self, Read, Write};

use crate::pwa::assets::{self, Asset};

pub const HEALTH_PATH: &str = "/health";
pub const HEALTH_BODY: &str = "orkai";
pub const SOCKET_PATH: &str = "/ws";
const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_HEADERS: usize = 48;
const SECURITY_HEADERS: &str = "X-Content-Type-Options: nosniff\r\n\
Referrer-Policy: no-referrer\r\n\
X-Frame-Options: DENY\r\n\
Permissions-Policy: camera=(), microphone=(), geolocation=()\r\n\
Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; \
img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    pub fn wants_socket(&self) -> bool {
        self.path == SOCKET_PATH && self.header("upgrade").is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

pub fn read_request(stream: &mut impl Read) -> io::Result<Request> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the request ended early"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&buffer).map_err(|_| invalid("malformed request"))? {
            httparse::Status::Complete(_) => {
                let target = request.path.unwrap_or("/");
                let path = target.split(['?', '#']).next().unwrap_or("/").to_string();
                let headers = request
                    .headers
                    .iter()
                    .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).trim().to_string()))
                    .collect();
                return Ok(Request { method: request.method.unwrap_or_default().to_string(), path, headers });
            }
            httparse::Status::Partial if buffer.len() >= MAX_HEAD_BYTES => {
                return Err(invalid("the request head is too large"));
            }
            httparse::Status::Partial => {}
        }
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub cache: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(status: u16, body: &str) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", cache: "no-store", body: body.as_bytes().to_vec() }
    }

    fn asset(asset: &Asset) -> Self {
        Self { status: 200, content_type: asset.content_type, cache: asset.cache, body: asset.body.to_vec() }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

pub fn respond(request: &Request) -> Response {
    if request.method != "GET" && request.method != "HEAD" {
        return Response::text(405, "method not allowed");
    }
    if request.path == HEALTH_PATH {
        return Response::text(200, HEALTH_BODY);
    }
    match assets::lookup(&request.path) {
        Some(asset) => Response::asset(asset),
        None => Response::text(404, "not found"),
    }
}

pub fn write_response(stream: &mut impl Write, response: &Response, head_only: bool) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nConnection: close\r\n{SECURITY_HEADERS}\r\n",
        response.status,
        reason(response.status),
        response.content_type,
        response.body.len(),
        response.cache,
    );
    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> io::Result<Request> {
        read_request(&mut text.as_bytes())
    }

    #[test]
    fn a_request_keeps_its_path_without_the_query_and_its_headers() {
        let request = read("GET /app.js?v=2 HTTP/1.1\r\nHost: pc\r\nUpgrade: WebSocket\r\n\r\n").unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/app.js");
        assert_eq!(request.header("host"), Some("pc"));
        assert_eq!(request.header("UPGRADE"), Some("WebSocket"));
        assert!(!request.wants_socket(), "only /ws upgrades");
    }

    #[test]
    fn the_socket_path_with_an_upgrade_is_a_socket() {
        let request = read("GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
        assert!(request.wants_socket());
    }

    #[test]
    fn a_truncated_or_oversized_head_is_refused() {
        assert_eq!(read("GET / HTTP/1.1\r\nHost: pc\r\n").unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        let huge = format!("GET / HTTP/1.1\r\nX-Pad: {}\r\n", "a".repeat(MAX_HEAD_BYTES));
        assert_eq!(read(&huge).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(read("NOT HTTP\r\n\r\n").unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_app_the_health_check_and_unknown_paths_answer() {
        let get = |path: &str| respond(&Request { method: "GET".into(), path: path.into(), headers: Vec::new() });
        assert_eq!(get("/").status, 200);
        assert_eq!(get(HEALTH_PATH).body, HEALTH_BODY.as_bytes());
        assert_eq!(get("/../Cargo.toml").status, 404);
        let post = respond(&Request { method: "POST".into(), path: "/".into(), headers: Vec::new() });
        assert_eq!(post.status, 405);
    }

    #[test]
    fn every_response_carries_the_security_headers() {
        let mut out = Vec::new();
        write_response(&mut out, &Response::text(404, "not found"), false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(text.contains("Content-Length: 9\r\n"));
        assert!(text.contains("Content-Security-Policy: default-src 'self'"));
        assert!(text.contains("X-Frame-Options: DENY"));
        assert!(text.ends_with("\r\n\r\nnot found"));
        let mut head = Vec::new();
        write_response(&mut head, &Response::text(200, "body"), true).unwrap();
        assert!(String::from_utf8(head).unwrap().ends_with("\r\n\r\n"));
    }
}
