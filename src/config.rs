use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "meshlet.toml";
const AGENTS_FILE: &str = "AGENTS.md";
const GITIGNORE_FILE: &str = ".gitignore";
const OKF_INDEX: &str = "# Meshlet OKF\n\n";
const OKF_LOG: &str = "# Meshlet OKF Log\n\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshletConfig {
    pub project: ProjectConfig,
    pub storage: StorageConfig,
    pub graphify: GraphifyConfig,
    pub okf: OkfConfig,
    pub agent: AgentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub state_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphifyConfig {
    pub enabled: bool,
    pub out_dir: String,
    pub graph_json: String,
    pub namespace: String,
    pub refresh_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OkfConfig {
    pub enabled: bool,
    pub out_dir: String,
    pub profile: String,
    pub atomic_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub default: String,
    pub refresh_on_task_done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptOptions {
    pub agent: String,
    pub okf_dir: PathBuf,
    pub graphify_out_dir: PathBuf,
    pub patch_agents: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdoptReport {
    pub state: String,
    pub config: String,
    pub okf: String,
    pub created: Vec<String>,
    pub patched: Vec<String>,
    pub already_present: Vec<String>,
    pub next: Vec<String>,
}

impl MeshletConfig {
    pub fn load_from(root: impl AsRef<Path>) -> Result<Self> {
        let path = root.as_ref().join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::load_file(path)
    }

    pub fn load_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse config {}", path.display()))
    }

    pub fn default_toml() -> Result<String> {
        toml::to_string_pretty(&Self::default()).context("serialize default config")
    }
}

impl AdoptOptions {
    pub fn new(
        agent: impl Into<String>,
        okf_dir: impl Into<PathBuf>,
        graphify_out_dir: impl Into<PathBuf>,
        patch_agents: bool,
    ) -> Self {
        Self {
            agent: agent.into(),
            okf_dir: okf_dir.into(),
            graphify_out_dir: graphify_out_dir.into(),
            patch_agents,
        }
    }
}

impl Default for AdoptOptions {
    fn default() -> Self {
        Self::new("codex", ".meshlet-okf", "graphify-out", true)
    }
}

pub fn adopt_project(root: impl AsRef<Path>, options: &AdoptOptions) -> Result<AdoptReport> {
    let root = root.as_ref();
    let mut report = AdoptReport::new(options);

    create_config(root, options, &mut report)?;
    create_okf_skeleton(root, &options.okf_dir, &mut report)?;
    patch_gitignore(root, &options.graphify_out_dir, &mut report)?;
    if options.patch_agents {
        patch_agents(root, options, &mut report)?;
    }

    Ok(report)
}

impl AdoptReport {
    pub fn new(options: &AdoptOptions) -> Self {
        let okf_dir = path_string(&options.okf_dir);
        Self {
            state: ".meshlet/meshlet.db".to_string(),
            config: CONFIG_FILE.to_string(),
            okf: path_string(&options.okf_dir.join("index.md")),
            created: Vec::new(),
            patched: Vec::new(),
            already_present: Vec::new(),
            next: vec![
                "meshlet refresh --graphify --okf".to_string(),
                format!("open {} in Obsidian", okf_dir.trim_end_matches('/')),
            ],
        }
    }

    pub fn record_state_db(&mut self, already_present: bool) {
        if already_present {
            self.already_present.insert(0, self.state.clone());
        } else {
            self.created.insert(0, self.state.clone());
        }
    }
}

fn create_config(root: &Path, options: &AdoptOptions, report: &mut AdoptReport) -> Result<()> {
    let path = root.join(CONFIG_FILE);
    if path.exists() {
        report.already_present.push(CONFIG_FILE.to_string());
        return Ok(());
    }
    let mut config = MeshletConfig::default();
    config.agent.default = options.agent.clone();
    config.okf.out_dir = path_string(&options.okf_dir);
    config.graphify.out_dir = path_string(&options.graphify_out_dir);
    config.graphify.graph_json = path_string(&options.graphify_out_dir.join("graph.json"));
    fs::write(&path, toml::to_string_pretty(&config)?)
        .with_context(|| format!("write config {}", path.display()))?;
    report.created.push(CONFIG_FILE.to_string());
    Ok(())
}

fn create_okf_skeleton(root: &Path, okf_dir: &Path, report: &mut AdoptReport) -> Result<()> {
    let dir = root.join(okf_dir);
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("create OKF dir {}", dir.display()))?;
    }
    create_file_if_missing(&dir.join("index.md"), OKF_INDEX, report, okf_dir)?;
    create_file_if_missing(&dir.join("log.md"), OKF_LOG, report, okf_dir)?;
    Ok(())
}

fn create_file_if_missing(
    path: &Path,
    text: &str,
    report: &mut AdoptReport,
    base: &Path,
) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let report_path = path_string(&base.join(name));
    if path.exists() {
        report.already_present.push(report_path);
        return Ok(());
    }
    fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    report.created.push(report_path);
    Ok(())
}

fn patch_gitignore(root: &Path, graphify_out_dir: &Path, report: &mut AdoptReport) -> Result<()> {
    let path = root.join(GITIGNORE_FILE);
    let graphify_ignore = trailing_slash(&path_string(graphify_out_dir));
    if append_missing_lines(&path, &[".meshlet/", &graphify_ignore])? {
        report.patched.push(GITIGNORE_FILE.to_string());
    }
    Ok(())
}

fn patch_agents(root: &Path, options: &AdoptOptions, report: &mut AdoptReport) -> Result<()> {
    let path = root.join(AGENTS_FILE);
    let text = read_text_or_empty(&path)?;
    if text
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("## Meshlet"))
    {
        return Ok(());
    }
    append_text(&path, &agents_section(options))?;
    report.patched.push(AGENTS_FILE.to_string());
    Ok(())
}

fn append_missing_lines(path: &Path, lines: &[&str]) -> Result<bool> {
    let text = read_text_or_empty(path)?;
    let existing = text.lines().map(str::trim).collect::<Vec<_>>();
    let missing = lines
        .iter()
        .copied()
        .filter(|line| !existing.contains(line))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(false);
    }
    let mut next = text;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    for line in missing {
        next.push_str(line);
        next.push('\n');
    }
    fs::write(path, next).with_context(|| format!("patch {}", path.display()))?;
    Ok(true)
}

fn append_text(path: &Path, section: &str) -> Result<()> {
    let mut text = read_text_or_empty(path)?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(section);
    fs::write(path, text).with_context(|| format!("patch {}", path.display()))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn trailing_slash(value: &str) -> String {
    if value.ends_with('/') {
        value.to_string()
    } else {
        format!("{value}/")
    }
}

fn read_text_or_empty(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn agents_section(options: &AdoptOptions) -> String {
    format!(
        "## Meshlet\n- Config: `meshlet.toml`.\n- Local state: `.meshlet/`; Graphify output: `{}`.\n- OKF bundle: `{}` when enabled.\n",
        trailing_slash(&path_string(&options.graphify_out_dir)),
        trailing_slash(&path_string(&options.okf_dir)),
    )
}

impl Default for MeshletConfig {
    fn default() -> Self {
        Self {
            project: ProjectConfig::default(),
            storage: StorageConfig::default(),
            graphify: GraphifyConfig::default(),
            okf: OkfConfig::default(),
            agent: AgentConfig::default(),
        }
    }
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "meshlet".to_string(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            state_dir: ".meshlet".to_string(),
        }
    }
}

impl Default for GraphifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            out_dir: "graphify-out".to_string(),
            graph_json: "graphify-out/graph.json".to_string(),
            namespace: "graphify:repo".to_string(),
            refresh_command: "graphify update .".to_string(),
        }
    }
}

impl Default for OkfConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            out_dir: ".meshlet-okf".to_string(),
            profile: "local-trusted".to_string(),
            atomic_sync: true,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default: "codex".to_string(),
            refresh_on_task_done: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Meshlet;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn init_and_adopt(root: &Path, options: &AdoptOptions) -> Result<AdoptReport> {
        let state_was_present = root.join(".meshlet/meshlet.db").exists();
        Meshlet::init(root)?;
        let mut report = adopt_project(root, options)?;
        report.record_state_db(state_was_present);
        Ok(report)
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn default_config_generation_matches_plan_values() -> Result<()> {
        let config = MeshletConfig::default();
        let text = MeshletConfig::default_toml()?;
        let parsed: MeshletConfig = toml::from_str(&text)?;

        assert_eq!(parsed, config);
        assert_eq!(config.project.name, "meshlet");
        assert_eq!(config.storage.state_dir, ".meshlet");
        assert_eq!(config.graphify.graph_json, "graphify-out/graph.json");
        assert_eq!(config.okf.out_dir, ".meshlet-okf");
        assert_eq!(config.agent.default, "codex");
        assert!(text.contains("out_dir = \".meshlet-okf\""));
        assert!(text.contains("graph_json = \"graphify-out/graph.json\""));
        assert!(text.contains("default = \"codex\""));
        Ok(())
    }

    #[test]
    fn load_config_uses_meshlet_toml_and_missing_file_falls_back_to_default() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(CONFIG_FILE),
            r#"
[project]
name = "custom-mesh"

[storage]
state_dir = "state"

[graphify]
enabled = false
out_dir = "gf"
graph_json = "gf/custom.json"
namespace = "graphify:custom"
refresh_command = "graphify update src"

[okf]
enabled = false
out_dir = "okf"
profile = "public-safe"
atomic_sync = false

[agent]
default = "claude"
refresh_on_task_done = false
"#,
        )?;
        let loaded = MeshletConfig::load_from(dir.path())?;

        assert_eq!(loaded.project.name, "custom-mesh");
        assert_eq!(loaded.storage.state_dir, "state");
        assert!(!loaded.graphify.enabled);
        assert_eq!(loaded.graphify.graph_json, "gf/custom.json");
        assert_eq!(loaded.okf.profile, "public-safe");
        assert!(!loaded.okf.atomic_sync);
        assert_eq!(loaded.agent.default, "claude");
        assert!(!loaded.agent.refresh_on_task_done);

        let missing_dir = tempdir()?;
        assert_eq!(
            MeshletConfig::load_from(missing_dir.path())?,
            MeshletConfig::default()
        );
        Ok(())
    }

    #[test]
    fn adopt_creates_meshlet_toml() -> Result<()> {
        let dir = tempdir()?;
        let options = AdoptOptions::new("codex", ".meshlet-okf", "graphify-out", false);

        let report = init_and_adopt(dir.path(), &options)?;
        let config = MeshletConfig::load_from(dir.path())?;

        assert!(dir.path().join(CONFIG_FILE).exists());
        assert!(report.created.contains(&CONFIG_FILE.to_string()));
        assert!(report.created.contains(&".meshlet/meshlet.db".to_string()));
        assert_eq!(config.agent.default, "codex");
        assert_eq!(config.okf.out_dir, ".meshlet-okf");
        assert_eq!(config.graphify.out_dir, "graphify-out");
        assert_eq!(config.graphify.graph_json, "graphify-out/graph.json");
        Ok(())
    }

    #[test]
    fn adopt_creates_okf_skeleton() -> Result<()> {
        let dir = tempdir()?;

        let report = init_and_adopt(dir.path(), &AdoptOptions::default())?;

        assert!(dir.path().join(".meshlet-okf/index.md").exists());
        assert!(dir.path().join(".meshlet-okf/log.md").exists());
        assert!(!report.created.contains(&".meshlet-okf/".to_string()));
        assert!(
            report
                .created
                .contains(&".meshlet-okf/index.md".to_string())
        );
        assert!(report.created.contains(&".meshlet-okf/log.md".to_string()));
        Ok(())
    }

    #[test]
    fn adopt_patches_gitignore_idempotently() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(GITIGNORE_FILE), "target/\n")?;

        let first = init_and_adopt(dir.path(), &AdoptOptions::default())?;
        let second = init_and_adopt(dir.path(), &AdoptOptions::default())?;
        let text = fs::read_to_string(dir.path().join(GITIGNORE_FILE))?;

        assert!(first.patched.contains(&GITIGNORE_FILE.to_string()));
        assert!(!second.patched.contains(&GITIGNORE_FILE.to_string()));
        assert_eq!(text.matches(".meshlet/").count(), 1);
        assert_eq!(text.matches("graphify-out/").count(), 1);
        Ok(())
    }

    #[test]
    fn adopt_patches_agents_idempotently() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(AGENTS_FILE), "# Agent Instructions\n")?;

        let first = init_and_adopt(dir.path(), &AdoptOptions::default())?;
        let second = init_and_adopt(dir.path(), &AdoptOptions::default())?;
        let text = fs::read_to_string(dir.path().join(AGENTS_FILE))?;

        assert!(first.patched.contains(&AGENTS_FILE.to_string()));
        assert!(!second.patched.contains(&AGENTS_FILE.to_string()));
        assert_eq!(text.matches("## Meshlet").count(), 1);
        Ok(())
    }

    #[test]
    fn adopt_report_contract_tracks_created_and_already_present() -> Result<()> {
        let dir = tempdir()?;
        let options = AdoptOptions::default();

        let first = init_and_adopt(dir.path(), &options)?;
        assert_eq!(first.state, ".meshlet/meshlet.db");
        assert_eq!(first.config, "meshlet.toml");
        assert_eq!(first.okf, ".meshlet-okf/index.md");
        assert_eq!(
            first.created,
            strings(&[
                ".meshlet/meshlet.db",
                "meshlet.toml",
                ".meshlet-okf/index.md",
                ".meshlet-okf/log.md"
            ])
        );
        assert_eq!(first.already_present, Vec::<String>::new());
        assert_eq!(
            first.next,
            strings(&[
                "meshlet refresh --graphify --okf",
                "open .meshlet-okf in Obsidian"
            ])
        );

        let second = init_and_adopt(dir.path(), &options)?;
        assert_eq!(second.created, Vec::<String>::new());
        assert_eq!(
            second.already_present,
            strings(&[
                ".meshlet/meshlet.db",
                "meshlet.toml",
                ".meshlet-okf/index.md",
                ".meshlet-okf/log.md"
            ])
        );
        assert_eq!(second.patched, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn adopt_report_contract_respects_custom_paths() -> Result<()> {
        let dir = tempdir()?;
        let options = AdoptOptions::new("codex", "okf-vault", "gf-out", false);

        let report = init_and_adopt(dir.path(), &options)?;
        let config = MeshletConfig::load_from(dir.path())?;

        assert_eq!(report.okf, "okf-vault/index.md");
        assert!(report.created.contains(&"okf-vault/index.md".to_string()));
        assert!(report.created.contains(&"okf-vault/log.md".to_string()));
        assert_eq!(
            report.next,
            strings(&[
                "meshlet refresh --graphify --okf",
                "open okf-vault in Obsidian"
            ])
        );
        assert_eq!(config.okf.out_dir, "okf-vault");
        assert_eq!(config.graphify.out_dir, "gf-out");
        assert_eq!(config.graphify.graph_json, "gf-out/graph.json");
        Ok(())
    }

    #[test]
    fn adopt_no_patch_agents_omits_agents_patch() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(AGENTS_FILE), "# Agent Instructions\n")?;
        let options = AdoptOptions::new("codex", ".meshlet-okf", "graphify-out", false);

        let report = init_and_adopt(dir.path(), &options)?;
        let text = fs::read_to_string(dir.path().join(AGENTS_FILE))?;

        assert!(!report.patched.contains(&AGENTS_FILE.to_string()));
        assert!(!text.contains("## Meshlet"));
        Ok(())
    }

    #[test]
    fn plain_init_still_creates_meshlet_db_without_adoption_files() -> Result<()> {
        let dir = tempdir()?;

        Meshlet::init(dir.path())?;

        assert!(dir.path().join(".meshlet/meshlet.db").exists());
        assert!(!dir.path().join(CONFIG_FILE).exists());
        assert!(!dir.path().join(".meshlet-okf").exists());
        Ok(())
    }
}
