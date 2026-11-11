//! pob-proxy: see crates/proxy/README.md

use std::sync::Arc;

fn main() {
    let config = proxy::Config::from_env().unwrap_or_else(|e| {
        eprintln!("pob-proxy: {e}");
        std::process::exit(2)
    });
    let server = tiny_http::Server::http(&config.bind).unwrap_or_else(|e| {
        eprintln!("pob-proxy: cannot listen on {}: {e}", config.bind);
        std::process::exit(1)
    });
    eprintln!(
        "pob-proxy: listening on {}, forwarding to {} for {}",
        config.bind, config.upstream, config.allow_origin
    );
    proxy::serve(Arc::new(proxy::Proxy::new(config)), server);
}
