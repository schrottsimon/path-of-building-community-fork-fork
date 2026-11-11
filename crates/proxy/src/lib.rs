//! A proxy for the public character endpoints of pathofexile.com, which
//! browsers cannot call themselves (no CORS headers). The web app's
//! character import (upstream's import tab, account name section) fetches
//! them through it.
//!
//! Only the paths that import uses are forwarded, only with GET, and without
//! the caller's cookies or headers: it can never act as a logged in user
//! (`POESESSID` is never used, see docs/design.md). Responses keep GGG's
//! status code and rate limit headers, and successful ones are cached
//! briefly so a few users clicking around do not hit GGG twice.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Settings, from the environment (see README.md).
#[derive(Clone, Debug)]
pub struct Config {
    /// Address to listen on
    pub bind: String,
    /// Where requests go, without a trailing slash
    pub upstream: String,
    /// Origin allowed to read responses (`Access-Control-Allow-Origin`)
    pub allow_origin: String,
    /// User agent sent to GGG
    pub user_agent: String,
    /// How long successful responses are reused
    pub cache_for: Duration,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let allow_origin = var("POB_PROXY_ALLOW_ORIGIN").ok_or(
            "POB_PROXY_ALLOW_ORIGIN must be set to the web app's origin, e.g. https://pob.example.com",
        )?;
        let cache_secs = match var("POB_PROXY_CACHE_SECS") {
            Some(v) => v
                .parse()
                .map_err(|_| format!("POB_PROXY_CACHE_SECS: not a number: {v}"))?,
            None => 60,
        };
        Ok(Config {
            bind: var("POB_PROXY_BIND").unwrap_or_else(|| "127.0.0.1:8787".into()),
            upstream: var("POB_PROXY_UPSTREAM")
                .unwrap_or_else(|| "https://www.pathofexile.com".into())
                .trim_end_matches('/')
                .to_string(),
            allow_origin,
            user_agent: var("POB_PROXY_USER_AGENT")
                .unwrap_or_else(|| concat!("pob-web-proxy/", env!("CARGO_PKG_VERSION")).into()),
            cache_for: Duration::from_secs(cache_secs),
        })
    }
}

/// Whether a request path (with its query) may be forwarded: the endpoints
/// upstream's import tab downloads for an account name import.
pub fn allowed(path_and_query: &str) -> bool {
    let path = path_and_query.split('?').next().unwrap_or("");
    match path {
        "/character-window/get-characters"
        | "/character-window/get-passive-skills"
        | "/character-window/get-items" => true,
        _ => path
            .strip_prefix("/account/view-profile/")
            .is_some_and(|name| !name.is_empty() && !name.contains('/') && !name.contains("..")),
    }
}

/// A response to hand back to the browser.
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: Option<String>,
    /// GGG's rate limit headers and `Retry-After`
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    fn text(status: u16, msg: &str) -> Reply {
        Reply {
            status,
            content_type: Some("text/plain; charset=utf-8".into()),
            headers: Vec::new(),
            body: msg.as_bytes().to_vec(),
        }
    }
}

/// Headers passed through from GGG's responses
fn passed_through(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("x-rate-limit") || n == "retry-after"
}

/// The response cap: GGG's character responses are a few hundred KB
const MAX_BODY: u64 = 16 * 1024 * 1024;

pub struct Proxy {
    config: Config,
    agent: ureq::Agent,
    cache: Mutex<HashMap<String, (Instant, Reply)>>,
}

impl Proxy {
    pub fn new(config: Config) -> Proxy {
        let agent = ureq::AgentBuilder::new()
            .user_agent(&config.user_agent)
            .timeout(Duration::from_secs(30))
            .redirects(0)
            .build();
        Proxy {
            config,
            agent,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Answers one request: method and path with query, as received.
    pub fn handle(&self, method: &str, path_and_query: &str) -> Reply {
        if method == "OPTIONS" {
            return Reply {
                status: 204,
                content_type: None,
                headers: Vec::new(),
                body: Vec::new(),
            };
        }
        if method != "GET" {
            return Reply::text(405, "only GET is forwarded");
        }
        if !allowed(path_and_query) {
            return Reply::text(404, "not a forwarded path");
        }
        if let Some(hit) = self.cached(path_and_query) {
            return hit;
        }
        let reply = self.fetch(path_and_query);
        if reply.status == 200 {
            let mut cache = self.cache.lock().unwrap();
            let now = Instant::now();
            cache.retain(|_, (at, _)| now.duration_since(*at) < self.config.cache_for);
            cache.insert(path_and_query.to_string(), (now, reply.clone()));
        }
        reply
    }

    fn cached(&self, key: &str) -> Option<Reply> {
        let cache = self.cache.lock().unwrap();
        cache
            .get(key)
            .filter(|(at, _)| at.elapsed() < self.config.cache_for)
            .map(|(_, r)| r.clone())
    }

    fn fetch(&self, path_and_query: &str) -> Reply {
        let url = format!("{}{}", self.config.upstream, path_and_query);
        let resp = match self.agent.get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Reply::text(502, &format!("upstream request failed: {e}")),
        };
        let status = resp.status();
        let content_type = resp.header("content-type").map(str::to_string);
        let headers = resp
            .headers_names()
            .into_iter()
            .filter(|n| passed_through(n))
            .filter_map(|n| resp.header(&n).map(|v| (n.clone(), v.to_string())))
            .collect();
        let mut body = Vec::new();
        if let Err(e) = resp.into_reader().take(MAX_BODY).read_to_end(&mut body) {
            return Reply::text(502, &format!("reading the upstream response failed: {e}"));
        }
        Reply {
            status,
            content_type,
            headers,
            body,
        }
    }
}

/// Serves until the process ends, one thread per request.
pub fn serve(proxy: std::sync::Arc<Proxy>, server: tiny_http::Server) {
    for request in server.incoming_requests() {
        let proxy = proxy.clone();
        std::thread::spawn(move || respond(&proxy, request));
    }
}

fn header(name: &str, value: &str) -> Option<tiny_http::Header> {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn respond(proxy: &Proxy, request: tiny_http::Request) {
    let method = request.method().as_str().to_string();
    let reply = proxy.handle(&method, request.url());
    let mut response = tiny_http::Response::from_data(reply.body).with_status_code(reply.status);
    let expose: Vec<&str> = reply.headers.iter().map(|(n, _)| n.as_str()).collect();
    let mut headers = vec![
        header("Access-Control-Allow-Origin", &proxy.config().allow_origin),
        header("Access-Control-Allow-Methods", "GET, OPTIONS"),
        header("Vary", "Origin"),
        header("Cache-Control", "no-store"),
    ];
    if !expose.is_empty() {
        headers.push(header("Access-Control-Expose-Headers", &expose.join(", ")));
    }
    if let Some(ct) = &reply.content_type {
        headers.push(header("Content-Type", ct));
    }
    for (n, v) in &reply.headers {
        headers.push(header(n, v));
    }
    for h in headers.into_iter().flatten() {
        response.add_header(h);
    }
    let _ = request.respond(response);
}
