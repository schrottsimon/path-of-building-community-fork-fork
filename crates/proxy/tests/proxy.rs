//! The proxy against a stand-in for pathofexile.com: allowed paths are
//! forwarded with GGG's status and rate limit headers and cached, nothing
//! else is, and the caller's cookies never reach GGG.

use proxy::{Config, Proxy, allowed};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn allowed_paths() {
    for p in [
        "/character-window/get-characters?accountName=A%230001&realm=pc",
        "/character-window/get-passive-skills?accountName=A%230001&character=x&realm=pc",
        "/character-window/get-items?accountName=A%230001&character=x&realm=pc",
        "/account/view-profile/A%230001",
    ] {
        assert!(allowed(p), "{p}");
    }
    for p in [
        "/",
        "/character-window/get-stash-items?league=x",
        "/account/view-profile/",
        "/account/view-profile/A/characters",
        "/account/view-profile/..",
        "/api/trade/search",
        "/character-window/get-items/../../x",
    ] {
        assert!(!allowed(p), "{p}");
    }
}

/// What the stand-in was asked: the path, and whether cookies or an
/// authorization came with it
type Seen = Arc<Mutex<Vec<(String, bool)>>>;

/// A stand-in for GGG: records what it was asked, answers 200 with a rate
/// limit header, or 403 for the account "Private#0001"
fn fake_ggg() -> (String, Seen, Arc<AtomicUsize>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr().to_ip().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let hits = Arc::new(AtomicUsize::new(0));
    let (seen2, hits2) = (seen.clone(), hits.clone());
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            hits2.fetch_add(1, Ordering::SeqCst);
            let cookie = req
                .headers()
                .iter()
                .any(|h| h.field.equiv("Cookie") || h.field.equiv("Authorization"));
            seen2.lock().unwrap().push((req.url().to_string(), cookie));
            let private = req.url().contains("Private");
            let mut resp = tiny_http::Response::from_string(if private {
                "false"
            } else {
                r#"[{"name":"x"}]"#
            })
            .with_status_code(if private { 403 } else { 200 });
            resp.add_header(
                tiny_http::Header::from_bytes(&b"X-Rate-Limit-Ip"[..], &b"45:60:60"[..]).unwrap(),
            );
            resp.add_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                    .unwrap(),
            );
            let _ = req.respond(resp);
        }
    });
    (addr, seen, hits)
}

fn config(upstream: String) -> Config {
    Config {
        bind: "127.0.0.1:0".into(),
        upstream,
        allow_origin: "https://pob.example.com".into(),
        user_agent: "pob-proxy-test".into(),
        cache_for: Duration::from_secs(60),
    }
}

#[test]
fn forwards_and_caches() {
    let (ggg, seen, hits) = fake_ggg();
    let p = Proxy::new(config(ggg));
    let path = "/character-window/get-characters?accountName=A%230001&realm=pc";
    let r = p.handle("GET", path);
    assert_eq!(r.status, 200);
    assert_eq!(r.body, br#"[{"name":"x"}]"#);
    assert_eq!(r.content_type.as_deref(), Some("application/json"));
    assert!(
        r.headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("x-rate-limit-ip") && v == "45:60:60"),
        "{:?}",
        r.headers
    );
    // Cached: GGG is asked once
    assert_eq!(p.handle("GET", path).status, 200);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(seen.lock().unwrap()[0], (path.to_string(), false));

    // Errors keep GGG's status, as upstream's import tab tells 403 from 404
    let private = "/character-window/get-characters?accountName=Private%230001&realm=pc";
    assert_eq!(p.handle("GET", private).status, 403);
    assert_eq!(p.handle("GET", private).status, 403);
    assert_eq!(hits.load(Ordering::SeqCst), 3, "errors are not cached");

    // Nothing else is forwarded
    assert_eq!(p.handle("GET", "/api/trade/search").status, 404);
    assert_eq!(p.handle("POST", path).status, 405);
    assert_eq!(p.handle("OPTIONS", path).status, 204);
    assert_eq!(hits.load(Ordering::SeqCst), 3);
}

/// Through HTTP: CORS headers for the configured origin, and the browser's
/// cookies stay with the proxy
#[test]
fn serves_http_with_cors() {
    let (ggg, seen, _) = fake_ggg();
    let p = Arc::new(Proxy::new(config(ggg)));
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", server.server_addr().to_ip().unwrap());
    std::thread::spawn(move || proxy::serve(p, server));
    let resp = ureq::get(&format!("{addr}/account/view-profile/A%230001"))
        .set("Cookie", "POESESSID=secret")
        .set("Origin", "https://pob.example.com")
        .call()
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.header("access-control-allow-origin"),
        Some("https://pob.example.com")
    );
    assert!(
        resp.header("access-control-expose-headers")
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("x-rate-limit-ip")
    );
    assert_eq!(
        seen.lock().unwrap().last().unwrap(),
        &("/account/view-profile/A%230001".to_string(), false)
    );
}
