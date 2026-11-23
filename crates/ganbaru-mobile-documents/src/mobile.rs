use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{PluginApi, PluginHandle},
};

const PLUGIN_IDENTIFIER: &str = "org.opengrimoire.ganbaru.mobile.documents";

#[derive(Debug)]
pub struct MobileDocuments<R: Runtime>(PluginHandle<R>);

/// A folder automatic backups are written to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupFolder<'a> {
    /// A folder of this name below Documents, written through MediaStore.
    Documents { subdirectory: &'a str },
    /// A document tree the user picked with [`MobileDocuments::pick_backup_tree`].
    Tree { uri: &'a str },
}

/// A document tree the user picked and the app holds persistent write access to.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedBackupTree {
    /// The tree URI later requests name the folder by.
    pub tree_uri: String,
    /// The folder name the document provider reports.
    pub display_name: String,
}

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

#[derive(Deserialize)]
struct SaveDocumentResponse {
    saved: bool,
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
            .map_err(|error| format!("import Ganbaru folder: {error}"))
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

    /// Stream one bounded Android document into an unused app-private file. An empty
    /// `accepted_extensions` accepts any file name, for callers that check the bytes themselves.
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

    /// Ask Android's system picker where to create one bounded UTF-8 document and write it there.
    /// Returns false when the person cancels the picker.
    pub fn save_utf8_document(
        &self,
        file_name: &str,
        contents: &str,
        max_bytes: u64,
        accepted_extensions: &[&str],
        mime_type: &str,
        document_kind: &str,
    ) -> Result<bool, String> {
        self.0
            .run_mobile_plugin::<SaveDocumentResponse>(
                "saveUtf8Document",
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
            .map(|response| response.saved)
            .map_err(|error| format!("save {document_kind} document: {error}"))
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

/// Access Ganbaru's Android document adapter from managed Tauri state.
pub trait MobileDocumentsExt<R: Runtime> {
    fn mobile_documents(&self) -> &MobileDocuments<R>;
}

impl<R: Runtime, T: Manager<R>> MobileDocumentsExt<R> for T {
    fn mobile_documents(&self) -> &MobileDocuments<R> {
        self.state::<MobileDocuments<R>>().inner()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupFolderFields<'a> {
    tree_uri: Option<&'a str>,
    documents_subdirectory: Option<&'a str>,
}

impl<'a> From<BackupFolder<'a>> for BackupFolderFields<'a> {
    fn from(folder: BackupFolder<'a>) -> Self {
        match folder {
            BackupFolder::Documents { subdirectory } => Self {
                tree_uri: None,
                documents_subdirectory: Some(subdirectory),
            },
            BackupFolder::Tree { uri } => Self {
                tree_uri: Some(uri),
                documents_subdirectory: None,
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PickBackupTreeResponse {
    tree_uri: Option<String>,
    display_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupTreeRequest<'a> {
    tree_uri: &'a str,
}

#[derive(Deserialize)]
struct BackupTreeAvailableResponse {
    available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveFileToBackupFolderRequest<'a> {
    #[serde(flatten)]
    folder: BackupFolderFields<'a>,
    source_path: &'a str,
    file_name: &'a str,
    max_bytes: u64,
    accepted_extensions: Vec<String>,
    mime_type: &'a str,
    document_kind: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupFolderFilesRequest<'a> {
    #[serde(flatten)]
    folder: BackupFolderFields<'a>,
    prefix: &'a str,
    names: &'a [String],
}

#[derive(Deserialize)]
struct BackupFolderNamesResponse {
    names: Vec<String>,
}

#[derive(Deserialize)]
struct BackupFolderDeletedResponse {
    deleted: u32,
}

impl<R: Runtime> MobileDocuments<R> {
    /// Ask Android's folder picker for a folder automatic backups go to, starting at Documents,
    /// and keep write access to it across restarts. Returns `None` when the person cancels.
    pub fn pick_backup_tree(&self) -> Result<Option<PickedBackupTree>, String> {
        let response = self
            .0
            .run_mobile_plugin::<PickBackupTreeResponse>("pickBackupTree", ())
            .map_err(|error| format!("pick backup folder: {error}"))?;
        match (response.tree_uri, response.display_name) {
            (Some(tree_uri), Some(display_name)) => Ok(Some(PickedBackupTree {
                tree_uri,
                display_name,
            })),
            (None, _) => Ok(None),
            (Some(_), None) => Err("pick backup folder: the folder has no name".to_string()),
        }
    }

    /// Whether the app still holds write access to the picked backup folder `tree_uri`.
    pub fn backup_tree_available(&self, tree_uri: &str) -> Result<bool, String> {
        self.0
            .run_mobile_plugin::<BackupTreeAvailableResponse>(
                "backupTreeAvailable",
                BackupTreeRequest { tree_uri },
            )
            .map(|response| response.available)
            .map_err(|error| format!("check backup folder: {error}"))
    }

    /// Stream one bounded app-private file into `folder` as `file_name`, returning the name
    /// the folder gave it.
    #[allow(clippy::too_many_arguments)]
    pub fn save_file_to_backup_folder(
        &self,
        folder: BackupFolder<'_>,
        source_path: &str,
        file_name: &str,
        max_bytes: u64,
        accepted_extensions: &[&str],
        mime_type: &str,
        document_kind: &str,
    ) -> Result<String, String> {
        self.0
            .run_mobile_plugin::<SaveDownloadResponse>(
                "saveFileToBackupFolder",
                SaveFileToBackupFolderRequest {
                    folder: folder.into(),
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
            .map_err(|error| format!("save {document_kind}: {error}"))
    }

    /// The names of the files in `folder` that start with `prefix`. A Documents folder lists only
    /// the files this installation created.
    pub fn list_backup_folder_files(
        &self,
        folder: BackupFolder<'_>,
        prefix: &str,
    ) -> Result<Vec<String>, String> {
        self.0
            .run_mobile_plugin::<BackupFolderNamesResponse>(
                "listBackupFolderFiles",
                BackupFolderFilesRequest {
                    folder: folder.into(),
                    prefix,
                    names: &[],
                },
            )
            .map(|response| response.names)
            .map_err(|error| format!("list backup folder: {error}"))
    }

    /// Deletes the files of `folder` named in `names` that start with `prefix`, returning how
    /// many were deleted.
    pub fn delete_backup_folder_files(
        &self,
        folder: BackupFolder<'_>,
        prefix: &str,
        names: &[String],
    ) -> Result<u32, String> {
        self.0
            .run_mobile_plugin::<BackupFolderDeletedResponse>(
                "deleteBackupFolderFiles",
                BackupFolderFilesRequest {
                    folder: folder.into(),
                    prefix,
                    names,
                },
            )
            .map(|response| response.deleted)
            .map_err(|error| format!("delete old backups: {error}"))
    }
}
