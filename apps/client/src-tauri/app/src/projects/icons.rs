use base64::{Engine as _, engine::general_purpose};
use ganbaru_notes::assets::image_metadata::{
    ManagedImageDimensionError, ManagedImageKind, ManagedImageMetadata, ManagedImageMetadataError,
    parse_managed_image_metadata, validate_managed_image_dimensions,
};
#[cfg(desktop)]
use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
    header::{CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
#[cfg(desktop)]
use std::{
    error::Error,
    fmt,
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs},
    pin::Pin,
    time::{Duration, Instant},
};
#[cfg(desktop)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::{db::connect_sqlite, vault};

const PROJECT_ICON_MAX_DISPLAY_MEGABYTES: usize = 3;
const PROJECT_ICON_MAX_BYTES: usize = PROJECT_ICON_MAX_DISPLAY_MEGABYTES * 1024 * 1024;
const PROJECT_ICON_MAX_BASE64_CHARS: usize = PROJECT_ICON_MAX_BYTES.div_ceil(3) * 4;
const PROJECT_ICON_DIR: &str = "project-icons";
const PROJECT_ICON_ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
#[cfg(desktop)]
const PROJECT_ICON_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(desktop)]
const PROJECT_ICON_MAX_REDIRECTS: usize = 4;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIconAsset {
    pub relative_path: String,
}

#[cfg(desktop)]
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|e| format!("selected path is not a local file: {e}"))
}

#[cfg(desktop)]
fn project_icon_start_directory<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().picture_dir().ok().filter(|path| path.is_dir())
}

fn active_project_icon_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(vault::active_vault_path(app)?
        .join("assets")
        .join(PROJECT_ICON_DIR))
}

fn active_writable_project_icon_dir<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<vault::WritableVaultPath, String> {
    Ok(vault::active_writable_vault_path(app)?
        .join("assets")
        .join(PROJECT_ICON_DIR))
}

fn asset_path_for_relative<R: Runtime>(
    app: &AppHandle<R>,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let file_name = validate_project_icon_relative_path(relative_path)?;
    Ok(active_project_icon_dir(app)?.join(file_name))
}

fn validate_project_icon_relative_path(relative_path: &str) -> Result<&str, String> {
    let relative_path = relative_path.trim();
    let prefix = format!("{PROJECT_ICON_DIR}/");
    let file_name = relative_path
        .strip_prefix(&prefix)
        .ok_or_else(|| "project icon path must stay under project-icons".to_string())?;
    if file_name.is_empty() {
        return Err("project icon path is missing a file name".to_string());
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("project icon path cannot contain nested or parent paths".to_string());
    }
    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("project icon path cannot contain nested or parent paths".to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !PROJECT_ICON_ALLOWED_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    {
        return Err(project_icon_unsupported_type_error());
    }
    Ok(file_name)
}

fn project_icon_unsupported_type_error() -> String {
    "Use PNG, JPG, or WebP. SVG is blocked for security because it can contain interactive or external content.".to_string()
}

fn ensure_project_icon_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("project icon image is empty".to_string());
    }
    if bytes.len() > PROJECT_ICON_MAX_BYTES {
        return Err(project_icon_size_limit_error());
    }
    Ok(())
}

fn project_icon_size_limit_label() -> String {
    format!("{PROJECT_ICON_MAX_DISPLAY_MEGABYTES} MB")
}

fn project_icon_size_limit_error() -> String {
    format!(
        "project icon image exceeds the {} limit",
        project_icon_size_limit_label()
    )
}

fn project_icon_metadata_error(error: ManagedImageMetadataError) -> String {
    match error {
        ManagedImageMetadataError::UnsupportedFormat => project_icon_unsupported_type_error(),
        ManagedImageMetadataError::MalformedHeader(reason) => {
            format!("project icon image header is malformed: {reason}")
        }
    }
}

fn project_icon_dimension_error(error: ManagedImageDimensionError) -> String {
    format!("project icon image {error}")
}

fn validate_project_icon_image(bytes: &[u8]) -> Result<ManagedImageMetadata, String> {
    let metadata = parse_managed_image_metadata(bytes).map_err(project_icon_metadata_error)?;
    validate_managed_image_dimensions(metadata).map_err(project_icon_dimension_error)?;
    Ok(metadata)
}

fn hex_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn write_binary_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "project icon target has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("create project icon directory: {e}"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "project icon target has no file name".to_string())?
        .to_string_lossy()
        .into_owned();
    let temporary_path = parent.join(format!("{file_name}.tmp"));
    {
        let mut file =
            fs::File::create(&temporary_path).map_err(|e| format!("write project icon: {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("write project icon: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync project icon: {e}"))?;
    }
    fs::rename(&temporary_path, path).map_err(|e| format!("save project icon: {e}"))
}

fn save_project_icon_bytes<R: Runtime>(
    app: &AppHandle<R>,
    bytes: Vec<u8>,
) -> Result<ProjectIconAsset, String> {
    ensure_project_icon_size(&bytes)?;
    let kind = validate_project_icon_image(&bytes)?.kind;
    let file_name = format!("{}.{}", hex_hash(&bytes), kind.extension());
    let relative_path = format!("{PROJECT_ICON_DIR}/{file_name}");
    let path = active_writable_project_icon_dir(app)?.join(file_name);
    if !path.exists() {
        write_binary_file_atomically(&path, &bytes)?;
    }
    Ok(ProjectIconAsset { relative_path })
}

fn read_file_capped(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("inspect project icon image: {e}"))?;
    if !metadata.is_file() {
        return Err("project icon image path must be a file".to_string());
    }
    if metadata.len() > PROJECT_ICON_MAX_BYTES as u64 {
        return Err(project_icon_size_limit_error());
    }
    let bytes = fs::read(path).map_err(|e| format!("read project icon image: {e}"))?;
    ensure_project_icon_size(&bytes)?;
    Ok(bytes)
}

fn decode_project_icon_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let trimmed = data_url.trim();
    let Some((data_url_metadata, payload)) = trimmed.split_once(',') else {
        return Err("project icon data URL is malformed".to_string());
    };
    let Some(declared_mime_type) = data_url_metadata
        .strip_prefix("data:")
        .and_then(|metadata| metadata.strip_suffix(";base64"))
    else {
        return Err("project icon data URL must be a base64 image".to_string());
    };
    if ManagedImageKind::from_mime_type(declared_mime_type).is_none() {
        return Err("project icon data URL must be a PNG, JPEG, or WebP image".to_string());
    }
    if payload.len() > PROJECT_ICON_MAX_BASE64_CHARS {
        return Err(project_icon_size_limit_error());
    }
    let bytes = general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("decode project icon data URL: {e}"))?;
    ensure_project_icon_size(&bytes)?;
    let metadata = validate_project_icon_image(&bytes)?;
    if !metadata.kind.matches_mime_type(declared_mime_type) {
        return Err(
            "project icon data URL MIME type does not match its image contents".to_string(),
        );
    }
    Ok(bytes)
}

fn project_icon_data_url(bytes: &[u8]) -> Result<String, String> {
    ensure_project_icon_size(bytes)?;
    let kind = validate_project_icon_image(bytes)?.kind;
    Ok(format!(
        "data:{};base64,{}",
        kind.mime_type(),
        general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(desktop)]
fn is_public_project_icon_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    if first == 0 || first == 10 || first == 127 || first >= 224 {
        return false;
    }
    if first == 100 && (64..=127).contains(&second) {
        return false;
    }
    if first == 169 && second == 254 {
        return false;
    }
    if first == 172 && (16..=31).contains(&second) {
        return false;
    }
    if first == 192
        && (second == 168
            || (second == 0 && matches!(third, 0 | 2))
            || (second == 88 && third == 99))
    {
        return false;
    }
    if first == 198 && (matches!(second, 18 | 19) || (second == 51 && third == 100)) {
        return false;
    }
    if first == 203 && second == 0 && third == 113 {
        return false;
    }
    true
}

#[cfg(desktop)]
fn is_public_project_icon_ipv6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_project_icon_ipv4(mapped);
    }
    let segments = address.segments();
    if segments[0] & 0xe000 != 0x2000 {
        return false;
    }
    if segments[0] == 0x2001
        && (segments[1] == 0
            || (segments[1] == 2 && segments[2] == 0)
            || segments[1] & 0xfff0 == 0x0010
            || segments[1] & 0xfff0 == 0x0020
            || segments[1] == 0x0db8)
    {
        return false;
    }
    if segments[0] == 0x2002 || segments[0] & 0xfff0 == 0x3ff0 {
        return false;
    }
    true
}

#[cfg(desktop)]
fn is_public_project_icon_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_project_icon_ipv4(address),
        IpAddr::V6(address) => is_public_project_icon_ipv6(address),
    }
}

#[cfg(desktop)]
fn validate_project_icon_network_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|_| "project icon URL is invalid".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("project icon URL must use http or https".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("project icon URL cannot contain credentials".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "project icon URL must include a host".to_string())?;
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return Err("project icon URL must use a public host".to_string());
    }
    let ip_host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if ip_host
        .parse::<IpAddr>()
        .is_ok_and(|address| !is_public_project_icon_ip(address))
    {
        return Err("project icon URL must use a public host".to_string());
    }
    Ok(parsed)
}

#[cfg(desktop)]
fn validate_project_icon_resolved_addresses(
    addresses: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>, ProjectIconDnsError> {
    if addresses.is_empty() {
        return Err(ProjectIconDnsError(
            "project icon host resolved to no addresses",
        ));
    }
    if addresses
        .iter()
        .any(|address| !is_public_project_icon_ip(address.ip()))
    {
        return Err(ProjectIconDnsError(
            "project icon host resolved to a non-public address",
        ));
    }
    Ok(addresses)
}

#[derive(Clone, Copy, Debug)]
#[cfg(desktop)]
struct PublicProjectIconDnsResolver;

#[derive(Clone, Copy, Debug)]
#[cfg(desktop)]
struct ProjectIconDnsError(&'static str);

#[cfg(desktop)]
impl fmt::Display for ProjectIconDnsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[cfg(desktop)]
impl Error for ProjectIconDnsError {}

#[cfg(desktop)]
impl Resolve for PublicProjectIconDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let lookup = tauri::async_runtime::spawn_blocking(move || {
            let addresses = (host.as_str(), 0)
                .to_socket_addrs()
                .map_err(|_| ProjectIconDnsError("project icon host could not be resolved"))?
                .collect::<Vec<_>>();
            validate_project_icon_resolved_addresses(addresses)
        });
        Box::pin(async move {
            match lookup.await {
                Ok(Ok(addresses)) => Ok(Box::new(addresses.into_iter()) as Addrs),
                Ok(Err(error)) => Err(Box::new(error) as Box<dyn Error + Send + Sync>),
                Err(_) => Err(
                    Box::new(ProjectIconDnsError("project icon host resolution failed"))
                        as Box<dyn Error + Send + Sync>,
                ),
            }
        })
    }
}

#[cfg(desktop)]
enum ProjectIconHttpHop {
    Redirect(String),
    Image {
        bytes: Vec<u8>,
        content_type: Option<String>,
    },
}

#[cfg(desktop)]
type ProjectIconHttpFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProjectIconHttpHop, String>> + Send + 'a>>;

#[cfg(desktop)]
trait ProjectIconHttpTransport: Sync {
    fn fetch<'a>(&'a self, url: Url, timeout: Duration) -> ProjectIconHttpFuture<'a>;
}

#[derive(Clone, Copy, Debug)]
#[cfg(desktop)]
struct ReqwestProjectIconHttpTransport;

#[cfg(desktop)]
fn append_project_icon_response_chunk(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<(), String> {
    let next_length = bytes
        .len()
        .checked_add(chunk.len())
        .ok_or_else(project_icon_size_limit_error)?;
    if next_length > PROJECT_ICON_MAX_BYTES {
        return Err(project_icon_size_limit_error());
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

#[cfg(desktop)]
impl ProjectIconHttpTransport for ReqwestProjectIconHttpTransport {
    fn fetch<'a>(&'a self, url: Url, timeout: Duration) -> ProjectIconHttpFuture<'a> {
        Box::pin(async move {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let client = reqwest::Client::builder()
                .timeout(timeout)
                .redirect(Policy::none())
                .no_proxy()
                .dns_resolver(PublicProjectIconDnsResolver)
                .build()
                .map_err(|_| "create project icon HTTP client failed".to_string())?;
            let mut response = client
                .get(url)
                .send()
                .await
                .map_err(|_| "download project icon image failed".to_string())?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .ok_or_else(|| "project icon redirect is missing a location".to_string())?
                    .to_str()
                    .map_err(|_| "project icon redirect location is invalid".to_string())?;
                return Ok(ProjectIconHttpHop::Redirect(location.to_owned()));
            }
            if !response.status().is_success() {
                return Err(format!("project icon URL returned {}", response.status()));
            }
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .map(|value| {
                    value
                        .to_str()
                        .map(str::to_owned)
                        .map_err(|_| "project icon response content type is invalid".to_string())
                })
                .transpose()?;
            if content_type
                .as_deref()
                .is_some_and(|value| ManagedImageKind::from_mime_type(value).is_none())
            {
                return Err(project_icon_unsupported_type_error());
            }
            if response
                .content_length()
                .is_some_and(|length| length > PROJECT_ICON_MAX_BYTES as u64)
            {
                return Err(project_icon_size_limit_error());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "read project icon image failed".to_string())?
            {
                append_project_icon_response_chunk(&mut bytes, &chunk)?;
            }
            ensure_project_icon_size(&bytes)?;
            Ok(ProjectIconHttpHop::Image {
                bytes,
                content_type,
            })
        })
    }
}

#[cfg(desktop)]
fn project_icon_remaining_timeout(started_at: Instant) -> Result<Duration, String> {
    PROJECT_ICON_DOWNLOAD_TIMEOUT
        .checked_sub(started_at.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| "project icon download timed out".to_string())
}

#[cfg(desktop)]
async fn download_project_icon_url_with_transport<T: ProjectIconHttpTransport>(
    transport: &T,
    url: &str,
) -> Result<Vec<u8>, String> {
    let started_at = Instant::now();
    let mut current = validate_project_icon_network_url(url)?;
    let mut visited = HashSet::new();
    let mut redirect_count = 0;
    loop {
        current = validate_project_icon_network_url(current.as_str())?;
        if !visited.insert(current.as_str().to_owned()) {
            return Err("project icon redirect loop detected".to_string());
        }
        let timeout = project_icon_remaining_timeout(started_at)?;
        match transport.fetch(current.clone(), timeout).await? {
            ProjectIconHttpHop::Image {
                bytes,
                content_type,
            } => {
                let metadata = validate_project_icon_image(&bytes)?;
                if content_type
                    .as_deref()
                    .is_some_and(|value| !metadata.kind.matches_mime_type(value))
                {
                    return Err(
                        "project icon response MIME type does not match its image contents"
                            .to_string(),
                    );
                }
                return Ok(bytes);
            }
            ProjectIconHttpHop::Redirect(location) => {
                if redirect_count >= PROJECT_ICON_MAX_REDIRECTS {
                    return Err(format!(
                        "project icon URL exceeded the {PROJECT_ICON_MAX_REDIRECTS} redirect limit"
                    ));
                }
                current = current
                    .join(&location)
                    .map_err(|_| "project icon redirect location is invalid".to_string())?;
                redirect_count += 1;
            }
        }
    }
}

#[cfg(desktop)]
async fn download_project_icon_url(url: &str) -> Result<Vec<u8>, String> {
    download_project_icon_url_with_transport(&ReqwestProjectIconHttpTransport, url).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn projects_icon_pick_image_file<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<ProjectIconAsset>, String> {
    let mut picker = app
        .dialog()
        .file()
        .set_title("Upload project icon")
        .add_filter("Image", PROJECT_ICON_ALLOWED_EXTENSIONS);
    if let Some(directory) = project_icon_start_directory(&app) {
        picker = picker.set_directory(directory);
    }
    let Some(path) = picker.blocking_pick_file().map(dialog_path).transpose()? else {
        return Ok(None);
    };
    let bytes = read_file_capped(&path)?;
    save_project_icon_bytes(&app, bytes).map(Some)
}

#[tauri::command]
pub fn projects_icon_save_image_data_url<R: Runtime>(
    app: AppHandle<R>,
    data_url: String,
) -> Result<ProjectIconAsset, String> {
    let bytes = decode_project_icon_data_url(&data_url)?;
    save_project_icon_bytes(&app, bytes)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn projects_icon_download_image_url<R: Runtime>(
    app: AppHandle<R>,
    url: String,
) -> Result<ProjectIconAsset, String> {
    let bytes = download_project_icon_url(&url).await?;
    save_project_icon_bytes(&app, bytes)
}

#[cfg(desktop)]
#[tauri::command]
pub fn projects_icon_asset_path<R: Runtime>(
    app: AppHandle<R>,
    relative_path: String,
) -> Result<String, String> {
    let path = asset_path_for_relative(&app, &relative_path)?;
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "project icon path contains non-utf8 characters".to_string())
}

#[tauri::command]
pub fn projects_icon_asset_data_url<R: Runtime>(
    app: AppHandle<R>,
    relative_path: String,
) -> Result<String, String> {
    let path = asset_path_for_relative(&app, &relative_path)?;
    let bytes = read_file_capped(&path)?;
    project_icon_data_url(&bytes)
}

#[tauri::command]
pub async fn projects_icon_delete_assets_if_unreferenced<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    relative_paths: Vec<String>,
) -> Result<(), String> {
    let _write_permit = vault::active_writable_vault_path(&app)?;
    let mut candidates = HashSet::new();
    for relative_path in relative_paths {
        validate_project_icon_relative_path(&relative_path)?;
        candidates.insert(relative_path);
    }
    if candidates.is_empty() {
        return Ok(());
    }
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let mut referenced = HashSet::new();
    for row in sqlx::query_scalar::<_, String>(
        "SELECT icon FROM project_groups
         UNION ALL SELECT icon FROM projects
         UNION ALL SELECT icon FROM music_playlists
         UNION ALL SELECT ('asset:' || asset_path) FROM project_custom_emojis",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("load project icon references: {e}"))?
    {
        if let Some(relative_path) = row.strip_prefix("asset:") {
            referenced.insert(relative_path.to_string());
        }
    }
    for relative_path in candidates.difference(&referenced) {
        let path = asset_path_for_relative(&app, relative_path)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("delete project icon asset: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    struct FakeProjectIconHttpTransport {
        responses: Mutex<VecDeque<Result<ProjectIconHttpHop, String>>>,
        requested_urls: Mutex<Vec<String>>,
        timeouts: Mutex<Vec<Duration>>,
    }

    impl FakeProjectIconHttpTransport {
        fn new(responses: Vec<Result<ProjectIconHttpHop, String>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                requested_urls: Mutex::new(Vec::new()),
                timeouts: Mutex::new(Vec::new()),
            }
        }

        fn request_count(&self) -> usize {
            self.requested_urls.lock().unwrap().len()
        }
    }

    impl ProjectIconHttpTransport for FakeProjectIconHttpTransport {
        fn fetch<'a>(&'a self, url: Url, timeout: Duration) -> ProjectIconHttpFuture<'a> {
            self.requested_urls.lock().unwrap().push(String::from(url));
            self.timeouts.lock().unwrap().push(timeout);
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected project icon request".to_string()));
            Box::pin(async move { response })
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 33];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[8..12].copy_from_slice(&13u32.to_be_bytes());
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn project_icon_metadata_accepts_bounded_images() {
        assert_eq!(
            validate_project_icon_image(&png(4032, 3024)).unwrap().kind,
            ManagedImageKind::Png
        );
        assert!(validate_project_icon_image(&png(5000, 4000)).is_err());
    }

    #[test]
    fn project_icon_metadata_rejects_unsupported_icon_images() {
        let expected = project_icon_unsupported_type_error();
        assert_eq!(
            validate_project_icon_image(b"GIF89amore").unwrap_err(),
            expected
        );
        assert_eq!(
            validate_project_icon_image(br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#)
                .unwrap_err(),
            expected,
        );
        assert_eq!(
            validate_project_icon_image(b"not-image").unwrap_err(),
            expected
        );
    }

    #[test]
    fn validate_project_icon_relative_path_rejects_escape_paths() {
        assert_eq!(
            validate_project_icon_relative_path("project-icons/abc.png").unwrap(),
            "abc.png",
        );
        assert!(validate_project_icon_relative_path("../abc.png").is_err());
        assert!(validate_project_icon_relative_path("project-icons/../abc.png").is_err());
        assert!(validate_project_icon_relative_path("project-icons/nested/abc.png").is_err());
        assert!(validate_project_icon_relative_path("project-icons/abc.gif").is_err());
        assert!(validate_project_icon_relative_path("project-icons/abc.svg").is_err());
        assert!(validate_project_icon_relative_path("project-icons/abc.txt").is_err());
    }

    #[test]
    fn decode_project_icon_data_url_requires_base64_image() {
        let bytes = png(100, 50);
        let data_url = format!(
            "data:image/png;base64,{}",
            general_purpose::STANDARD.encode(&bytes)
        );
        assert!(decode_project_icon_data_url(&data_url).is_ok());
        assert!(decode_project_icon_data_url("data:text/plain;base64,SGk=").is_err());
        assert_eq!(
            decode_project_icon_data_url(&format!(
                "data:image/jpeg;base64,{}",
                general_purpose::STANDARD.encode(bytes)
            ))
            .unwrap_err(),
            "project icon data URL MIME type does not match its image contents"
        );
    }

    #[test]
    fn project_icon_data_url_uses_sniffed_image_mime_type() {
        let bytes = png(100, 50);
        assert_eq!(
            project_icon_data_url(&bytes).unwrap(),
            format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(&bytes)
            ),
        );
    }

    #[test]
    fn project_icon_size_limit_error_uses_human_units() {
        assert_eq!(
            project_icon_size_limit_error(),
            "project icon image exceeds the 3 MB limit",
        );
    }

    #[test]
    fn project_icon_network_policy_rejects_non_public_addresses() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.168.0.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "100::1",
            "2001:db8::1",
            "2002:7f00:1::",
            "3fff::1",
            "fc00::1",
            "fe80::1",
            "ff00::1",
        ] {
            let address = address.parse().unwrap();
            assert!(
                !is_public_project_icon_ip(address),
                "{address} must not be reachable by the downloader",
            );
        }
        for address in ["8.8.8.8", "93.184.216.34", "2001:4860:4860::8888"] {
            let address = address.parse().unwrap();
            assert!(
                is_public_project_icon_ip(address),
                "{address} should be accepted as globally routable",
            );
        }
    }

    #[test]
    fn project_icon_url_policy_rejects_local_and_encoded_host_edges() {
        for url in [
            "http://127.0.0.1/icon.png",
            "http://2130706433/icon.png",
            "http://0177.0.0.1/icon.png",
            "http://0x7f000001/icon.png",
            "http://[::1]/icon.png",
            "http://[::ffff:127.0.0.1]/icon.png",
            "http://user@example.com/icon.png",
            "http://user:secret@example.com/icon.png",
            "http://127.0.0.1%2f@example.com/icon.png",
            "file:///tmp/icon.png",
        ] {
            assert!(
                validate_project_icon_network_url(url).is_err(),
                "{url} must be rejected",
            );
        }
    }

    #[test]
    fn project_icon_dns_policy_rejects_mixed_public_and_private_results() {
        let addresses = vec![
            "93.184.216.34:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap(),
        ];
        assert!(validate_project_icon_resolved_addresses(addresses).is_err());
        assert!(
            validate_project_icon_resolved_addresses(vec![
                "93.184.216.34:443".parse().unwrap(),
                "[2001:4860:4860::8888]:443".parse().unwrap(),
            ])
            .is_ok()
        );
    }

    #[test]
    fn project_icon_public_to_private_redirect_is_rejected_before_second_request() {
        let transport = FakeProjectIconHttpTransport::new(vec![Ok(ProjectIconHttpHop::Redirect(
            "http://127.0.0.1/private.png".to_string(),
        ))]);
        let error = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap_err();
        assert_eq!(error, "project icon URL must use a public host");
        assert_eq!(transport.request_count(), 1);
    }

    #[test]
    fn project_icon_redirect_with_credentials_is_rejected_before_following() {
        let transport = FakeProjectIconHttpTransport::new(vec![Ok(ProjectIconHttpHop::Redirect(
            "https://user:secret@example.net/icon.png".to_string(),
        ))]);
        let error = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap_err();
        assert_eq!(error, "project icon URL cannot contain credentials");
        assert_eq!(transport.request_count(), 1);
    }

    #[test]
    fn project_icon_redirect_loop_is_rejected_without_repeating_request() {
        let transport = FakeProjectIconHttpTransport::new(vec![Ok(ProjectIconHttpHop::Redirect(
            "/icon.png".to_string(),
        ))]);
        let error = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap_err();
        assert_eq!(error, "project icon redirect loop detected");
        assert_eq!(transport.request_count(), 1);
    }

    #[test]
    fn project_icon_redirect_count_stays_at_the_existing_limit() {
        let transport = FakeProjectIconHttpTransport::new(
            (1..=5)
                .map(|index| {
                    Ok(ProjectIconHttpHop::Redirect(format!(
                        "/redirect-{index}.png"
                    )))
                })
                .collect(),
        );
        let error = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap_err();
        assert_eq!(error, "project icon URL exceeded the 4 redirect limit",);
        assert_eq!(transport.request_count(), 5);
    }

    #[test]
    fn oversized_chunked_project_icon_body_is_rejected_while_streaming() {
        let mut bytes = Vec::new();
        let first_chunk = vec![0; PROJECT_ICON_MAX_BYTES];
        append_project_icon_response_chunk(&mut bytes, &first_chunk).unwrap();
        let error = append_project_icon_response_chunk(&mut bytes, &[0]).unwrap_err();
        assert_eq!(error, project_icon_size_limit_error());
        assert_eq!(bytes.len(), PROJECT_ICON_MAX_BYTES);
    }

    #[test]
    fn valid_public_project_icon_response_returns_sniffed_image() {
        let expected = png(100, 50);
        let transport = FakeProjectIconHttpTransport::new(vec![Ok(ProjectIconHttpHop::Image {
            bytes: expected.clone(),
            content_type: Some("image/png".to_string()),
        })]);
        let actual = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(transport.request_count(), 1);
        assert!(
            transport
                .timeouts
                .lock()
                .unwrap()
                .iter()
                .all(|timeout| *timeout <= PROJECT_ICON_DOWNLOAD_TIMEOUT)
        );
    }

    #[test]
    fn successful_non_image_response_is_rejected_after_sniffing() {
        let transport = FakeProjectIconHttpTransport::new(vec![Ok(ProjectIconHttpHop::Image {
            bytes: b"remote secret error body".to_vec(),
            content_type: None,
        })]);
        let error = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap_err();
        assert_eq!(error, project_icon_unsupported_type_error());
        assert!(!error.contains("remote secret error body"));
    }

    #[test]
    fn successful_image_response_rejects_mime_signature_mismatch() {
        let transport = FakeProjectIconHttpTransport::new(vec![Ok(ProjectIconHttpHop::Image {
            bytes: png(100, 50),
            content_type: Some("image/jpeg; charset=binary".to_string()),
        })]);
        let error = tauri::async_runtime::block_on(download_project_icon_url_with_transport(
            &transport,
            "https://example.com/icon.png",
        ))
        .unwrap_err();
        assert_eq!(
            error,
            "project icon response MIME type does not match its image contents"
        );
    }
}
