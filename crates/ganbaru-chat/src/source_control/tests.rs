use super::*;

#[test]
fn detects_supported_https_and_ssh_remotes() {
    assert_eq!(
        detect_remote("git@github.com:owner/project.git"),
        Some((HostedSourceControlKind::Github, "owner/project".to_string()))
    );
    assert_eq!(
        detect_remote("https://gitlab.com/group/nested/project.git"),
        Some((
            HostedSourceControlKind::Gitlab,
            "group/nested/project".to_string()
        ))
    );
    assert_eq!(
        detect_remote("https://bitbucket.org/team/project.git"),
        Some((
            HostedSourceControlKind::Bitbucket,
            "team/project".to_string()
        ))
    );
}

#[test]
fn normalizes_change_requests_from_each_cli_shape() {
    let github = parse_change_request_list(
        HostedSourceControlKind::Github,
        r#"[{"number":17,"title":"Improve chat","url":"https://github.com/o/r/pull/17","state":"OPEN","baseRefName":"dev","headRefName":"feature","author":{"login":"alice"},"isDraft":true}]"#,
    )
    .unwrap();
    assert_eq!(github[0].number, 17);
    assert_eq!(github[0].author.as_deref(), Some("alice"));
    assert!(github[0].draft);

    let gitlab = parse_change_request_list(
        HostedSourceControlKind::Gitlab,
        r#"[{"iid":4,"title":"Improve chat","web_url":"https://gitlab.com/o/r/-/merge_requests/4","state":"opened","target_branch":"dev","source_branch":"feature","author":{"username":"alice"}}]"#,
    )
    .unwrap();
    assert_eq!(gitlab[0].url, "https://gitlab.com/o/r/-/merge_requests/4");

    let azure = parse_change_request_list(
        HostedSourceControlKind::AzureDevops,
        r#"[{"pullRequestId":8,"title":"Improve chat","url":"https://api.invalid/8","status":"active","targetRefName":"refs/heads/dev","sourceRefName":"refs/heads/feature","createdBy":{"displayName":"Alice"},"repository":{"webUrl":"https://dev.azure.com/o/p/_git/r"}}]"#,
    )
    .unwrap();
    assert_eq!(azure[0].base_branch, "dev");
    assert_eq!(
        azure[0].url,
        "https://dev.azure.com/o/p/_git/r/pullrequest/8"
    );
}

#[test]
fn source_control_arguments_keep_values_as_distinct_process_arguments() {
    let request = CreateHostedChangeRequest {
        working_folder_id: ProjectWorkingFolderId::new("workspace").unwrap(),
        execution_environment_id: None,
        provider_kind: HostedSourceControlKind::Github,
        repository_slug: "owner/repository".to_string(),
        title: "Title with shell syntax $(ignored)".to_string(),
        body: "Body with `literal text`".to_string(),
        base_branch: "dev".to_string(),
        head_branch: "feature/chat".to_string(),
        draft: false,
    };
    let arguments = create_arguments(&request).unwrap();
    assert!(arguments.contains(&request.title));
    assert!(arguments.contains(&request.body));
}

#[test]
fn normalizes_bitbucket_change_requests() {
    let change_request = parse_bitbucket_change_request(&serde_json::json!({
        "id": 23,
        "title": "Improve chat",
        "state": "OPEN",
        "destination": { "branch": { "name": "dev" } },
        "source": { "branch": { "name": "feature/chat" } },
        "author": { "display_name": "Alice" },
        "links": { "html": { "href": "https://bitbucket.org/team/project/pull-requests/23" } },
        "draft": true
    }))
    .unwrap();

    assert_eq!(change_request.number, 23);
    assert_eq!(change_request.base_branch, "dev");
    assert_eq!(change_request.head_branch, "feature/chat");
    assert_eq!(change_request.author.as_deref(), Some("Alice"));
    assert!(change_request.draft);
}

#[test]
fn encodes_bitbucket_repository_path_segments() {
    assert_eq!(percent_encode_segment("team name"), "team%20name");
    assert_eq!(percent_encode_segment("project/name"), "project%2Fname");
    assert_eq!(percent_encode_segment("safe._-~"), "safe._-~");
}
