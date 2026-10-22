//! Scripted local HTTP server for Notion API client and import tests.

use reqwest::Url;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

pub(crate) struct ScriptedResponse {
    pub(crate) status: &'static str,
    pub(crate) content_type: Option<&'static str>,
    pub(crate) body: Vec<u8>,
    pub(crate) declared_length: Option<usize>,
}

impl ScriptedResponse {
    pub(crate) fn json(status: &'static str, body: Value) -> Self {
        Self {
            status,
            content_type: Some("application/json"),
            body: serde_json::to_vec(&body).unwrap(),
            declared_length: None,
        }
    }
}

pub(crate) fn read_http_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut expected_length = None;
    loop {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).unwrap_or(0);
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > 64 * 1024 {
            break;
        }
        if expected_length.is_none() {
            if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let header_length = header_end + 4;
                let headers = String::from_utf8_lossy(&request[..header_length]);
                let body_length = headers
                    .lines()
                    .find_map(|line| line.split_once(':'))
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                expected_length = Some(header_length + body_length);
            }
        }
        if expected_length.is_some_and(|length| request.len() >= length) {
            break;
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

pub(crate) fn start_scripted_server(
    responses: Vec<ScriptedResponse>,
) -> (Url, Receiver<String>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (request_tx, request_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            let _ = request_tx.send(request);
            let content_type = response
                .content_type
                .map(|value| format!("Content-Type: {value}\r\n"))
                .unwrap_or_default();
            let declared_length = response.declared_length.unwrap_or(response.body.len());
            write!(
                stream,
                "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
                response.status, content_type, declared_length
            )
            .unwrap();
            stream.write_all(&response.body).unwrap();
            stream.flush().unwrap();
        }
    });
    let base_url = Url::parse(&format!("http://{address}/v1/")).unwrap();
    (base_url, request_rx, handle)
}
