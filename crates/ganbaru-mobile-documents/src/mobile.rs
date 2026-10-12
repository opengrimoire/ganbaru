use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{PluginApi, PluginHandle},
};

const PLUGIN_IDENTIFIER: &str = "org.opengrimoire.ganbaruai.mobile.documents";

#[derive(Debug)]
pub struct MobileDocuments<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Clone for MobileDocuments<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickUtf8DocumentRequest {
    max_bytes: u64,
    accepted_extensions: Vec<String>,
    mime_types: Vec<String>,
    document_kind: String,
}

#[derive(Deserialize)]
struct PickUtf8DocumentResponse {
    contents: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickDocumentToPathRequest<'a> {
    destination_path: &'a str,
    max_bytes: u64,
    accepted_extensions: Vec<String>,
    mime_types: Vec<String>,
    document_kind: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PickDocumentToPathResponse {
    display_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveUtf8DownloadRequest<'a> {
    file_name: &'a str,
    contents: &'a str,
    max_bytes: u64,
    accepted_extensions: Vec<String>,
    mime_type: &'a str,
    document_kind: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDownloadResponse {
    display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveFileDownloadRequest<'a> {
    source_path: &'a str,
    file_name: &'a str,
    max_bytes: u64,
    accepted_extensions: Vec<String>,
    mime_type: &'a str,
    document_kind: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickVaultTreeToPathRequest<'a> {
    destination_path: &'a str,
    max_files: u32,
    max_bytes: u64,
    max_depth: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PickVaultTreeToPathResponse {
    display_name: Option<String>,
}

pub(crate) fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> tauri::Result<MobileDocuments<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MobileDocumentsPlugin")?;
    Ok(MobileDocuments(handle))
}

impl<R: Runtime> MobileDocuments<R> {
    /// Select an Android document tree and stream it into an empty app-private directory.
    pub fn pick_vault_tree_to_path(
        &self,
        destination_path: &str,
        max_files: u32,
        max_bytes: u64,
        max_depth: u32,
    ) -> Result<Option<String>, String> {
        self.0
            .run_mobile_plugin::<PickVaultTreeToPathResponse>(
                "pickVaultTreeToPath",
                PickVaultTreeToPathRequest {
                    destination_path,
                    max_files,
                    max_bytes,
                    max_depth,
                },
            )
            .map(|response| response.display_name)
            .map_err(|error| format!("import Ganbaru AI folder: {error}"))
    }

    /// Ask Android to select and read one UTF-8 document within `max_bytes`.
    pub fn pick_utf8_document(&self, max_bytes: u64) -> Result<Option<String>, String> {
        self.pick_utf8_document_matching(max_bytes, &["json"], &["application/json"], "theme")
    }

    /// Ask Android to select and read one bounded UTF-8 document of an allowed type.
    pub fn pick_utf8_document_matching(
        &self,
        max_bytes: u64,
        accepted_extensions: &[&str],
        mime_types: &[&str],
        document_kind: &str,
    ) -> Result<Option<String>, String> {
        self.0
            .run_mobile_plugin::<PickUtf8DocumentResponse>(
                "pickUtf8Document",
                PickUtf8DocumentRequest {
                    max_bytes,
                    accepted_extensions: accepted_extensions
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    mime_types: mime_types
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    document_kind: document_kind.to_string(),
                },
            )
            .map(|response| response.contents)
            .map_err(|error| format!("pick {document_kind} document: {error}"))
    }

    /// Stream one bounded Android document into an unused app-private file.
    pub fn pick_document_to_path(
        &self,
        destination_path: &str,
        max_bytes: u64,
        accepted_extensions: &[&str],
        mime_types: &[&str],
        document_kind: &str,
    ) -> Result<Option<String>, String> {
        self.0
            .run_mobile_plugin::<PickDocumentToPathResponse>(
                "pickDocumentToPath",
                PickDocumentToPathRequest {
                    destination_path,
                    max_bytes,
                    accepted_extensions: accepted_extensions
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    mime_types: mime_types
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    document_kind,
                },
            )
            .map(|response| response.display_name)
            .map_err(|error| format!("pick {document_kind} document: {error}"))
    }

    /// Save bounded UTF-8 text into Android's public Downloads collection.
    pub fn save_utf8_download(
        &self,
        file_name: &str,
        contents: &str,
        max_bytes: u64,
    ) -> Result<String, String> {
        self.save_utf8_download_with_type(
            file_name,
            contents,
            max_bytes,
            &["json"],
            "application/json",
            "theme",
        )
    }

    /// Save bounded UTF-8 text with an explicit safe document type into Downloads.
    pub fn save_utf8_download_with_type(
        &self,
        file_name: &str,
        contents: &str,
        max_bytes: u64,
        accepted_extensions: &[&str],
        mime_type: &str,
        document_kind: &str,
    ) -> Result<String, String> {
        self.0
            .run_mobile_plugin::<SaveDownloadResponse>(
                "saveUtf8Download",
                SaveUtf8DownloadRequest {
                    file_name,
                    contents,
                    max_bytes,
                    accepted_extensions: accepted_extensions
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    mime_type,
                    document_kind,
                },
            )
            .map(|response| response.display_name)
            .map_err(|error| format!("save {document_kind} download: {error}"))
    }

    /// Stream one bounded app-private file into Android's public Downloads collection.
    pub fn save_file_download(
        &self,
        source_path: &str,
        file_name: &str,
        max_bytes: u64,
        accepted_extensions: &[&str],
        mime_type: &str,
        document_kind: &str,
    ) -> Result<String, String> {
        self.0
            .run_mobile_plugin::<SaveDownloadResponse>(
                "saveFileDownload",
                SaveFileDownloadRequest {
                    source_path,
                    file_name,
                    max_bytes,
                    accepted_extensions: accepted_extensions
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    mime_type,
                    document_kind,
                },
            )
            .map(|response| response.display_name)
            .map_err(|error| format!("save {document_kind} download: {error}"))
    }
}

/// Access Ganbaru AI's Android document adapter from managed Tauri state.
pub trait MobileDocumentsExt<R: Runtime> {
    fn mobile_documents(&self) -> &MobileDocuments<R>;
}

impl<R: Runtime, T: Manager<R>> MobileDocumentsExt<R> for T {
    fn mobile_documents(&self) -> &MobileDocuments<R> {
        self.state::<MobileDocuments<R>>().inner()
    }
}
