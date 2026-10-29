//! Servidor HTTP mínimo para testes: rotas fixas e registro das requisições.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub type Route = Box<dyn Fn(&str, &str, &str) -> Option<(u16, String)> + Send + Sync>;

pub struct Mock {
    pub url: String,
    pub log: Arc<Mutex<Vec<Recorded>>>,
}

impl Mock {
    pub fn start(route: Route) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let log: Arc<Mutex<Vec<Recorded>>> = Arc::default();
        let route = Arc::new(route);
        let l2 = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (log, route) = (l2.clone(), route.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.is_empty() {
                        return;
                    }
                    let mut parts = line.split_whitespace();
                    let method = parts.next().unwrap_or("").to_string();
                    let path = parts.next().unwrap_or("").to_string();
                    let mut headers = Vec::new();
                    let mut len = 0usize;
                    loop {
                        let mut h = String::new();
                        reader.read_line(&mut h).unwrap();
                        let h = h.trim_end();
                        if h.is_empty() {
                            break;
                        }
                        if let Some((k, v)) = h.split_once(':') {
                            if k.eq_ignore_ascii_case("content-length") {
                                len = v.trim().parse().unwrap_or(0);
                            }
                            headers.push((k.trim().to_string(), v.trim().to_string()));
                        }
                    }
                    let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
                    let mut body = Vec::new();
                    if chunked {
                        loop {
                            let mut size = String::new();
                            reader.read_line(&mut size).unwrap();
                            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
                            let mut chunk = vec![0; n + 2];
                            reader.read_exact(&mut chunk).unwrap();
                            if n == 0 {
                                break;
                            }
                            body.extend_from_slice(&chunk[..n]);
                        }
                    } else {
                        body = vec![0; len];
                        reader.read_exact(&mut body).unwrap();
                    }
                    let body = String::from_utf8_lossy(&body).to_string();
                    let (status, resp) = route(&method, &path, &body).unwrap_or((404, r#"{"message":"Not Found"}"#.into()));
                    log.lock().unwrap().push(Recorded { method, path, headers, body });
                    let mut s = stream;
                    let _ = write!(
                        s,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                        resp.len()
                    );
                });
            }
        });
        Self { url, log }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.log.lock().unwrap().clone()
    }
}
