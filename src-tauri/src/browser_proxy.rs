//! A loopback HTTP proxy for native browser windows. It resolves each public destination once,
//! rejects local and reserved addresses, then connects to that checked address so a DNS change
//! cannot redirect a page request into the user's network.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::OnceLock;
use std::time::Duration;

static ADDRESS: OnceLock<Result<SocketAddr, String>> = OnceLock::new();
const MAX_HEADER: usize = 16 * 1024;

pub fn address() -> Result<SocketAddr, String> {
    ADDRESS
        .get_or_init(|| {
            let listener =
                TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
            let address = listener.local_addr().map_err(|error| error.to_string())?;
            std::thread::Builder::new()
                .name(String::from("browser-public-proxy"))
                .spawn(move || {
                    for incoming in listener.incoming() {
                        if let Ok(client) = incoming {
                            let _ = std::thread::Builder::new()
                                .name(String::from("browser-proxy-connection"))
                                .spawn(move || serve(client));
                        }
                    }
                })
                .map_err(|error| error.to_string())?;
            Ok(address)
        })
        .clone()
}

fn serve(mut client: TcpStream) {
    let _ = client.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = client.set_write_timeout(Some(Duration::from_secs(20)));
    let Ok((head, extra)) = read_head(&mut client) else {
        return;
    };
    let Ok(head_text) = std::str::from_utf8(&head) else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    let Some(first) = head_text.lines().next() else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    let mut parts = first.split_ascii_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    if parts.next().is_some() {
        respond(&mut client, "400 Bad Request");
        return;
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        let Some((host, port)) = connect_authority(target) else {
            respond(&mut client, "403 Forbidden");
            return;
        };
        let Ok(mut upstream) = connect_public(&host, port) else {
            respond(&mut client, "403 Forbidden");
            return;
        };
        let _ = client.set_read_timeout(None);
        let _ = client.set_write_timeout(None);
        let _ = upstream.set_read_timeout(None);
        let _ = upstream.set_write_timeout(None);
        if client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .and_then(|_| upstream.write_all(&extra))
            .is_err()
        {
            return;
        }
        tunnel(client, upstream);
        return;
    }

    let Ok(url) = tauri::Url::parse(target) else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        respond(&mut client, "403 Forbidden");
        return;
    }
    let Some(host) = url.host_str() else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    let Some(port) = url.port_or_known_default() else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    let Ok(mut upstream) = connect_public(host, port) else {
        respond(&mut client, "403 Forbidden");
        return;
    };

    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let Some(headers) = head_text.split_once("\r\n") else {
        respond(&mut client, "400 Bad Request");
        return;
    };
    let upgrade = headers.1.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("upgrade") && !value.trim().is_empty()
        })
    });
    let mut output = format!("{method} {path} {version}\r\n");
    for line in headers.1.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let name = line.split_once(':').map(|(name, _)| name).unwrap_or("");
        if name.eq_ignore_ascii_case("proxy-authorization")
            || name.eq_ignore_ascii_case("proxy-connection")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        output.push_str(line);
        output.push_str("\r\n");
    }
    output.push_str(if upgrade {
        "Connection: Upgrade\r\n\r\n"
    } else {
        "Connection: close\r\n\r\n"
    });
    let _ = client.set_read_timeout(None);
    let _ = client.set_write_timeout(None);
    let _ = upstream.set_read_timeout(None);
    let _ = upstream.set_write_timeout(None);
    if upstream
        .write_all(output.as_bytes())
        .and_then(|_| upstream.write_all(&extra))
        .is_err()
    {
        return;
    }
    tunnel(client, upstream);
}

fn read_head(client: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut data = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    loop {
        let read = client.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err(String::from("browser closed the proxy connection"));
        }
        data.extend_from_slice(&chunk[..read]);
        if let Some(end) = data.windows(4).position(|part| part == b"\r\n\r\n") {
            let extra = data.split_off(end + 4);
            data.truncate(end + 4);
            return Ok((data, extra));
        }
        if data.len() > MAX_HEADER {
            return Err(String::from("browser request header is too large"));
        }
    }
}

fn connect_authority(authority: &str) -> Option<(String, u16)> {
    let parsed = tauri::Url::parse(&format!("https://{authority}")).ok()?;
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.path() != "/" {
        return None;
    }
    Some((
        parsed.host_str()?.to_string(),
        parsed.port_or_known_default()?,
    ))
}

fn connect_public(host: &str, port: u16) -> Result<TcpStream, String> {
    if private_hostname(host) {
        return Err(String::from("private host"));
    }
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| !autumn_natter_core::search::public_ip(address.ip()))
    {
        return Err(String::from("private or reserved address"));
    }
    let mut last = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, Duration::from_secs(8)) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
    }
    Err(last.map_or_else(
        || String::from("host has no usable address"),
        |error| error.to_string(),
    ))
}

fn private_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host == "metadata.google.internal"
}

fn tunnel(mut client: TcpStream, mut upstream: TcpStream) {
    let Ok(mut client_read) = client.try_clone() else {
        return;
    };
    let Ok(mut upstream_write) = upstream.try_clone() else {
        return;
    };
    let outgoing = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_read, &mut upstream_write);
        let _ = upstream_write.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream, &mut client);
    let _ = client.shutdown(std::net::Shutdown::Write);
    let _ = outgoing.join();
}

fn respond(client: &mut TcpStream, status: &str) {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = client.write_all(response.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::connect_authority;
    use autumn_natter_core::search::public_ip;
    use std::net::IpAddr;

    fn address(value: &str) -> IpAddr {
        value.parse().expect("test address parses")
    }

    #[test]
    fn proxy_blocks_private_reserved_and_special_addresses() {
        for value in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.2.1",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
            "2001:db8::1",
            "2002::1",
        ] {
            assert!(!public_ip(address(value)), "{value} must be blocked");
        }
        assert!(public_ip(address("1.1.1.1")));
        assert!(public_ip(address("2001:4860:4860::8888")));
        assert!(!public_ip(address("64:ff9b::c0a8:101")));
    }

    #[test]
    fn proxy_parses_only_host_port_connect_targets() {
        assert_eq!(
            connect_authority("example.com:443"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(
            connect_authority("127.0.0.1:8080"),
            Some(("127.0.0.1".into(), 8080))
        );
        assert_eq!(connect_authority("https://example.com"), None);
    }
}
