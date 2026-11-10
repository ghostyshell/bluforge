//! Probe TMDB/ChapterDB access with different request shapes.
//! `cargo run -p keepsake-core --example tmdb_probe`

const TMDB: &str = "https://www.themoviedb.org/search?query=Gladiator+II";

fn try_req(name: &str, f: impl Fn(ureq::Request) -> ureq::Request) {
    let req = f(ureq::get(TMDB));
    match req.call() {
        Ok(r) => println!(
            "{name:<34} -> {} len={}",
            r.status(),
            r.header("content-length").unwrap_or("-")
        ),
        Err(e) => println!("{name:<34} -> ERR {e}"),
    }
}

fn main() {
    try_req("no UA at all", |r| r);
    try_req("UA=ureq/2.10.1", |r| r.set("User-Agent", "ureq/2.10.1"));
    try_req("UA=curl/8.7.1", |r| r.set("User-Agent", "curl/8.7.1"));
    try_req("UA=KeepsakeAtelier/0.1", |r| {
        r.set("User-Agent", "KeepsakeAtelier/0.1")
    });
    try_req("UA=browser only", |r| {
        r.set(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        )
    });
    try_req("browser + full headers", |r| {
        r.set(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        )
        .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .set("Accept-Language", "en-US,en;q=0.9")
        .set("Sec-Fetch-Dest", "document")
        .set("Sec-Fetch-Mode", "navigate")
        .set("Sec-Fetch-Site", "none")
        .set("Upgrade-Insecure-Requests", "1")
        .set("Connection", "keep-alive")
    });
    try_req("no UA + Accept-Language", |r| {
        r.set("Accept-Language", "en-US,en;q=0.9")
    });
    try_req("no UA + Accept html", |r| {
        r.set("Accept", "text/html,application/xhtml+xml")
    });
}
