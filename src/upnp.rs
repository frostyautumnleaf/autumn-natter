// UPnP port mapping for the remote mode.
//
// The program asks the router to open the remote port, and only when the user
// switched this on in the settings. The steps are the usual ones: look for a
// router with SSDP, read the description it points at, then call the SOAP
// action AddPortMapping. The mapping is taken away again when the program
// stops, because the guard removes it when it is dropped.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use crate::util;

// Address and port every SSDP search goes to.
const SSDP: &str = "239.255.255.250:1900";
// How long one search for a router lasts.
const SEARCH: Duration = Duration::from_secs(2);
// The two services a router may offer for port mapping.
const SERVICES: [&str; 2] = [
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

// The mapping of one port. When the value is dropped, the router is asked to
// remove the mapping again.
pub struct Mapping {
    control_url: String,
    service: String,
    port: u16,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        let body = envelope(
            &self.service,
            "RemovePortMapping",
            &format!(
                "<NewRemoteHost></NewRemoteHost><NewExternalPort>{}</NewExternalPort>\
                 <NewProtocol>TCP</NewProtocol>",
                self.port
            ),
        );
        let _ = call(&self.control_url, &self.service, "RemovePortMapping", &body);
    }
}

// Ask the router to map the port to this machine.
pub fn open(port: u16) -> Result<Mapping, String> {
    let local_ip = util::lan_address().ok_or("this machine has no address on a local network")?;
    let location = search()?;
    let (service, control_url) = describe(&location)?;
    let body = envelope(
        &service,
        "AddPortMapping",
        &format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort>\
             <NewProtocol>TCP</NewProtocol><NewInternalPort>{port}</NewInternalPort>\
             <NewInternalClient>{ip}</NewInternalClient><NewEnabled>1</NewEnabled>\
             <NewPortMappingDescription>Autumn Natter</NewPortMappingDescription>\
             <NewLeaseDuration>0</NewLeaseDuration>",
            port = port,
            ip = local_ip
        ),
    );
    call(&control_url, &service, "AddPortMapping", &body)?;
    Ok(Mapping {
        control_url,
        service,
        port,
    })
}

// Send the SSDP search and take the description address of the first router
// that answers.
fn search() -> Result<String, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("cannot open a UDP socket: {}", e))?;
    socket
        .set_read_timeout(Some(SEARCH))
        .map_err(|e| e.to_string())?;
    socket.set_broadcast(true).map_err(|e| e.to_string())?;
    let request = "M-SEARCH * HTTP/1.1\r\n\
                   HOST: 239.255.255.250:1900\r\n\
                   MAN: \"ssdp:discover\"\r\n\
                   MX: 2\r\n\
                   ST: urn:schemas-upnp-org:service:WANIPConnection:1\r\n\r\n";
    socket
        .send_to(request.as_bytes(), SSDP)
        .map_err(|e| format!("cannot send the search: {}", e))?;

    let deadline = Instant::now() + SEARCH;
    let mut buffer = [0u8; 2048];
    while Instant::now() < deadline {
        match socket.recv(&mut buffer) {
            Ok(size) => {
                let text = String::from_utf8_lossy(&buffer[..size]);
                if let Some(address) = header_value(&text, "location") {
                    return Ok(address.to_string());
                }
            }
            // No answer inside the time, so no router is there.
            Err(_) => break,
        }
    }
    Err("no router answered the search".to_string())
}

// Read the description of the router and find the service that can map a port.
// The answer is the service name and the address its actions live at.
fn describe(location: &str) -> Result<(String, String), String> {
    let text = ureq::get(location)
        .set("user-agent", "Autumn-Natter")
        .call()
        .map_err(|e| format!("cannot read the description of the router: {}", e))?
        .into_string()
        .map_err(|e| e.to_string())?;
    for name in SERVICES {
        let Some(at) = text.find(name) else {
            continue;
        };
        if let Some(path) = tag_value(&text[at..], "controlURL") {
            return Ok((name.to_string(), join_url(location, &path)));
        }
    }
    Err("the router has no service for port mapping".to_string())
}

// Make a SOAP body for one action of one service.
fn envelope(service: &str, action: &str, fields: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"\
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{service}\">{fields}</u:{action}></s:Body></s:Envelope>"
    )
}

// Send one SOAP call to the router.
fn call(url: &str, service: &str, action: &str, body: &str) -> Result<(), String> {
    let soap_action = format!("\"{}#{}\"", service, action);
    let answer = ureq::post(url)
        .set("user-agent", "Autumn-Natter")
        .set("content-type", "text/xml; charset=\"utf-8\"")
        .set("soapaction", &soap_action)
        .send_string(body)
        .map_err(|e| format!("the router did not answer: {}", e))?;
    let status = answer.status();
    let text = answer.into_string().unwrap_or_default();
    // A router that refuses answers with a SOAP fault, sometimes with a good
    // status number.
    if status >= 400 || text.contains("Fault") {
        return Err(format!("the router refused the call (status {})", status));
    }
    Ok(())
}

// Value of one header in the answer of the router. The name is not case
// sensitive.
fn header_value<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    for line in text.lines() {
        if let Some((head, value)) = line.split_once(':') {
            if head.trim().eq_ignore_ascii_case(name) {
                return Some(value.trim());
            }
        }
    }
    None
}

// Text inside one XML element, searched from the start of the given text.
fn tag_value<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim())
}

// The control address in a description is often only a path. Put the start of
// the description address in front of it.
fn join_url(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    let scheme_end = base.find("://").map(|at| at + 3).unwrap_or(0);
    let head = &base[..scheme_end];
    let authority = base[scheme_end..].split('/').next().unwrap_or("");
    if path.starts_with('/') {
        format!("{}{}{}", head, authority, path)
    } else {
        format!("{}{}/{}", head, authority, path)
    }
}
