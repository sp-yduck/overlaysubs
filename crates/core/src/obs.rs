//! Local HTTP server that lets an OBS browser source show the overlay: it serves the overlay page
//! and pushes the same events the desktop overlay gets as Server-Sent Events on `/events`.
//! Bound to 127.0.0.1 only, and only while the user has the OBS option turned on.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A file the server answers with: request path, content type, body.
pub type File = (&'static str, &'static str, &'static str);

/// A stalled browser source must not hold up the session thread that sends the captions.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// The connected browser sources. It outlives the server, so senders need not know whether one runs.
#[derive(Default)]
pub struct Hub {
    clients: Mutex<Vec<TcpStream>>,
    /// The last settings sent, repeated to every new client so it can style itself at once.
    settings: Mutex<String>,
}

fn frame(event: &str, json: &str) -> String {
    format!("event: {event}\ndata: {json}\n\n")
}

impl Hub {
    /// Sends one event to every client and forgets the clients that have gone away.
    /// `json` must be a single line, which is what serde_json writes.
    pub fn send(&self, event: &str, json: &str) {
        let msg = frame(event, json);
        self.clients.lock().unwrap().retain_mut(|c| c.write_all(msg.as_bytes()).is_ok());
    }

    pub fn set_settings(&self, json: String) {
        self.send("settings", &json);
        *self.settings.lock().unwrap() = json;
    }
}

/// The running server. Dropping it closes the port and disconnects the browser sources.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn port(&self) -> u16 {
        self.addr.port()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop so it sees the flag.
        let _ = TcpStream::connect(self.addr);
    }
}

pub fn serve(port: u16, files: &'static [File], hub: Arc<Hub>) -> io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || {
        // shortcut: one request at a time on this thread, enough for a few local browser sources;
        // move to a thread per connection if a slow client ever delays another.
        for stream in listener.incoming() {
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(stream) = stream {
                let _ = handle(stream, addr.port(), files, &hub);
            }
        }
        // Close the port before the streams, so nothing can connect once they have ended.
        drop(listener);
        hub.clients.lock().unwrap().clear();
    });
    Ok(Server { addr, stop })
}

fn handle(mut stream: TcpStream, port: u16, files: &[File], hub: &Hub) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut buf = [0u8; 4096];
    let mut len = 0;
    while !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf[len..])?;
        if n == 0 {
            return Ok(());
        }
        len += n;
    }
    let head = String::from_utf8_lossy(&buf[..len]);
    let mut lines = head.lines();
    let mut request = lines.next().unwrap_or("").split(' ');
    let (method, target) = (request.next().unwrap_or(""), request.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    // A web page on a domain that resolves to 127.0.0.1 could otherwise read the captions (DNS rebinding).
    let host = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.trim());
    if !host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")) {
        return respond(&mut stream, "403 Forbidden", "text/plain", "");
    }
    if method != "GET" {
        return respond(&mut stream, "405 Method Not Allowed", "text/plain", "");
    }
    if path == "/events" {
        // The client list is locked from before the settings are read until the client is added,
        // so a settings change in between cannot be missed.
        let mut clients = hub.clients.lock().unwrap();
        let hello = frame("settings", &hub.settings.lock().unwrap());
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n{hello}").as_bytes())?;
        clients.push(stream);
        return Ok(());
    }
    match files.iter().find(|f| f.0 == path) {
        Some((_, mime, body)) => respond(&mut stream, "200 OK", mime, body),
        None => respond(&mut stream, "404 Not Found", "text/plain", ""),
    }
}

fn respond(stream: &mut TcpStream, status: &str, mime: &str, body: &str) -> io::Result<()> {
    // no-store: an app update must not leave an old overlay page in the browser source's cache.
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &[File] = &[("/", "text/html; charset=utf-8", "<p>overlay</p>")];

    fn request(port: u16, path: &str, host: &str) -> TcpStream {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        s
    }

    fn body(mut s: TcpStream) -> String {
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    /// Reads from an open event stream until `end` has arrived.
    fn read_until(s: &mut TcpStream, end: &str) -> String {
        let mut out = String::new();
        let mut buf = [0u8; 1024];
        while !out.contains(end) {
            let n = s.read(&mut buf).unwrap();
            assert!(n > 0, "stream closed after {out:?}");
            out.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        out
    }

    #[test]
    fn serves_the_page_and_streams_events_to_local_clients_only() {
        let hub = Arc::new(Hub::default());
        hub.set_settings(r#"{"ui_lang":"ja"}"#.into());
        let server = serve(0, FILES, hub.clone()).unwrap();
        let port = server.port();
        let local = format!("127.0.0.1:{port}");

        let page = body(request(port, "/?layer=1", &local));
        assert!(page.starts_with("HTTP/1.1 200 OK") && page.ends_with("<p>overlay</p>"), "{page}");
        assert!(body(request(port, "/settings.json", &local)).starts_with("HTTP/1.1 404"));
        assert!(body(request(port, "/events", "evil.example")).starts_with("HTTP/1.1 403"), "DNS rebinding");

        // A new client first gets the current settings, then every event sent after it connected.
        let mut events = request(port, "/events", &format!("localhost:{port}"));
        assert!(read_until(&mut events, "\n\n").ends_with("event: settings\ndata: {\"ui_lang\":\"ja\"}\n\n"));
        hub.send("caption", r#"{"id":1}"#);
        assert_eq!(read_until(&mut events, "\n\n"), "event: caption\ndata: {\"id\":1}\n\n");

        // Turning the option off closes the port and the open streams.
        drop(server);
        assert_eq!(body(events), "");
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }
}
