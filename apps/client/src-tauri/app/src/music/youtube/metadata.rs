use reqwest::{Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::Instant;

const YOUTUBE_OEMBED_ENDPOINT: &str = "https://www.youtube.com/oembed";
const MAX_METADATA_VIDEOS: usize = 250;
const MAX_CONCURRENT_REQUESTS: usize = 12;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const METADATA_BATCH_TIMEOUT: Duration = Duration::from_secs(12);

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
    let bytes = client
        .get(request_url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
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

#[tauri::command]
pub async fn music_youtube_metadata(
    request: MusicYouTubeMetadataRequest,
) -> Result<MusicYouTubeMetadataResponse, String> {
    validate_request(&request)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .user_agent("Ganbaru-AI/0.1 YouTube metadata preview")
        .build()
        .map_err(|error| format!("Could not prepare the YouTube metadata request: {error}"))?;
    let truncated = request.video_ids.len() > MAX_METADATA_VIDEOS;
    let video_ids = request
        .video_ids
        .iter()
        .take(MAX_METADATA_VIDEOS)
        .cloned()
        .collect::<Vec<_>>();
    let (playlist, videos) = tokio::join!(
        fetch_playlist(&client, request.playlist_id.as_deref()),
        fetch_videos(&client, &video_ids),
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

    #[test]
    fn oembed_source_url_is_encoded_as_one_query_value() {
        assert_eq!(
            percent_encode_query_value("https://www.youtube.com/watch?v=01L4CFQdrWA"),
            "https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D01L4CFQdrWA"
        );
    }
}
