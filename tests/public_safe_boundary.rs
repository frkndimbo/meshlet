use std::fs;

use anyhow::Result;
use meshlet::{EventVisibility, Meshlet, OutputMode, SafetyProfile};
use serde::Serialize;
use serde_json::{Value, json};
use tempfile::tempdir;

fn serialize<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialize public-safe surface")
}

fn assert_no_forbidden_sentinels(output: &str, sentinels: &[&str]) {
    for sentinel in sentinels {
        assert!(
            !output.contains(sentinel),
            "public-safe output contains forbidden sentinel `{sentinel}` in:\n{output}"
        );
    }
}

fn assert_public_surface_safe<T: Serialize>(result: anyhow::Result<T>, sentinels: &[&str]) {
    if let Ok(value) = result {
        let output = serialize(&value);
        assert_no_forbidden_sentinels(&output, sentinels);
    }
}

fn append_public_untrusted(meshlet: &Meshlet, payload: Value) -> Result<()> {
    meshlet.append_event_with_options(
        "context.added",
        "test:public-boundary",
        payload,
        EventVisibility::Public,
        SafetyProfile::LocalTrusted,
    )?;
    Ok(())
}

fn unsafe_public_meshlet() -> Result<(tempfile::TempDir, Meshlet)> {
    let dir = tempdir()?;
    let meshlet = Meshlet::init(dir.path())?;
    append_public_untrusted(
        &meshlet,
        json!({
            "label": "PR1_EXPORT_FAILOPEN_LABEL",
            "summary": "Bearer PR1_EXPORT_FAILOPEN_TOKEN"
        }),
    )?;
    Ok((dir, meshlet))
}

fn public_query(meshlet: &Meshlet, q: &str) -> Result<Value> {
    meshlet.query_scoped_view(
        q,
        Some("all"),
        None,
        100,
        OutputMode::Compact,
        SafetyProfile::PublicSafe,
    )
}

#[test]
fn public_json_export_fails_when_doctor_fails() -> Result<()> {
    let (dir, meshlet) = unsafe_public_meshlet()?;
    let doctor = meshlet.public_doctor()?;
    let output_path = dir.path().join("public.json");

    assert_eq!(doctor.get("ok").and_then(Value::as_bool), Some(false));
    let result = meshlet.public_export(20);

    if result.is_err() {
        assert!(
            !output_path.exists(),
            "failed unsafe public JSON export created shareable file {}",
            output_path.display()
        );
    }
    assert!(
        result.is_err(),
        "unsafe public JSON export must fail closed when doctor fails; got Ok: {}",
        serialize(&result.ok())
    );
    Ok(())
}

#[test]
fn public_okf_export_fails_when_doctor_fails() -> Result<()> {
    let (dir, meshlet) = unsafe_public_meshlet()?;
    let doctor = meshlet.public_doctor()?;
    let out_dir = dir.path().join("okf-public");

    assert_eq!(doctor.get("ok").and_then(Value::as_bool), Some(false));
    let result = meshlet.public_export_okf(&out_dir, 20);

    if result.is_err() {
        assert!(
            !out_dir.exists() || fs::read_dir(&out_dir)?.next().transpose()?.is_none(),
            "failed unsafe OKF export created shareable output in {}",
            out_dir.display()
        );
    }
    assert!(
        result.is_err(),
        "unsafe OKF export must fail closed when doctor fails; got Ok: {}",
        serialize(&result.ok())
    );
    Ok(())
}

#[test]
fn public_outputs_do_not_emit_adversarial_strings() -> Result<()> {
    let dir = tempdir()?;
    let meshlet = Meshlet::init(dir.path())?;
    let sentinels = [
        ("api-token", "PR1ADV_API_TOKEN_VALUE_7f3a"),
        ("bearer-token", "Bearer PR1ADV_BEARER_TOKEN_9c2b"),
        ("github-token", "ghp_PR1ADV_GITHUB_TOKEN_1234567890"),
        ("slack-token", "xoxb-PR1ADV-SLACK-TOKEN-123456"),
        ("openai-key", "sk-PR1ADVOPENAITOKEN1234567890"),
        (
            "signed-url",
            "https://example.test/object?X-Amz-Signature=PR1ADV_SIGNED_URL",
        ),
        ("email", "pr1adv-secret-user@example.test"),
        (
            "private-path",
            "/home/pr1adv/private/project/.env.PR1ADV_PATH",
        ),
        ("internal-hostname", "pr1adv-internal.service.local"),
        (
            "stack-trace",
            "thread 'main' panicked at 'PR1ADV_STACK_TRACE_SECRET', src/main.rs:42:9",
        ),
        (
            "base64-high-entropy",
            "UFIxQURWX0JBU0U2NF9ISUdIX0VOVFJPUFlfU0VDUkVUXzEyMzQ1Njc4OTA=",
        ),
        (
            "natural-language-secret",
            "The deployment secret is PR1ADV_NATURAL_LANGUAGE_SECRET_4d5e.",
        ),
    ];
    let forbidden = sentinels
        .iter()
        .map(|(_, sentinel)| *sentinel)
        .collect::<Vec<_>>();

    for (case, sentinel) in sentinels {
        append_public_untrusted(
            &meshlet,
            json!({
                "label": format!("PR1ADV {case}"),
                "summary": sentinel
            }),
        )?;
    }

    assert_public_surface_safe(meshlet.public_export(100), &forbidden);
    assert_public_surface_safe(public_query(&meshlet, "PR1ADV"), &forbidden);
    assert_public_surface_safe(
        meshlet.context_digest_limited(100, SafetyProfile::PublicSafe),
        &forbidden,
    );
    Ok(())
}

#[test]
fn public_digest_omits_absolute_paths() -> Result<()> {
    let dir = tempdir()?;
    let meshlet = Meshlet::init(dir.path())?;
    let path_sentinel = dir
        .path()
        .join("PR1_DIGEST_ABSOLUTE_PATH_SENTINEL.txt")
        .display()
        .to_string();
    let sentinels = [path_sentinel.as_str()];

    meshlet.append_event_with_options(
        "evidence.attached",
        "test:public-boundary",
        json!({
            "path": path_sentinel,
            "sha256": "0".repeat(64)
        }),
        EventVisibility::Public,
        SafetyProfile::LocalTrusted,
    )?;

    let digest = meshlet.context_digest_limited(20, SafetyProfile::PublicSafe)?;
    assert_no_forbidden_sentinels(&serialize(&digest), &sentinels);
    Ok(())
}

#[test]
fn public_safe_mailbox_does_not_emit_body_sentinel() -> Result<()> {
    let dir = tempdir()?;
    let meshlet = Meshlet::init(dir.path())?;
    let body_sentinel = "PR1_MAILBOX_BODY_SENTINEL_8153";
    let sentinels = [body_sentinel];

    meshlet.create_task(
        Some("pr1-mailbox-task"),
        "PR1 mailbox public task",
        None,
        Some("agent:b"),
        None,
        EventVisibility::Public,
        SafetyProfile::PublicSafe,
    )?;
    meshlet.send_agent_message(
        "agent:a",
        "agent:b",
        "PR1 mailbox summary",
        Some("pr1-mailbox-task"),
        Some(body_sentinel),
        None,
        EventVisibility::Public,
        SafetyProfile::LocalTrusted,
    )?;

    let mailbox = meshlet.list_mailbox("agent:b", "inbox", 20, SafetyProfile::PublicSafe)?;
    assert_no_forbidden_sentinels(&serialize(&mailbox), &sentinels);
    Ok(())
}

#[test]
fn public_safe_evidence_does_not_emit_path_or_secret_attrs() -> Result<()> {
    let dir = tempdir()?;
    let meshlet = Meshlet::init(dir.path())?;
    let path_sentinel = dir
        .path()
        .join("PR1_EVIDENCE_ABSOLUTE_PATH_SENTINEL.txt")
        .display()
        .to_string();
    let attr_sentinel = "Bearer PR1_EVIDENCE_SECRET_ATTR_SENTINEL_71ac";
    let sentinels = [path_sentinel.as_str(), attr_sentinel];

    meshlet.append_event_with_options(
        "evidence.attached",
        "test:public-boundary",
        json!({
            "path": path_sentinel,
            "sha256": "1".repeat(64),
            "note": attr_sentinel
        }),
        EventVisibility::Public,
        SafetyProfile::LocalTrusted,
    )?;

    let evidence = meshlet.list_evidence_scoped(20, SafetyProfile::PublicSafe)?;
    assert_no_forbidden_sentinels(&serialize(&evidence), &sentinels);
    Ok(())
}

#[test]
fn mixed_visibility_public_surfaces_only_safe_public_projection() -> Result<()> {
    let dir = tempdir()?;
    let meshlet = Meshlet::init(dir.path())?;
    let private_sentinel = "PR1_MIXED_PRIVATE_SENTINEL_d4fd";
    let local_sentinel = "PR1_MIXED_LOCAL_SENTINEL_f3cd";
    let public_sentinel = "PR1_MIXED_PUBLIC_SENTINEL_54cf";
    let hidden = [private_sentinel, local_sentinel];

    meshlet.append_event_with_options(
        "context.added",
        "test:public-boundary",
        json!({"label": format!("PR1MIXED {private_sentinel}")}),
        EventVisibility::Private,
        SafetyProfile::LocalTrusted,
    )?;
    meshlet.append_event_with_options(
        "context.added",
        "test:public-boundary",
        json!({"label": format!("PR1MIXED {local_sentinel}")}),
        EventVisibility::Local,
        SafetyProfile::LocalTrusted,
    )?;
    meshlet.append_event_with_options(
        "context.added",
        "test:public-boundary",
        json!({"label": format!("PR1MIXED {public_sentinel}")}),
        EventVisibility::Public,
        SafetyProfile::PublicSafe,
    )?;

    let export = meshlet.public_export(20)?;
    let export_text = serialize(&export);
    assert_no_forbidden_sentinels(&export_text, &hidden);
    assert!(
        export_text.contains(public_sentinel),
        "public export missing safe public sentinel `{public_sentinel}` in:\n{export_text}"
    );

    let query = public_query(&meshlet, "PR1MIXED")?;
    let query_text = serialize(&query);
    assert_no_forbidden_sentinels(&query_text, &hidden);
    assert!(
        query_text.contains(public_sentinel),
        "public query missing safe public sentinel `{public_sentinel}` in:\n{query_text}"
    );

    let digest = meshlet.context_digest_limited(20, SafetyProfile::PublicSafe)?;
    let digest_text = serialize(&digest);
    assert_no_forbidden_sentinels(&digest_text, &hidden);
    assert!(
        digest_text.contains(public_sentinel),
        "public digest missing safe public sentinel `{public_sentinel}` in:\n{digest_text}"
    );
    Ok(())
}
