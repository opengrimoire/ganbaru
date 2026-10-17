use super::*;

pub fn parse_change_request_value(
    kind: HostedSourceControlKind,
    value: &Value,
) -> ChatResult<HostedChangeRequestRead> {
    let object = value.as_object().ok_or_else(protocol_error)?;
    let number = first_u64(object, &["number", "iid", "pullRequestId"])?;
    let title = first_string(object, &["title"])?;
    let mut url = first_optional_string(object, &["url", "web_url", "webUrl"]);
    if kind == HostedSourceControlKind::AzureDevops {
        if let Some(repository_url) = object
            .get("repository")
            .and_then(Value::as_object)
            .and_then(|repository| first_optional_string(repository, &["webUrl"]))
        {
            url = Some(format!("{repository_url}/pullrequest/{number}"));
        }
    }
    let author = object
        .get("author")
        .or_else(|| object.get("createdBy"))
        .and_then(Value::as_object)
        .and_then(|author| {
            first_optional_string(author, &["login", "username", "name", "displayName"])
        });
    Ok(HostedChangeRequestRead {
        provider_kind: kind,
        number,
        title,
        url: url.ok_or_else(protocol_error)?,
        state: first_optional_string(object, &["state", "status"])
            .unwrap_or_else(|| "unknown".to_string())
            .to_ascii_lowercase(),
        base_branch: normalize_branch(&first_string(
            object,
            &[
                "baseRefName",
                "target_branch",
                "targetBranch",
                "targetRefName",
            ],
        )?),
        head_branch: normalize_branch(&first_string(
            object,
            &[
                "headRefName",
                "source_branch",
                "sourceBranch",
                "sourceRefName",
            ],
        )?),
        author,
        draft: object
            .get("isDraft")
            .or_else(|| object.get("draft"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub fn first_string(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> ChatResult<String> {
    first_optional_string(object, fields).ok_or_else(protocol_error)
}

pub fn first_optional_string(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<String> {
    fields
        .iter()
        .find_map(|field| object.get(*field).and_then(Value::as_str))
        .map(str::to_string)
}

pub fn first_u64(object: &serde_json::Map<String, Value>, fields: &[&str]) -> ChatResult<u64> {
    fields
        .iter()
        .find_map(|field| object.get(*field).and_then(Value::as_u64))
        .ok_or_else(protocol_error)
}

pub fn normalize_branch(value: &str) -> String {
    value
        .strip_prefix("refs/heads/")
        .unwrap_or(value)
        .to_string()
}

pub fn validate_text(value: &str, field: &str, maximum: usize) -> ChatResult<()> {
    if value.trim().is_empty()
        || value.len() > maximum
        || value.chars().any(char::is_control)
        || value.starts_with('-')
    {
        return Err(ChatError::validation(
            field,
            "Source-control value is invalid",
        ));
    }
    Ok(())
}

pub fn protocol_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        "Source-control provider returned an invalid response",
        true,
    )
}
