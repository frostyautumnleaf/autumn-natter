// Remote mode. It runs the same engine as the desktop window, and serves one
// web page for the other computers of the local network.
//
// The page asks for the state with a short poll. Nothing is pushed to the
// browser: the text of an answer arrives with the next poll. This keeps the
// server small, and a local network answers fast enough for it.
//
// Only a computer of the local network may open the page. A stranger on the
// internet is refused, even when the router maps the port.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tiny_http::{Header, Request, Response, Server};

use crate::config::Config;
use crate::engine::Engine;
use crate::upnp;
use crate::util;
use crate::web;

#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

// How long one wait for a request lasts. After every wait the loop looks at the
// stop flag, so Ctrl-C ends the run in an orderly way.
const WAIT: Duration = Duration::from_millis(200);

// The page, and the icon of the program, which the page asks for once.
const ICON: &[u8] = include_bytes!("../assets/icons/autumn-natter.svg");

// Set by Ctrl-C. A signal handler may only write to a value like this.
#[cfg(unix)]
static STOPPED: AtomicBool = AtomicBool::new(false);

// Serve the page until the program is asked to stop.
pub fn start(cfg: Config) -> Result<(), String> {
    let port = cfg.remote_port;
    let upnp_enabled = cfg.upnp_enabled;
    // The engine is the same one the desktop window drives.
    let engine = Engine::start(cfg);

    let lan = util::lan_address();
    let bind = lan.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let server =
        Server::http((bind, port)).map_err(|e| format!("cannot listen on port {}: {}", port, e))?;

    println!("Autumn Natter serves the page on http://{}:{}", bind, port);
    println!("Open that address in a browser on another computer.");
    if lan.is_none() {
        println!("No local network address was found, so only this machine can connect.");
    }
    // The router is only asked when the user switched this on in the settings.
    let mapping = if upnp_enabled {
        match upnp::open(port) {
            Ok(guard) => {
                println!("The router maps port {} to this machine.", port);
                Some(guard)
            }
            Err(problem) => {
                println!("The router did not map the port: {}", problem);
                None
            }
        }
    } else {
        None
    };

    watch_ctrl_c();

    loop {
        if stop_asked() {
            break;
        }
        match server.recv_timeout(WAIT) {
            Ok(Some(request)) => handle(&engine, request),
            Ok(None) => {}
            Err(problem) => eprintln!("autumn-natter: the web server had a problem: {}", problem),
        }
    }

    println!("autumn-natter: the page is closed, the model is leaving the memory");
    engine.shutdown();
    // Dropping the guard asks the router to take the mapping away again.
    drop(mapping);
    Ok(())
}

// Answer one request.
fn handle(engine: &Engine, mut request: Request) {
    let url = request.url().to_string();
    let (path, query) = split_url(&url);
    let method = request.method().as_str().to_string();

    if !allowed(&request) {
        let text = "Only a computer of the local network may open this page.";
        let _ = request.respond(text_response(text, 403));
        return;
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/") => {
            let _ = request.respond(Response::from_string(web::PAGE).with_header(content_type("text/html; charset=utf-8")));
        }
        ("GET", "/autumn-natter.svg") => {
            let _ = request.respond(
                Response::from_data(ICON)
                    .with_header(content_type("image/svg+xml"))
                    // The icon never changes while the program runs.
                    .with_header(header("cache-control", "public, max-age=86400")),
            );
        }
        ("GET", "/api/state") => {
            let snap = engine.snapshot();
            let value = web::state(&snap, query_u64(&query, "since"));
            let _ = request.respond(json_response(&value));
        }
        ("POST", _) => {
            let mut body = String::new();
            // A call with no body is allowed, so a read error is not a problem.
            let _ = request.as_reader().read_to_string(&mut body);
            let value = action(engine, path.as_str(), &body);
            let _ = request.respond(json_response(&value));
        }
        _ => {
            let _ = request.respond(json_status(404, "there is nothing at that address"));
        }
    }
}

// One call of the page. Every call is answered with a small JSON record, and
// the page then reads the state again.
fn action(engine: &Engine, path: &str, body: &str) -> serde_json::Value {
    let value = serde_json::from_str::<serde_json::Value>(body)
        .unwrap_or_else(|_| serde_json::json!({}));
    match path {
        "/api/send" => {
            let text = text_field(&value, "text");
            if text.trim().is_empty() {
                return web::refused("nothing was written");
            }
            engine.prompt(text);
            web::done()
        }
        "/api/cancel" => {
            engine.cancel();
            web::done()
        }
        "/api/code" => {
            engine.set_code(flag(&value, "on"));
            web::done()
        }
        "/api/sandbox" => {
            engine.set_sandbox(flag(&value, "on"));
            web::done()
        }
        "/api/new_chat" => {
            engine.new_chat();
            web::done()
        }
        "/api/open_chat" => {
            engine.open_chat(text_field(&value, "id"));
            web::done()
        }
        "/api/select_model" => {
            engine.select_model(text_field(&value, "id"));
            web::done()
        }
        "/api/select_repository" => {
            engine.select_repository(text_field(&value, "id"));
            web::done()
        }
        "/api/create_repository" => {
            engine.create_repository(text_field(&value, "name"));
            web::done()
        }
        "/api/dark" => {
            engine.set_dark_mode(flag(&value, "on"));
            web::done()
        }
        // The page only makes the calls above, so anything else is refused.
        other => web::refused(&format!("the page may not call {}", other)),
    }
}

// True when the caller sits on this machine or inside the local network.
fn allowed(request: &Request) -> bool {
    match request.remote_addr() {
        Some(SocketAddr::V4(address)) => {
            let ip = address.ip();
            ip.is_loopback() || ip.is_private() || ip.is_link_local()
        }
        Some(SocketAddr::V6(address)) => {
            let ip = address.ip();
            // fc00::/7 is the private range of IPv6.
            ip.is_loopback() || ip.is_unicast_link_local() || (ip.octets()[0] & 0xfe) == 0xfc
        }
        // The caller is unknown, so it is not trusted.
        None => false,
    }
}

// Split "a/b?x=1" into the path and the query.
fn split_url(url: &str) -> (String, String) {
    match url.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (url.to_string(), String::new()),
    }
}

// A number from the query of the page, for example since=41.
fn query_u64(query: &str, key: &str) -> Option<u64> {
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next() == Some(key) {
            if let Some(value) = parts.next() {
                if let Ok(number) = value.parse::<u64>() {
                    return Some(number);
                }
            }
        }
    }
    None
}

// One text of the JSON body. A missing text gives an empty string.
fn text_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// One switch of the JSON body. A missing switch is off.
fn flag(value: &serde_json::Value, key: &str) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn json_response(value: &serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(value.to_string()).with_header(content_type("application/json; charset=utf-8"))
}

// A JSON answer with a status number that is not 200.
fn json_status(code: u16, text: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(&web::refused(text)).with_status_code(code)
}

fn text_response(text: &str, code: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(text.to_string())
        .with_header(content_type("text/plain; charset=utf-8"))
        .with_status_code(code)
}

// A header of the answers. The two names are the only ones that are needed.
fn content_type(value: &str) -> Header {
    header("content-type", value)
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .unwrap_or_else(|_| Header::from_bytes("content-type", "text/plain").unwrap())
}

// Ctrl-C sets the flag instead of ending the run at once. The loop then takes
// the model out of the memory and asks the router to remove the mapping.
#[cfg(unix)]
fn watch_ctrl_c() {
    // SAFETY: the handler only writes to a stop flag.
    unsafe {
        libc::signal(libc::SIGINT, on_ctrl_c as *const () as libc::sighandler_t);
    }
}

#[cfg(unix)]
extern "C" fn on_ctrl_c(_signal: libc::c_int) {
    STOPPED.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
fn stop_asked() -> bool {
    STOPPED.load(Ordering::Relaxed)
}

// Windows has no signal handler here. Closing the window of the console ends
// the run, and the router drops a mapping when the program does not answer.
#[cfg(not(unix))]
fn watch_ctrl_c() {}

#[cfg(not(unix))]
fn stop_asked() -> bool {
    false
}
