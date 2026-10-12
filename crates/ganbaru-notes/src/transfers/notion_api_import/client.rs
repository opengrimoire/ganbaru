use reqwest::{
    Method, StatusCode, Url,
    header::{CONTENT_TYPE, HeaderMap},
};
use serde_json::{Value, json};
use sqlx::__rt::sleep;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub(crate) const NOTION_API_VERSION: &str = "2026-03-11";

const NOTION_API_BASE_URL: &str = "https://api.notion.com/v1/";
const MAX_RETRY_COUNT: usize = 3;
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(350);
const MAX_SUCCESS_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_PAGINATION_PAGES: usize = 100;
const MAX_PAGINATED_OBJECTS: usize = 10_000;
const MAX_CURSOR_BYTES: usize = 2 * 1024;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(10);
const MAX_TOTAL_RETRY_DELAY: Duration = Duration::from_secs(20);

#[derive(Default)]
pub(crate) struct NotionApiStats {
    pub(crate) request_count: i64,
    pub(crate) retry_count: i64,
    pub(crate) rate_limit_count: i64,
}

pub(crate) struct NotionApiClient {
    http: reqwest::Client,
    base_url: Url,
    token: String,
    page_size: i64,
    request_interval: Duration,
    last_request_at: Option<Instant>,
    stats: NotionApiStats,
}

#[derive(Debug)]
pub(crate) struct NotionApiError {
    pub(crate) status: Option<StatusCode>,
    pub(crate) message: String,
}

impl NotionApiError {
    fn request(message: impl Into<String>) -> Self {
        Self {
            status: None,
            message: message.into(),
        }
    }

    fn response(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status: Some(status),
            message: message.into(),
        }
    }
}

impl NotionApiClient {
    pub(crate) fn new(token: String, page_size: i64) -> Result<Self, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "create Notion API HTTP client".to_string())?;
        let base_url = Url::parse(NOTION_API_BASE_URL)
            .map_err(|_| "configure Notion API origin".to_string())?;
        Ok(Self {
            http,
            base_url,
            token,
            page_size,
            request_interval: MIN_REQUEST_INTERVAL,
            last_request_at: None,
            stats: NotionApiStats::default(),
        })
    }

    pub(crate) fn stats(&self) -> &NotionApiStats {
        &self.stats
    }

    pub(crate) async fn retrieve_page(&mut self, page_id: &str) -> Result<Value, NotionApiError> {
        self.send_json(Method::GET, &format!("/pages/{page_id}"), None, None)
            .await
    }

    pub(crate) async fn retrieve_data_source(
        &mut self,
        data_source_id: &str,
    ) -> Result<Value, NotionApiError> {
        self.send_json(
            Method::GET,
            &format!("/data_sources/{data_source_id}"),
            None,
            None,
        )
        .await
    }

    pub(crate) async fn list_block_children(
        &mut self,
        block_id: &str,
        max_results: usize,
    ) -> Result<Vec<Value>, NotionApiError> {
        self.paginated_get(
            &format!("/blocks/{block_id}/children"),
            "block children",
            Some("block"),
            &[],
            max_results,
        )
        .await
    }

    pub(crate) async fn list_comments(
        &mut self,
        block_id: &str,
    ) -> Result<Vec<Value>, NotionApiError> {
        self.paginated_get(
            "/comments",
            "comments",
            Some("comment"),
            &[("block_id", block_id)],
            MAX_PAGINATED_OBJECTS,
        )
        .await
    }

    pub(crate) async fn list_users(&mut self) -> Result<Vec<Value>, NotionApiError> {
        self.paginated_get("/users", "users", Some("user"), &[], MAX_PAGINATED_OBJECTS)
            .await
    }

    pub(crate) async fn query_data_source(
        &mut self,
        data_source_id: &str,
    ) -> Result<Vec<Value>, NotionApiError> {
        let mut results = Vec::new();
        let mut start_cursor: Option<String> = None;
        let mut pagination = PaginationGuard::new(MAX_PAGINATED_OBJECTS);
        loop {
            let mut body = json!({ "page_size": self.page_size });
            if let Some(cursor) = start_cursor.as_ref() {
                body["start_cursor"] = Value::String(cursor.clone());
            }
            let page = self
                .send_json(
                    Method::POST,
                    &format!("/data_sources/{data_source_id}/query"),
                    None,
                    Some(body),
                )
                .await?;
            start_cursor = pagination.accept_page(&page, "data source rows")?;
            append_page_results(&mut results, &page, "data source rows")?;
            if start_cursor.is_none() {
                break;
            }
        }
        Ok(results)
    }

    async fn paginated_get(
        &mut self,
        path: &str,
        label: &'static str,
        result_type: Option<&'static str>,
        extra_query: &[(&str, &str)],
        max_results: usize,
    ) -> Result<Vec<Value>, NotionApiError> {
        let mut results = Vec::new();
        let mut start_cursor: Option<String> = None;
        let mut pagination = PaginationGuard::new(max_results);
        loop {
            let mut owned_query = vec![("page_size".to_string(), self.page_size.to_string())];
            if let Some(cursor) = start_cursor.as_ref() {
                owned_query.push(("start_cursor".to_string(), cursor.clone()));
            }
            for (key, value) in extra_query {
                owned_query.push(((*key).to_string(), (*value).to_string()));
            }
            let borrowed_query = owned_query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let page = self
                .send_json(Method::GET, path, Some(&borrowed_query), None)
                .await?;
            if let Some(expected_type) = result_type {
                for result in page_results(&page, label)? {
                    if result.get("object").and_then(Value::as_str) != Some(expected_type) {
                        return Err(NotionApiError::request(format!(
                            "Notion {label} response included an unexpected object type"
                        )));
                    }
                }
            }
            start_cursor = pagination.accept_page(&page, label)?;
            append_page_results(&mut results, &page, label)?;
            if start_cursor.is_none() {
                break;
            }
        }
        Ok(results)
    }

    async fn send_json(
        &mut self,
        method: Method,
        path: &str,
        query: Option<&[(&str, &str)]>,
        body: Option<Value>,
    ) -> Result<Value, NotionApiError> {
        let mut url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(|_| NotionApiError::request("build Notion API URL"))?;
        if let Some(query_items) = query {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query_items {
                pairs.append_pair(key, value);
            }
        }
        let mut retry_count = 0;
        let mut total_retry_delay = Duration::ZERO;
        loop {
            self.pace_request().await;
            self.stats.request_count += 1;
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(&self.token)
                .header("Notion-Version", NOTION_API_VERSION)
                .header("Accept", "application/json");
            if let Some(payload) = body.clone() {
                request = request.json(&payload);
            }
            let response = request.send().await.map_err(|error| {
                let message = if error.is_timeout() {
                    "Notion API request timed out"
                } else if error.is_connect() {
                    "Notion API connection failed"
                } else {
                    "Notion API request failed"
                };
                NotionApiError::request(message)
            })?;
            let mut response = response;
            let status = response.status();
            let headers = response.headers().clone();
            let (response_cap, response_label) = if status.is_success() {
                (MAX_SUCCESS_RESPONSE_BYTES, "success")
            } else {
                (MAX_ERROR_RESPONSE_BYTES, "error")
            };
            let body =
                read_capped_response_body(&mut response, response_cap, response_label).await?;
            if !response_content_type_is_json(&headers) {
                return Err(NotionApiError::response(
                    status,
                    "Notion API response had an unsupported content type",
                ));
            }
            if status.is_success() {
                return serde_json::from_slice(&body).map_err(|_| {
                    NotionApiError::response(status, "Notion API returned invalid JSON")
                });
            }
            let message = safe_error_message(status);
            if let Some(delay) = retry_delay(status, &headers, retry_count, total_retry_delay) {
                self.stats.retry_count += 1;
                if is_rate_limited(status) {
                    self.stats.rate_limit_count += 1;
                }
                retry_count += 1;
                total_retry_delay += delay;
                sleep(delay).await;
                continue;
            }
            return Err(NotionApiError::response(status, message));
        }
    }

    async fn pace_request(&mut self) {
        if let Some(last_request_at) = self.last_request_at {
            let elapsed = last_request_at.elapsed();
            if elapsed < self.request_interval {
                sleep(self.request_interval - elapsed).await;
            }
        }
        self.last_request_at = Some(Instant::now());
    }
}

async fn read_capped_response_body(
    response: &mut reqwest::Response,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, NotionApiError> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(NotionApiError::response(
            status,
            format!("Notion API {label} response exceeded {max_bytes} bytes"),
        ));
    }
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(max_bytes as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| NotionApiError::response(status, "Notion API response could not be read"))?
    {
        append_capped_response_chunk(&mut body, &chunk, max_bytes, label)
            .map_err(|error| NotionApiError::response(status, error.message))?;
    }
    Ok(body)
}

fn append_capped_response_chunk(
    body: &mut Vec<u8>,
    chunk: &[u8],
    max_bytes: usize,
    label: &str,
) -> Result<(), NotionApiError> {
    if chunk.len() > max_bytes.saturating_sub(body.len()) {
        return Err(NotionApiError::request(format!(
            "Notion API {label} response exceeded {max_bytes} bytes"
        )));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn response_content_type_is_json(headers: &HeaderMap) -> bool {
    let Some(content_type) = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    media_type == "application/json"
        || (media_type.starts_with("application/") && media_type.ends_with("+json"))
}

fn safe_error_message(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "Notion API authentication failed",
        StatusCode::FORBIDDEN => "Notion API access was denied",
        StatusCode::NOT_FOUND => {
            "Notion API object was not found or is not shared with this integration"
        }
        StatusCode::CONFLICT => "Notion API request conflicted with the remote object",
        status if is_rate_limited(status) => "Notion API rate limit was reached",
        status if status.is_server_error() => "Notion API is temporarily unavailable",
        _ => "Notion API request was rejected",
    }
}

fn append_page_results(
    results: &mut Vec<Value>,
    page: &Value,
    label: &str,
) -> Result<(), NotionApiError> {
    let items = page_results(page, label)?;
    results.extend(items.iter().cloned());
    Ok(())
}

fn page_results<'a>(page: &'a Value, label: &str) -> Result<&'a [Value], NotionApiError> {
    page.get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| {
            NotionApiError::request(format!("Notion {label} response did not include results"))
        })
}

struct PaginationGuard {
    page_count: usize,
    object_count: usize,
    max_objects: usize,
    seen_cursors: HashSet<String>,
}

impl PaginationGuard {
    fn new(max_objects: usize) -> Self {
        Self {
            page_count: 0,
            object_count: 0,
            max_objects,
            seen_cursors: HashSet::new(),
        }
    }

    fn accept_page(&mut self, page: &Value, label: &str) -> Result<Option<String>, NotionApiError> {
        let page_objects = page_results(page, label)?.len();
        let next_object_count = self
            .object_count
            .checked_add(page_objects)
            .filter(|count| *count <= self.max_objects)
            .ok_or_else(|| {
                NotionApiError::request(format!(
                    "Notion {label} pagination exceeded {} objects",
                    self.max_objects
                ))
            })?;
        self.page_count += 1;
        if self.page_count > MAX_PAGINATION_PAGES {
            return Err(NotionApiError::request(format!(
                "Notion {label} pagination exceeded {MAX_PAGINATION_PAGES} pages"
            )));
        }
        let cursor = next_cursor(page, label)?;
        if cursor.is_some() && self.page_count >= MAX_PAGINATION_PAGES {
            return Err(NotionApiError::request(format!(
                "Notion {label} pagination exceeded {MAX_PAGINATION_PAGES} pages"
            )));
        }
        if cursor.is_some() && next_object_count >= self.max_objects {
            return Err(NotionApiError::request(format!(
                "Notion {label} pagination exceeded {} objects",
                self.max_objects
            )));
        }
        if let Some(cursor) = cursor.as_ref() {
            if !self.seen_cursors.insert(cursor.clone()) {
                return Err(NotionApiError::request(format!(
                    "Notion {label} pagination returned a repeated cursor"
                )));
            }
        }
        self.object_count = next_object_count;
        Ok(cursor)
    }

    #[cfg(test)]
    fn object_count(&self) -> usize {
        self.object_count
    }
}

fn next_cursor(page: &Value, label: &str) -> Result<Option<String>, NotionApiError> {
    if page.get("has_more").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    let cursor = page
        .get("next_cursor")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            NotionApiError::request(format!(
                "Notion {label} response had has_more without next_cursor"
            ))
        })?;
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES {
        return Err(NotionApiError::request(format!(
            "Notion {label} response included an invalid next_cursor"
        )));
    }
    Ok(Some(cursor.to_string()))
}

pub(crate) fn retry_delay(
    status: StatusCode,
    headers: &HeaderMap,
    retry_count: usize,
    total_retry_delay: Duration,
) -> Option<Duration> {
    if retry_count >= MAX_RETRY_COUNT {
        return None;
    }
    let delay = if is_rate_limited(status) {
        let seconds = headers
            .get("Retry-After")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(1);
        Duration::from_secs(seconds).min(MAX_RETRY_DELAY)
    } else if matches!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) {
        Duration::from_millis(250 * 2_u64.pow(retry_count as u32))
    } else {
        return None;
    };
    let remaining = MAX_TOTAL_RETRY_DELAY.checked_sub(total_retry_delay)?;
    (delay <= remaining).then_some(delay)
}

fn is_rate_limited(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 529
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::JoinHandle;

    struct ScriptedResponse {
        status: &'static str,
        content_type: Option<&'static str>,
        body: Vec<u8>,
        declared_length: Option<usize>,
    }

    impl ScriptedResponse {
        fn json(status: &'static str, body: Value) -> Self {
            Self {
                status,
                content_type: Some("application/json"),
                body: serde_json::to_vec(&body).unwrap(),
                declared_length: None,
            }
        }
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
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

    fn start_scripted_server(
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

    fn test_client(base_url: Url, page_size: i64) -> NotionApiClient {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        NotionApiClient {
            http,
            base_url,
            token: "bearer_secret".to_string(),
            page_size,
            request_interval: Duration::ZERO,
            last_request_at: None,
            stats: NotionApiStats::default(),
        }
    }

    #[test]
    fn oversized_json_response_is_rejected_before_parsing() {
        let (base_url, _, server) = start_scripted_server(vec![ScriptedResponse {
            status: "200 OK",
            content_type: Some("application/json"),
            body: b"{}".to_vec(),
            declared_length: Some(MAX_SUCCESS_RESPONSE_BYTES + 1),
        }]);
        let mut client = test_client(base_url, 100);

        let error = sqlx::__rt::test_block_on(client.retrieve_page("page-id"))
            .expect_err("oversized success response must fail");

        assert!(error.message.contains("success response exceeded"));
        assert!(!error.message.contains("bearer_secret"));
        server.join().unwrap();
    }

    #[test]
    fn oversized_error_body_is_rejected_without_remote_content() {
        let remote_content = "Private workspace title bearer_secret";
        let (base_url, _, server) = start_scripted_server(vec![ScriptedResponse {
            status: "400 Bad Request",
            content_type: Some("application/json"),
            body: format!(r#"{{"message":"{remote_content}"}}"#).into_bytes(),
            declared_length: Some(MAX_ERROR_RESPONSE_BYTES + 1),
        }]);
        let mut client = test_client(base_url, 100);

        let error = sqlx::__rt::test_block_on(client.retrieve_page("page-id"))
            .expect_err("oversized error response must fail");

        assert!(error.message.contains("error response exceeded"));
        assert!(!error.message.contains(remote_content));
        assert!(!error.message.contains("bearer_secret"));
        server.join().unwrap();
    }

    #[test]
    fn malformed_content_type_is_rejected() {
        let (base_url, _, server) = start_scripted_server(vec![ScriptedResponse {
            status: "200 OK",
            content_type: Some("text/html"),
            body: b"{}".to_vec(),
            declared_length: None,
        }]);
        let mut client = test_client(base_url, 100);

        let error = sqlx::__rt::test_block_on(client.retrieve_page("page-id"))
            .expect_err("non-JSON response must fail");

        assert!(error.message.contains("unsupported content type"));
        server.join().unwrap();
    }

    #[test]
    fn query_data_source_collects_valid_multi_page_import_rows() {
        let (base_url, requests, server) = start_scripted_server(vec![
            ScriptedResponse::json(
                "200 OK",
                json!({
                    "results": [{ "id": "row-1" }, { "id": "row-2" }],
                    "has_more": true,
                    "next_cursor": "cursor-a"
                }),
            ),
            ScriptedResponse::json(
                "200 OK",
                json!({
                    "results": [{ "id": "row-3" }],
                    "has_more": false,
                    "next_cursor": null
                }),
            ),
        ]);
        let mut client = test_client(base_url, 2);

        let rows = sqlx::__rt::test_block_on(client.query_data_source("data-source-id")).unwrap();

        assert_eq!(
            rows.iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            vec!["row-1", "row-2", "row-3"]
        );
        server.join().unwrap();
        let requests = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains(r#""start_cursor":"cursor-a""#));
    }

    #[test]
    fn block_pagination_stops_at_the_remaining_import_limit() {
        let (base_url, requests, server) = start_scripted_server(vec![ScriptedResponse::json(
            "200 OK",
            json!({
                "results": [{ "object": "block", "id": "block-1" }],
                "has_more": true,
                "next_cursor": "cursor-a"
            }),
        )]);
        let mut client = test_client(base_url, 100);

        let error = sqlx::__rt::test_block_on(client.list_block_children("page-id", 1))
            .expect_err("pagination must stop when the remaining block capacity is full");

        assert!(error.message.contains("exceeded 1 objects"));
        server.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
    }

    #[test]
    fn stalled_response_read_is_cancelled_when_the_future_is_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (closed_tx, closed_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{",
                )
                .unwrap();
            stream.flush().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut byte = [0_u8; 1];
            let disconnected = match stream.read(&mut byte) {
                Ok(0) => true,
                Err(error)
                    if !matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    true
                }
                Ok(_) | Err(_) => false,
            };
            closed_tx.send(disconnected).unwrap();
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).unwrap();
        let mut client = test_client(base_url, 100);

        let result = sqlx::__rt::test_block_on(sqlx::__rt::timeout(
            Duration::from_millis(500),
            client.retrieve_page("page-id"),
        ));

        assert!(result.is_err());
        assert!(closed_rx.recv_timeout(Duration::from_secs(3)).unwrap());
        server.join().unwrap();
    }

    #[test]
    fn retry_delay_respects_retry_after_for_rate_limits() {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", HeaderValue::from_static("999999"));
        let delay = retry_delay(StatusCode::TOO_MANY_REQUESTS, &headers, 0, Duration::ZERO);
        assert_eq!(delay, Some(MAX_RETRY_DELAY));
    }

    #[test]
    fn retry_delay_stops_at_the_total_wait_budget() {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", HeaderValue::from_static("10"));
        let delay = retry_delay(
            StatusCode::TOO_MANY_REQUESTS,
            &headers,
            2,
            MAX_TOTAL_RETRY_DELAY,
        );
        assert_eq!(delay, None);
    }

    #[test]
    fn pagination_rejects_cursor_cycles() {
        let mut pagination = PaginationGuard::new(10);
        let first = json!({
            "results": [{ "id": "first" }],
            "has_more": true,
            "next_cursor": "cursor-a"
        });
        let repeated = json!({
            "results": [{ "id": "second" }],
            "has_more": true,
            "next_cursor": "cursor-a"
        });

        assert_eq!(
            pagination.accept_page(&first, "test objects").unwrap(),
            Some("cursor-a".to_string())
        );
        let error = pagination
            .accept_page(&repeated, "test objects")
            .expect_err("a repeated cursor must stop pagination");
        assert!(error.message.contains("repeated cursor"));
    }

    #[test]
    fn pagination_rejects_endless_has_more_and_excess_objects() {
        let mut pagination = PaginationGuard::new(1);
        let over_object_limit = json!({
            "results": [{ "id": "first" }, { "id": "second" }],
            "has_more": false,
            "next_cursor": null
        });
        assert!(
            pagination
                .accept_page(&over_object_limit, "test objects")
                .is_err()
        );

        let mut pagination = PaginationGuard::new(MAX_PAGINATED_OBJECTS);
        for page_index in 0..MAX_PAGINATION_PAGES {
            let page = json!({
                "results": [],
                "has_more": true,
                "next_cursor": format!("cursor-{page_index}")
            });
            let result = pagination.accept_page(&page, "test objects");
            if page_index + 1 == MAX_PAGINATION_PAGES {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok());
            }
        }
    }

    #[test]
    fn pagination_accepts_valid_multi_page_results() {
        let mut pagination = PaginationGuard::new(3);
        let first = json!({
            "results": [{ "id": "first" }, { "id": "second" }],
            "has_more": true,
            "next_cursor": "cursor-a"
        });
        let second = json!({
            "results": [{ "id": "third" }],
            "has_more": false,
            "next_cursor": null
        });

        assert_eq!(
            pagination.accept_page(&first, "test objects").unwrap(),
            Some("cursor-a".to_string())
        );
        assert_eq!(
            pagination.accept_page(&second, "test objects").unwrap(),
            None
        );
        assert_eq!(pagination.object_count(), 3);
    }

    #[test]
    fn response_content_type_requires_json() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        assert!(response_content_type_is_json(&headers));
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("text/html"),
        );
        assert!(!response_content_type_is_json(&headers));
    }

    #[test]
    fn safe_error_message_never_returns_remote_content() {
        let remote_content = "Private workspace title and bearer_secret";
        let message = safe_error_message(StatusCode::BAD_REQUEST);
        assert!(!message.contains(remote_content));
        assert!(!message.contains("bearer_secret"));
        assert_eq!(message, "Notion API request was rejected");
    }

    #[test]
    fn capped_body_rejects_oversized_success_and_error_chunks() {
        let mut success = Vec::new();
        let success_error = append_capped_response_chunk(&mut success, b"12345", 4, "success")
            .expect_err("oversized success response must be rejected");
        assert!(success_error.message.contains("success response exceeded"));

        let mut error = Vec::new();
        let error_error = append_capped_response_chunk(&mut error, b"12345", 4, "error")
            .expect_err("oversized error response must be rejected");
        assert!(error_error.message.contains("error response exceeded"));
    }

    #[test]
    fn retry_delay_backs_off_for_transient_server_errors() {
        let headers = HeaderMap::new();
        let delay = retry_delay(StatusCode::SERVICE_UNAVAILABLE, &headers, 1, Duration::ZERO);
        assert_eq!(delay, Some(Duration::from_millis(500)));
    }

    #[test]
    fn next_cursor_requires_cursor_when_response_has_more() {
        let page = json!({ "has_more": true, "results": [] });
        let error = next_cursor(&page, "blocks").expect_err("cursor should be required");
        assert!(error.message.contains("next_cursor"));

        let oversized = json!({
            "has_more": true,
            "results": [],
            "next_cursor": "x".repeat(MAX_CURSOR_BYTES + 1)
        });
        let error = next_cursor(&oversized, "blocks").expect_err("cursor must be bounded");
        assert!(error.message.contains("invalid next_cursor"));
    }
}
