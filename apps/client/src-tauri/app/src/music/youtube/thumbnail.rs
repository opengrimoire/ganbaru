use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, StatusCode, Url, redirect::Policy};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::{Mutex, Semaphore};

use super::metadata::fetch_video_thumbnail_url;

const MAX_CACHE_AGE: Duration = Duration::from_secs(28 * 24 * 60 * 60);
const MAX_TEMPORARY_AGE: Duration = Duration::from_secs(60 * 60);
const MAX_THUMBNAIL_BYTES: usize = 256 * 1024;
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 2_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
const CACHE_DIRECTORY: &str = "music-youtube-thumbnails";

static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static FETCH_LIMIT: Semaphore = Semaphore::const_new(6);
static PRUNE_LOCK: Mutex<()> = Mutex::const_new(());
static PRUNED_ONCE: AtomicBool = AtomicBool::new(false);
static WRITE_COUNT: AtomicUsize = AtomicUsize::new(0);
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn is_valid_video_id(video_id: &str) -> bool {
    video_id.len() == 11
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validated_thumbnail_url(value: &str, video_id: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|error| format!("invalid YouTube thumbnail URL: {error}"))?;
    let expected_prefix = format!("/vi/{video_id}/");
    if url.scheme() != "https"
        || url.host_str() != Some("i.ytimg.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with(&expected_prefix)
        || !url.path().ends_with(".jpg")
    {
        return Err("YouTube supplied an unexpected thumbnail URL.".to_string());
    }
    Ok(url)
}

fn is_valid_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && bytes.len() <= MAX_THUMBNAIL_BYTES
        && bytes.starts_with(&[0xff, 0xd8])
        && bytes.ends_with(&[0xff, 0xd9])
}

fn fresh(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .is_ok_and(|age| age < MAX_CACHE_AGE)
}

fn cache_path(directory: &Path, video_id: &str) -> PathBuf {
    directory.join(format!("{video_id}.jpg"))
}

fn abandoned_temporary_file(path: &Path, modified: SystemTime, now: SystemTime) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let parts = name.split('.').collect::<Vec<_>>();
    parts.len() == 4
        && is_valid_video_id(parts[0])
        && parts[1].bytes().all(|byte| byte.is_ascii_digit())
        && parts[2].bytes().all(|byte| byte.is_ascii_digit())
        && parts[3] == "tmp"
        && now
            .duration_since(modified)
            .is_ok_and(|age| age >= MAX_TEMPORARY_AGE)
}

async fn cached_thumbnail(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("inspect cached YouTube thumbnail: {error}")),
    };
    let is_fresh = metadata.len() <= MAX_THUMBNAIL_BYTES as u64
        && metadata
            .modified()
            .is_ok_and(|modified| fresh(modified, SystemTime::now()));
    if is_fresh {
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read cached YouTube thumbnail: {error}")),
        };
        if is_valid_jpeg(&bytes) {
            return Ok(Some(bytes));
        }
    }
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("remove stale YouTube thumbnail: {error}")),
    }
}

fn thumbnail_client() -> Result<&'static Client, String> {
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .redirect(Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .user_agent("Ganbaru-AI/0.1 YouTube thumbnails")
                .build()
                .map_err(|error| format!("prepare YouTube thumbnail client: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

async fn fetch_thumbnail(client: &Client, video_id: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(thumbnail_url) = fetch_video_thumbnail_url(client, video_id).await else {
        return Ok(None);
    };
    let url = validated_thumbnail_url(&thumbnail_url, video_id)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("request YouTube thumbnail: {error}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    response = response
        .error_for_status()
        .map_err(|error| format!("load YouTube thumbnail: {error}"))?;
    if response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("image/jpeg")
    {
        return Err("YouTube thumbnail is not a JPEG response.".to_string());
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_THUMBNAIL_BYTES as u64)
    {
        return Err("YouTube thumbnail exceeds the image size limit.".to_string());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("read YouTube thumbnail: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_THUMBNAIL_BYTES {
            return Err("YouTube thumbnail exceeds the image size limit.".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !is_valid_jpeg(&bytes) {
        return Err("YouTube thumbnail is not a valid JPEG image.".to_string());
    }
    Ok(Some(bytes))
}

async fn save_thumbnail(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), sequence));
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|error| format!("write YouTube thumbnail cache: {error}"))?;
    if let Err(error) = tokio::fs::rename(&temporary, path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        if cached_thumbnail(path).await?.is_some() {
            return Ok(());
        }
        return Err(format!("finish YouTube thumbnail cache write: {error}"));
    }
    Ok(())
}

async fn prune_cache(directory: &Path) -> Result<(), String> {
    let _guard = PRUNE_LOCK.lock().await;
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(directory)
        .await
        .map_err(|error| format!("inspect YouTube thumbnail cache: {error}"))?;
    let now = SystemTime::now();
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|error| format!("read YouTube thumbnail cache: {error}"))?
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("tmp") {
            let metadata = entry
                .metadata()
                .await
                .map_err(|error| format!("inspect YouTube thumbnail temporary file: {error}"))?;
            if metadata.is_file()
                && metadata
                    .modified()
                    .is_ok_and(|modified| abandoned_temporary_file(&path, modified, now))
            {
                tokio::fs::remove_file(&path)
                    .await
                    .map_err(|error| format!("remove abandoned YouTube thumbnail: {error}"))?;
            }
            continue;
        }
        let Some(video_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("jpg")
            || !is_valid_video_id(video_id)
        {
            continue;
        }
        let metadata = entry
            .metadata()
            .await
            .map_err(|error| format!("inspect YouTube thumbnail cache entry: {error}"))?;
        let modified = metadata.modified().ok();
        if !metadata.is_file()
            || metadata.len() > MAX_THUMBNAIL_BYTES as u64
            || !modified.is_some_and(|value| fresh(value, now))
        {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|error| format!("remove stale YouTube thumbnail: {error}"))?;
            continue;
        }
        if let Some(modified) = modified {
            entries.push((path, modified, metadata.len()));
        }
    }
    entries.sort_by_key(|entry| entry.1);
    let mut total_bytes: u64 = entries.iter().map(|entry| entry.2).sum();
    let mut count = entries.len();
    for (path, _, len) in entries {
        if count <= MAX_CACHE_ENTRIES && total_bytes <= MAX_CACHE_BYTES {
            break;
        }
        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| format!("trim YouTube thumbnail cache: {error}"))?;
        count -= 1;
        total_bytes -= len;
    }
    Ok(())
}

async fn maybe_prune_cache(directory: &Path, after_write: bool) -> Result<(), String> {
    let should_prune = if after_write {
        WRITE_COUNT.fetch_add(1, Ordering::Relaxed) % 32 == 31
    } else {
        !PRUNED_ONCE.swap(true, Ordering::Relaxed)
    };
    if should_prune {
        prune_cache(directory).await?;
    }
    Ok(())
}

/// Loads a YouTube-supplied JPEG from a bounded, device-local cache.
#[tauri::command]
pub async fn music_youtube_thumbnail<R: Runtime>(
    app: AppHandle<R>,
    video_id: String,
) -> Result<Option<String>, String> {
    if !is_valid_video_id(&video_id) {
        return Err("The YouTube video id is invalid.".to_string());
    }
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("find app cache directory: {error}"))?
        .join(CACHE_DIRECTORY);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| format!("create YouTube thumbnail cache: {error}"))?;
    maybe_prune_cache(&directory, false).await?;
    let path = cache_path(&directory, &video_id);
    let bytes = if let Some(bytes) = cached_thumbnail(&path).await? {
        bytes
    } else {
        let _permit = FETCH_LIMIT
            .acquire()
            .await
            .map_err(|error| format!("prepare YouTube thumbnail request: {error}"))?;
        if let Some(bytes) = cached_thumbnail(&path).await? {
            bytes
        } else {
            let Some(bytes) = fetch_thumbnail(thumbnail_client()?, &video_id).await? else {
                return Ok(None);
            };
            save_thumbnail(&path, &bytes).await?;
            maybe_prune_cache(&directory, true).await?;
            bytes
        }
    };
    Ok(Some(format!(
        "data:image/jpeg;base64,{}",
        STANDARD.encode(bytes)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_ids_and_unrelated_image_hosts() {
        assert!(is_valid_video_id("01L4CFQdrWA"));
        assert!(!is_valid_video_id("../../private"));
        assert!(
            validated_thumbnail_url(
                "https://i.ytimg.com/vi/01L4CFQdrWA/hqdefault.jpg",
                "01L4CFQdrWA"
            )
            .is_ok()
        );
        assert!(
            validated_thumbnail_url(
                "https://example.com/vi/01L4CFQdrWA/hqdefault.jpg",
                "01L4CFQdrWA"
            )
            .is_err()
        );
        assert!(
            validated_thumbnail_url(
                "https://i.ytimg.com/vi/OTHER_VIDEO/hqdefault.jpg",
                "01L4CFQdrWA"
            )
            .is_err()
        );
    }

    #[test]
    fn only_accepts_bounded_complete_jpeg_images() {
        assert!(is_valid_jpeg(&[0xff, 0xd8, 0xff, 0xd9]));
        assert!(!is_valid_jpeg(&[0xff, 0xd8, 0x00, 0x00]));
        assert!(!is_valid_jpeg(&vec![0xff; MAX_THUMBNAIL_BYTES + 1]));
    }

    #[test]
    fn expires_images_before_thirty_days() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(40 * 24 * 60 * 60);
        assert!(fresh(now - Duration::from_secs(27 * 24 * 60 * 60), now));
        assert!(!fresh(now - MAX_CACHE_AGE, now));
        assert!(!fresh(now + Duration::from_secs(1), now));
    }

    #[test]
    fn only_prunes_old_temporary_files_from_this_cache() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let old = now - MAX_TEMPORARY_AGE;
        assert!(abandoned_temporary_file(
            Path::new("01L4CFQdrWA.123.4.tmp"),
            old,
            now
        ));
        assert!(!abandoned_temporary_file(
            Path::new("01L4CFQdrWA.123.4.tmp"),
            now,
            now
        ));
        assert!(!abandoned_temporary_file(
            Path::new("unrelated.tmp"),
            old,
            now
        ));
    }

    #[tokio::test]
    async fn removes_expired_files_without_touching_fresh_images() {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "ganbaru-youtube-thumbnail-test-{}-{sequence}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let stale = cache_path(&directory, "01L4CFQdrWA");
        let current = cache_path(&directory, "abcDEF_1234");
        let image = [0xff, 0xd8, 0xff, 0xd9];
        std::fs::write(&stale, image).unwrap();
        std::fs::write(&current, image).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - MAX_CACHE_AGE - Duration::from_secs(60))
            .unwrap();

        prune_cache(&directory).await.unwrap();
        assert!(!stale.exists());
        assert_eq!(
            cached_thumbnail(&current).await.unwrap(),
            Some(image.to_vec())
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
