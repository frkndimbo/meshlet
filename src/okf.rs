use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::Value;

use super::Event;

pub(crate) struct OkfDocument {
    pub(crate) id: String,
    pub(crate) item_type: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) resource: String,
    pub(crate) tags: Vec<String>,
    pub(crate) timestamp: String,
    pub(crate) source_event_id: String,
    pub(crate) visibility: String,
    pub(crate) relative_path: String,
    pub(crate) body: String,
}

pub(crate) fn prepare_okf_output_dir(path: &Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            bail!("OKF output path must be a directory");
        }
        if fs::read_dir(path)?.next().transpose()?.is_some() {
            bail!("OKF output directory must be empty");
        }
    } else {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

pub(crate) fn okf_index(documents: &[OkfDocument]) -> String {
    let mut text = String::from(
        "---\nokf_version: \"0.1\"\ntype: \"Meshlet OKF Bundle\"\ntitle: \"Meshlet Public Export\"\n---\n# Meshlet Public Export\n\n",
    );
    for doc in documents {
        text.push_str(&format!(
            "- [{}]({}) - {}\n",
            doc.title, doc.relative_path, doc.item_type
        ));
    }
    text
}

pub(crate) fn okf_log(events: &[Event]) -> String {
    let mut text = String::from("# Meshlet Public Event Log\n\n");
    for event in events {
        text.push_str(&format!(
            "- {} `{}` by `{}` (`{}`)\n",
            event.created_at, event.event_type, event.actor, event.id
        ));
    }
    text
}

pub(crate) fn okf_document_text(doc: &OkfDocument, relations: &str) -> String {
    let mut text = format!(
        "---\ntype: {}\ntitle: {}\ndescription: {}\nresource: {}\ntags:\n{}\ntimestamp: {}\nmeshlet_id: {}\nmeshlet_event_id: {}\nsource_event_id: {}\nvisibility: {}\n---\n# {}\n\n{}",
        yaml_string(&doc.item_type),
        yaml_string(&doc.title),
        yaml_string(&doc.description),
        yaml_string(&doc.resource),
        doc.tags
            .iter()
            .map(|tag| format!("  - {}\n", yaml_string(tag)))
            .collect::<String>(),
        yaml_string(&doc.timestamp),
        yaml_string(&doc.id),
        yaml_string(&doc.source_event_id),
        yaml_string(&doc.source_event_id),
        yaml_string(&doc.visibility),
        doc.title,
        doc.body,
    );
    if !relations.is_empty() {
        text.push_str("\n## Relations\n");
        text.push_str(relations);
    }
    text
}

pub(crate) fn okf_relation_lines(
    id: &str,
    edges: &[Value],
    path_by_id: &BTreeMap<String, String>,
    relative_path: &str,
) -> String {
    let mut lines = Vec::new();
    for edge in edges {
        let kind = string_value(edge, "kind").unwrap_or("references");
        if string_value(edge, "from_id") == Some(id) {
            if let Some(to_id) = string_value(edge, "to_id")
                && let Some(path) = path_by_id.get(to_id)
            {
                lines.push(format!(
                    "- `{kind}` [{}]({})\n",
                    to_id,
                    relative_link(relative_path, path)
                ));
            }
        } else if string_value(edge, "to_id") == Some(id)
            && let Some(from_id) = string_value(edge, "from_id")
            && let Some(path) = path_by_id.get(from_id)
        {
            lines.push(format!(
                "- [{}]({}) `{kind}` this\n",
                from_id,
                relative_link(relative_path, path)
            ));
        }
    }
    lines.concat()
}

fn relative_link(from_file: &str, to_file: &str) -> String {
    let from_depth = from_file.matches('/').count();
    let mut link = String::new();
    for _ in 0..from_depth {
        link.push_str("../");
    }
    link.push_str(to_file);
    link
}

pub(crate) fn string_value<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

pub(crate) fn okf_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "item".to_string()
    } else {
        slug.to_string()
    }
}

pub(crate) fn collect_markdown_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn okf_frontmatter_type(text: &str) -> Option<String> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        let Some(value) = line.trim().strip_prefix("type:") else {
            continue;
        };
        return Some(
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        );
    }
    Some(String::new())
}

pub(crate) fn markdown_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find(')') else {
            break;
        };
        links.push(after_start[..end].trim().to_string());
        rest = &after_start[end + 1..];
    }
    links
}

pub(crate) fn link_is_external_or_anchor(link: &str) -> bool {
    link.starts_with('#') || link.contains("://") || link.starts_with("mailto:")
}
