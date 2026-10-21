use reqwest::{Client, Response, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::Instant;

const YOUTUBE_OEMBED_ENDPOINT: &str = "https://www.youtube.com/oembed";
const MAX_METADATA_VIDEOS: usize = 250;
const MAX_CONCURRENT_REQUESTS: usize = 12;
/// oEmbed documents are a few hundred bytes; larger bodies are refused while streaming.
const MAX_OEMBED_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const METADATA_BATCH_TIMEOUT: Duration = Duration::from_secs(12);

static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
/// Bounds oEmbed requests across concurrent metadata commands and thumbnail lookups.
static OEMBED_REQUEST_LIMIT: Semaphore = Semaphore::const_new(MAX_CONCURRENT_REQUESTS);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubeMetadataRequest {
    pub playlist_id: Option<String>,
    pub video_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubeMetadataItem {
    pub video_id: Option<String>,
    pub title: String,
    pub channel: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubeMetadataResponse {
    pub playlist: Option<MusicYouTubeMetadataItem>,
    pub videos: Vec<MusicYouTubeMetadataItem>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
struct YouTubeOEmbedDocument {
    title: String,
    author_name: String,
    thumbnail_url: Option<String>,
}

fn is_valid_youtube_id(value: &str, minimum_length: usize, maximum_length: usize) -> bool {
    (minimum_length..=maximum_length).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validate_request(request: &MusicYouTubeMetadataRequest) -> Result<(), String> {
    if request.video_ids.len() > 5_000 {
        return Err("A YouTube metadata request cannot contain more than 5000 videos.".to_string());
    }
    if request
        .playlist_id
        .as_deref()
        .is_some_and(|value| !is_valid_youtube_id(value, 6, 100))
    {
        return Err("The YouTube playlist id is invalid.".to_string());
    }
    if request
        .video_ids
        .iter()
        .any(|value| !is_valid_youtube_id(value, 6, 64))
    {
        return Err("A YouTube video id is invalid.".to_string());
    }
    Ok(())
}

async fn fetch_oembed(client: &Client, source_url: String) -> Option<YouTubeOEmbedDocument> {
    let request_url = format!(
        "{YOUTUBE_OEMBED_ENDPOINT}?url={}&format=json",
        percent_encode_query_value(&source_url)
    );
    let _permit = OEMBED_REQUEST_LIMIT.acquire().await.ok()?;
    let response = client
        .get(request_url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let bytes = read_bounded_body(response, MAX_OEMBED_BYTES).await?;
    serde_json::from_slice(&bytes).ok()
}

/// Reads a response body chunk by chunk, abandoning it as soon as it exceeds `limit` bytes.
async fn read_bounded_body(mut response: Response, limit: usize) -> Option<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

/// Reads the thumbnail URL supplied by YouTube for a validated video ID.
pub(super) async fn fetch_video_thumbnail_url(client: &Client, video_id: &str) -> Option<String> {
    let source_url = format!("https://www.youtube.com/watch?v={video_id}");
    fetch_oembed(client, source_url).await?.thumbnail_url
}

fn percent_encode_query_value(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

async fn fetch_playlist(
    client: &Client,
    playlist_id: Option<&str>,
) -> Option<MusicYouTubeMetadataItem> {
    let playlist_id = playlist_id?;
    let source_url = format!("https://www.youtube.com/playlist?list={playlist_id}");
    let document = fetch_oembed(client, source_url).await?;
    Some(MusicYouTubeMetadataItem {
        video_id: None,
        title: document.title,
        channel: document.author_name,
    })
}

async fn fetch_videos(client: &Client, video_ids: &[String]) -> Vec<MusicYouTubeMetadataItem> {
    let mut next_index = 0;
    let mut tasks = JoinSet::new();
    let mut resolved = vec![None; video_ids.len()];
    let deadline = Instant::now() + METADATA_BATCH_TIMEOUT;

    while next_index < video_ids.len() && tasks.len() < MAX_CONCURRENT_REQUESTS {
        spawn_video_request(
            &mut tasks,
            client.clone(),
            next_index,
            video_ids[next_index].clone(),
        );
        next_index += 1;
    }

    while !tasks.is_empty() {
        let result = tokio::select! {
            result = tasks.join_next() => result,
            () = tokio::time::sleep_until(deadline) => {
                tasks.abort_all();
                break;
            }
        };
        if let Some(Ok((index, metadata))) = result {
            resolved[index] = metadata;
        }
        if next_index < video_ids.len() {
            spawn_video_request(
                &mut tasks,
                client.clone(),
                next_index,
                video_ids[next_index].clone(),
            );
            next_index += 1;
        }
    }

    resolved.into_iter().flatten().collect()
}

fn spawn_video_request(
    tasks: &mut JoinSet<(usize, Option<MusicYouTubeMetadataItem>)>,
    client: Client,
    index: usize,
    video_id: String,
) {
    tasks.spawn(async move {
        let source_url = format!("https://www.youtube.com/watch?v={video_id}");
        let metadata =
            fetch_oembed(&client, source_url)
                .await
                .map(|document| MusicYouTubeMetadataItem {
                    video_id: Some(video_id),
                    title: document.title,
                    channel: document.author_name,
                });
        (index, metadata)
    });
}

fn metadata_client() -> Result<&'static Client, String> {
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .redirect(Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .user_agent("Ganbaru/0.1 YouTube metadata preview")
                .build()
                .map_err(|error| format!("Could not prepare the YouTube metadata request: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

#[tauri::command]
pub async fn music_youtube_metadata(
    request: MusicYouTubeMetadataRequest,
) -> Result<MusicYouTubeMetadataResponse, String> {
    validate_request(&request)?;
    let client = metadata_client()?;
    let truncated = request.video_ids.len() > MAX_METADATA_VIDEOS;
    let video_ids = request
        .video_ids
        .iter()
        .take(MAX_METADATA_VIDEOS)
        .cloned()
        .collect::<Vec<_>>();
    let (playlist, videos) = tokio::join!(
        fetch_playlist(client, request.playlist_id.as_deref()),
        fetch_videos(client, &video_ids),
    );
    Ok(MusicYouTubeMetadataResponse {
        playlist,
        videos,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bounded_youtube_identifiers() {
        assert!(
            validate_request(&MusicYouTubeMetadataRequest {
                playlist_id: Some("PLdR7m7PFLzQ5uOyE0psTM3pCleLxeKaCj".to_string()),
                video_ids: vec!["01L4CFQdrWA".to_string()],
            })
            .is_ok()
        );
    }

    #[test]
    fn rejects_identifiers_that_could_change_the_remote_url() {
        assert!(
            validate_request(&MusicYouTubeMetadataRequest {
                playlist_id: Some("playlist&format=xml".to_string()),
                video_ids: vec!["01L4CFQdrWA".to_string()],
            })
            .is_err()
        );
        assert!(
            validate_request(&MusicYouTubeMetadataRequest {
                playlist_id: None,
                video_ids: vec!["../../account".to_string()],
            })
            .is_err()
        );
    }

    /// Serves one response whose body is `body_prefix` followed by an endless stream when
    /// `endless` is set, and returns the URL to request.
    async fn serve_once(body_prefix: &'static [u8], endless: bool) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let head =
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
            if stream.write_all(head.as_bytes()).await.is_err()
                || stream.write_all(body_prefix).await.is_err()
            {
                return;
            }
            let filler = [b' '; 16 * 1024];
            while endless && stream.write_all(&filler).await.is_ok() {}
        });
        format!("http://{address}/oembed")
    }

    async fn read_from(url: String) -> Option<Vec<u8>> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let response = Client::new().get(url).send().await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(10),
            read_bounded_body(response, MAX_OEMBED_BYTES),
        )
        .await
        .expect("an oversized body is abandoned instead of buffered")
    }

    #[tokio::test]
    async fn oembed_body_without_length_is_abandoned_at_the_cap() {
        let url = serve_once(b"{\"title\":\"", true).await;
        assert!(read_from(url).await.is_none());
    }

    #[tokio::test]
    async fn oembed_body_within_the_cap_is_read_whole() {
        let document = br#"{"title":"Song","author_name":"Channel"}"#;
        let url = serve_once(document, false).await;
        assert_eq!(read_from(url).await.as_deref(), Some(&document[..]));
    }

    #[test]
    fn oembed_source_url_is_encoded_as_one_query_value() {
        assert_eq!(
            percent_encode_query_value("https://www.youtube.com/watch?v=01L4CFQdrWA"),
            "https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D01L4CFQdrWA"
        );
    }
}
