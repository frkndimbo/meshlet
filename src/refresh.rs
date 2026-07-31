use std::io::ErrorKind;
use std::process::Command;

use super::*;
use crate::config::{CONFIG_FILE, MeshletConfig};

#[derive(Debug, Clone, Default)]
pub struct RefreshOptions {
    pub graphify: bool,
    pub okf: bool,
    pub task_id: Option<String>,
}

impl Meshlet {
    pub fn refresh(&self, options: RefreshOptions) -> Result<Value> {
        let config = MeshletConfig::load_from(&self.root)?;
        let chain = self.verify_event_chain()?;
        if !chain.ok {
            bail!(
                "event chain verification failed at seq {:?}: {}",
                chain.first_invalid_seq,
                chain.reason.as_deref().unwrap_or("unknown")
            );
        }

        let graphify = if options.graphify {
            Some(self.refresh_graphify(&config)?)
        } else {
            None
        };
        let okf = if options.okf {
            Some(self.refresh_okf(&config)?)
        } else {
            None
        };
        let task = if let Some(task_id) = options.task_id.as_deref() {
            Some(self.append_refresh_summary(task_id, graphify.is_some(), okf.is_some())?)
        } else {
            None
        };

        Ok(json!({
            "status": "refreshed",
            "config": CONFIG_FILE,
            "event_chain": {
                "ok": chain.ok,
                "events": chain.events,
                "checked_until_seq": chain.checked_until_seq,
            },
            "graphify": graphify,
            "okf": okf,
            "task": task,
        }))
    }

    fn refresh_graphify(&self, config: &MeshletConfig) -> Result<Value> {
        let command = config.graphify.refresh_command.trim();
        if command.is_empty() {
            bail!("graphify.refresh_command must not be empty");
        }
        let mut parts = command.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| anyhow!("graphify.refresh_command must not be empty"))?;
        let args = parts.collect::<Vec<_>>();
        let output = Command::new(program)
            .args(&args)
            .current_dir(&self.root)
            .output()
            .map_err(|error| {
                if error.kind() == ErrorKind::NotFound {
                    anyhow!("graphify refresh command executable not found: {program}")
                } else {
                    anyhow!("run graphify refresh command `{command}`: {error}")
                }
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "graphify refresh command failed with {}: {}",
                output.status,
                stderr.trim()
            );
        }

        let graph_json = config_path(&self.root, &config.graphify.graph_json);
        if !graph_json.exists() {
            bail!(
                "graphify graph JSON not found after refresh: {}",
                graph_json.display()
            );
        }
        let import = self.import_graph_file_with_visibility(
            &graph_json,
            "graphify",
            &config.graphify.namespace,
            EventVisibility::Local,
        )?;
        self.rebuild_graph()?;

        Ok(json!({
            "status": "refreshed",
            "command": command,
            "graph_json": graph_json.display().to_string(),
            "namespace": config.graphify.namespace,
            "import": import,
            "rebuilt": true,
        }))
    }

    fn refresh_okf(&self, config: &MeshletConfig) -> Result<Value> {
        let profile = parse_safety_profile_arg(&config.okf.profile)?;
        let out_dir = config_path(&self.root, &config.okf.out_dir);
        let sync = self.okf_sync(&out_dir, profile)?;
        Ok(json!({
            "status": "synced",
            "out_dir": out_dir.display().to_string(),
            "profile": profile.as_str(),
            "sync": sync,
        }))
    }

    fn append_refresh_summary(&self, task_id: &str, graphify: bool, okf: bool) -> Result<Value> {
        if task_id.trim().is_empty() {
            bail!("task_id must not be empty");
        }
        let linked = self.task_exists(task_id)?;
        let mut payload = serde_json::Map::new();
        payload.insert("kind".to_string(), json!("refresh"));
        payload.insert("namespace".to_string(), json!("meshlet:refresh"));
        payload.insert("title".to_string(), json!("Meshlet refresh"));
        payload.insert(
            "summary".to_string(),
            json!(format!(
                "Refresh completed: graphify={}, okf={}",
                graphify, okf
            )),
        );
        payload.insert("graphify".to_string(), json!(graphify));
        payload.insert("okf".to_string(), json!(okf));
        if linked {
            payload.insert("task_id".to_string(), json!(task_id));
        } else {
            payload.insert("requested_task_id".to_string(), json!(task_id));
        }
        let event = self.append_event_with_options(
            "context.added",
            "cli",
            Value::Object(payload),
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;

        Ok(json!({
            "requested": task_id,
            "linked": linked,
            "event_id": event.id,
            "visibility": event.visibility.as_str(),
        }))
    }
}

fn config_path(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CONFIG_FILE;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn write_config(root: &Path, refresh_command: &str) -> Result<()> {
        fs::write(
            root.join(CONFIG_FILE),
            format!(
                r#"
[graphify]
graph_json = "graphify-out/graph.json"
namespace = "graphify:test"
refresh_command = "{refresh_command}"

[okf]
out_dir = ".meshlet-okf"
profile = "local-trusted"
"#
            ),
        )?;
        Ok(())
    }

    #[cfg(unix)]
    fn fake_graphify(root: &Path, creates_graph: bool) -> Result<String> {
        let script = root.join("fake-graphify");
        let body = if creates_graph {
            r#"#!/bin/sh
mkdir -p graphify-out
cat > graphify-out/graph.json <<'JSON'
{"nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","relation":"uses"}]}
JSON
"#
        } else {
            "#!/bin/sh\nexit 0\n"
        };
        fs::write(&script, body)?;
        let mut permissions = fs::metadata(&script)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions)?;
        Ok(script.display().to_string())
    }

    #[cfg(unix)]
    #[test]
    fn refresh_graphify_imports_local_graph_and_rebuilds() -> Result<()> {
        let dir = tempdir()?;
        let command = fake_graphify(dir.path(), true)?;
        write_config(dir.path(), &command)?;
        let meshlet = Meshlet::init(dir.path())?;

        let report = meshlet.refresh(RefreshOptions {
            graphify: true,
            okf: false,
            task_id: None,
        })?;

        let nodes = meshlet.graph_nodes_limited(Some("imported"), 20)?;
        let edges = meshlet.graph_edges_limited(Some("graphify:test:a"), 20)?;

        assert_eq!(report["event_chain"]["ok"], true);
        assert_eq!(report["graphify"]["import"]["visibility"], "local");
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert!(nodes.iter().all(|node| node["visibility"] == "local"));
        assert!(edges.iter().all(|edge| edge["visibility"] == "local"));
        Ok(())
    }

    #[test]
    fn refresh_without_flags_verifies_chain_only() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;

        let report = meshlet.refresh(RefreshOptions::default())?;

        assert_eq!(report["status"], "refreshed");
        assert_eq!(report["event_chain"]["ok"], true);
        assert!(report["graphify"].is_null());
        assert!(report["okf"].is_null());
        assert!(report["task"].is_null());
        assert_eq!(meshlet.event_count()?, 1);
        Ok(())
    }

    #[test]
    fn refresh_okf_uses_configured_output() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(CONFIG_FILE),
            r#"
[okf]
out_dir = "okf-out"
profile = "local-trusted"
"#,
        )?;
        let meshlet = Meshlet::init(dir.path())?;

        let report = meshlet.refresh(RefreshOptions {
            graphify: false,
            okf: true,
            task_id: None,
        })?;

        assert_eq!(report["okf"]["profile"], "local-trusted");
        assert!(dir.path().join("okf-out/index.md").exists());
        assert!(dir.path().join("okf-out/log.md").exists());
        Ok(())
    }

    #[test]
    fn refresh_task_summary_links_existing_task() -> Result<()> {
        let dir = tempdir()?;
        let meshlet = Meshlet::init(dir.path())?;
        meshlet.create_task(
            Some("task-123"),
            "Refresh task",
            None,
            None,
            None,
            EventVisibility::Local,
            SafetyProfile::LocalTrusted,
        )?;

        let report = meshlet.refresh(RefreshOptions {
            graphify: false,
            okf: false,
            task_id: Some("task-123".to_string()),
        })?;
        let timeline = meshlet.task_timeline("task-123", 20, SafetyProfile::LocalTrusted)?;

        assert_eq!(report["task"]["linked"], true);
        assert!(
            timeline["items"]
                .as_array()
                .expect("timeline")
                .iter()
                .any(|item| item["type"] == "context.added"
                    && item["payload"]["summary"]
                        == "Refresh completed: graphify=false, okf=false")
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn refresh_missing_graph_json_fails_before_okf_sync() -> Result<()> {
        let dir = tempdir()?;
        let command = fake_graphify(dir.path(), false)?;
        write_config(dir.path(), &command)?;
        fs::create_dir_all(dir.path().join(".meshlet-okf"))?;
        fs::write(dir.path().join(".meshlet-okf/index.md"), "keep")?;
        let meshlet = Meshlet::init(dir.path())?;

        let error = meshlet
            .refresh(RefreshOptions {
                graphify: true,
                okf: true,
                task_id: None,
            })
            .expect_err("missing graph.json should fail");

        assert!(error.to_string().contains("graphify graph JSON not found"));
        assert_eq!(
            fs::read_to_string(dir.path().join(".meshlet-okf/index.md"))?,
            "keep"
        );
        Ok(())
    }

    #[test]
    fn refresh_missing_graphify_executable_is_helpful() -> Result<()> {
        let dir = tempdir()?;
        write_config(dir.path(), "missing-graphify-binary-for-meshlet-test")?;
        let meshlet = Meshlet::init(dir.path())?;

        let error = meshlet
            .refresh(RefreshOptions {
                graphify: true,
                okf: false,
                task_id: None,
            })
            .expect_err("missing binary should fail");

        assert!(
            error
                .to_string()
                .contains("graphify refresh command executable not found")
        );
        Ok(())
    }
}
