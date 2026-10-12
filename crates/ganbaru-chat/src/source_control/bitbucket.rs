use super::parsing::*;
use super::*;

pub async fn probe_bitbucket(
    repository: Option<&(HostedSourceControlKind, String, String)>,
    configuration_hint: &str,
    credential_store: &dyn CredentialStore,
) -> HostedSourceControlRead {
    let Some((_, remote_name, repository_slug)) = repository else {
        return HostedSourceControlRead {
            kind: HostedSourceControlKind::Bitbucket,
            label: "Bitbucket".to_string(),
            detected_for_repository: false,
            remote_name: None,
            repository_slug: None,
            status: "configuration_required".to_string(),
            version: None,
            unavailable_reason: Some("No Bitbucket remote was detected".to_string()),
            configuration_hint: Some(configuration_hint.to_string()),
        };
    };
    let result = read_bitbucket_credential(repository_slug, credential_store)
        .and_then(|credential| credential.ok_or_else(missing_credential_error));
    let (status, unavailable_reason) = match result {
        Ok(credential) => {
            match bitbucket_authenticated_request(reqwest::Method::GET, "/user", &credential, None)
                .await
            {
                Ok(_) => ("available", None),
                Err(error) => ("authentication_required", Some(error.message)),
            }
        }
        Err(error) => ("configuration_required", Some(error.message)),
    };
    HostedSourceControlRead {
        kind: HostedSourceControlKind::Bitbucket,
        label: "Bitbucket".to_string(),
        detected_for_repository: true,
        remote_name: Some(remote_name.clone()),
        repository_slug: Some(repository_slug.clone()),
        status: status.to_string(),
        version: None,
        unavailable_reason,
        configuration_hint: (status != "available").then(|| configuration_hint.to_string()),
    }
}

pub async fn bitbucket_list_change_requests(
    repository_slug: &str,
    limit: u32,
    credential_store: &dyn CredentialStore,
) -> ChatResult<Vec<HostedChangeRequestRead>> {
    let credential = require_bitbucket_credential(repository_slug, credential_store)?;
    let (workspace, repository) = bitbucket_repository_parts(repository_slug)?;
    let path = format!(
        "/repositories/{}/{}/pullrequests?pagelen={}&sort=-updated_on&state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED",
        percent_encode_segment(workspace),
        percent_encode_segment(repository),
        limit.clamp(1, 50),
    );
    let value =
        bitbucket_authenticated_request(reqwest::Method::GET, &path, &credential, None).await?;
    let entries = value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?;
    entries.iter().map(parse_bitbucket_change_request).collect()
}

pub async fn bitbucket_create_change_request(
    request: &CreateHostedChangeRequest,
    credential_store: &dyn CredentialStore,
) -> ChatResult<HostedChangeRequestRead> {
    let credential = require_bitbucket_credential(&request.repository_slug, credential_store)?;
    let (workspace, repository) = bitbucket_repository_parts(&request.repository_slug)?;
    let path = format!(
        "/repositories/{}/{}/pullrequests",
        percent_encode_segment(workspace),
        percent_encode_segment(repository),
    );
    let body = serde_json::json!({
        "title": request.title,
        "description": request.body,
        "draft": request.draft,
        "source": { "branch": { "name": request.head_branch } },
        "destination": { "branch": { "name": request.base_branch } },
    });
    let value =
        bitbucket_authenticated_request(reqwest::Method::POST, &path, &credential, Some(body))
            .await?;
    parse_bitbucket_change_request(&value)
}

pub fn parse_bitbucket_change_request(value: &Value) -> ChatResult<HostedChangeRequestRead> {
    let object = value.as_object().ok_or_else(protocol_error)?;
    let branch = |name: &str| {
        object
            .get(name)
            .and_then(Value::as_object)
            .and_then(|value| value.get("branch"))
            .and_then(Value::as_object)
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(protocol_error)
    };
    let url = object
        .get("links")
        .and_then(Value::as_object)
        .and_then(|links| links.get("html"))
        .and_then(Value::as_object)
        .and_then(|html| html.get("href"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(protocol_error)?;
    Ok(HostedChangeRequestRead {
        provider_kind: HostedSourceControlKind::Bitbucket,
        number: first_u64(object, &["id"])?,
        title: first_string(object, &["title"])?,
        url,
        state: first_optional_string(object, &["state"])
            .unwrap_or_else(|| "unknown".to_string())
            .to_ascii_lowercase(),
        base_branch: branch("destination")?,
        head_branch: branch("source")?,
        author: object
            .get("author")
            .and_then(Value::as_object)
            .and_then(|author| first_optional_string(author, &["display_name", "nickname"])),
        draft: object
            .get("draft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub async fn bitbucket_authenticated_request(
    method: reqwest::Method,
    path: &str,
    credential: &BitbucketCredential,
    body: Option<Value>,
) -> ChatResult<Value> {
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(protocol_error());
    }
    let client = reqwest::Client::builder()
        .timeout(OPERATION_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| transport_error())?;
    let mut request = client
        .request(method, format!("{BITBUCKET_API_ROOT}{path}"))
        .basic_auth(&credential.username, Some(&credential.token))
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|_| transport_error())?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|_| transport_error())?;
    if bytes.len() > MAX_OPERATION_OUTPUT_BYTES {
        return Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Bitbucket response exceeded the supported limit",
            true,
        ));
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ChatError::new(
            ChatErrorCode::AuthenticationRequired,
            "Bitbucket credentials were rejected",
            true,
        ));
    }
    if !status.is_success() {
        return Err(ChatError::new(
            ChatErrorCode::DriverUnavailable,
            format!(
                "Bitbucket request failed with HTTP status {}",
                status.as_u16()
            ),
            true,
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| protocol_error())
}

fn require_bitbucket_credential(
    repository_slug: &str,
    credential_store: &dyn CredentialStore,
) -> ChatResult<BitbucketCredential> {
    read_bitbucket_credential(repository_slug, credential_store)?
        .ok_or_else(missing_credential_error)
}

fn read_bitbucket_credential(
    repository_slug: &str,
    credential_store: &dyn CredentialStore,
) -> ChatResult<Option<BitbucketCredential>> {
    let Some(secret) = credential_store
        .read(&bitbucket_credential_reference(repository_slug)?)
        .map_err(|_| credential_error())?
    else {
        return Ok(None);
    };
    serde_json::from_str(secret.expose())
        .map(Some)
        .map_err(|_| credential_error())
}

pub fn bitbucket_credential_reference(repository_slug: &str) -> ChatResult<CredentialReferenceId> {
    let digest = format!("{:x}", Sha256::digest(repository_slug.as_bytes()));
    CredentialReferenceId::new(format!("source-control:bitbucket:{}", &digest[..32]))
        .map_err(|_| credential_error())
}

fn bitbucket_repository_parts(repository_slug: &str) -> ChatResult<(&str, &str)> {
    let (workspace, repository) = repository_slug.split_once('/').ok_or_else(|| {
        ChatError::validation("repositorySlug", "Bitbucket repository is invalid")
    })?;
    if workspace.is_empty() || repository.is_empty() || repository.contains('/') {
        return Err(ChatError::validation(
            "repositorySlug",
            "Bitbucket repository is invalid",
        ));
    }
    Ok((workspace, repository))
}

pub fn percent_encode_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn missing_credential_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::AuthenticationRequired,
        "Bitbucket credentials are not configured",
        true,
    )
}

pub fn credential_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Bitbucket credential operation failed",
        true,
    )
}

fn transport_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::TransportUnavailable,
        "Bitbucket is unavailable",
        true,
    )
}
