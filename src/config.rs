use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "meshlet.toml";
const AGENTS_FILE: &str = "AGENTS.md";
const CODEX_DIR: &str = ".codex";
const CODEX_CONFIG_FILE: &str = "config.toml";
const CODEX_CONFIG_BACKUP_FILE: &str = "config.toml.meshlet.bak";
const GITIGNORE_FILE: &str = ".gitignore";
const OKF_INDEX: &str = "# Meshlet OKF\n\n";
const OKF_LOG: &str = "# Meshlet OKF Log\n\n";
const AGENTS_BLOCK_START: &str = "<!-- meshlet:codex:start -->";
const AGENTS_BLOCK_END: &str = "<!-- meshlet:codex:end -->";
const CODEX_MCP_BLOCK_START: &str = "# BEGIN Meshlet Codex MCP";
const CODEX_MCP_BLOCK_END: &str = "# END Meshlet Codex MCP";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAgentInstallOptions {
    pub patch_project_config: bool,
    pub mcp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentFileReport {
    pub path: String,
    pub section_present: bool,
    pub patched: bool,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodexProjectConfigReport {
    pub path: String,
    pub known_safe: bool,
    pub patched: bool,
    pub backup: Option<String>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodexAgentInstallReport {
    pub agent: String,
    pub root: String,
    pub explicit_mcp: bool,
    pub agents_md: AgentFileReport,
    pub codex_project_config: CodexProjectConfigReport,
    pub mcp_command_snippet: String,
    pub recommended_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodexAgentShowReport {
    pub agent: String,
    pub root: String,
    pub agents_md_section_present: bool,
    pub meshlet_toml_present: bool,
    pub meshlet_db_present: bool,
    pub okf_out_dir: String,
    pub okf_out_dir_present: bool,
    pub mcp_command_snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodexAgentUninstallReport {
    pub agent: String,
    pub root: String,
    pub agents_md: AgentFileReport,
    pub manual_cleanup: Vec<String>,
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

impl Default for CodexAgentInstallOptions {
    fn default() -> Self {
        Self {
            patch_project_config: false,
            mcp: false,
        }
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

pub fn install_codex_agent(
    root: impl AsRef<Path>,
    options: &CodexAgentInstallOptions,
) -> Result<CodexAgentInstallReport> {
    let root = root.as_ref();
    let agents_md = patch_codex_agents(root)?;
    let codex_project_config = if options.patch_project_config {
        patch_codex_project_config(root)?
    } else {
        codex_project_config_status(root, Some("run with --patch to update project config"))
    };

    Ok(CodexAgentInstallReport {
        agent: "codex".to_string(),
        root: path_string(root),
        explicit_mcp: options.mcp,
        agents_md,
        codex_project_config,
        mcp_command_snippet: codex_mcp_config_snippet(),
        recommended_commands: codex_recommended_commands(),
    })
}

pub fn show_codex_agent(root: impl AsRef<Path>) -> Result<CodexAgentShowReport> {
    let root = root.as_ref();
    let config = MeshletConfig::load_from(root)?;
    let okf_dir = config.okf.out_dir;

    Ok(CodexAgentShowReport {
        agent: "codex".to_string(),
        root: path_string(root),
        agents_md_section_present: codex_agents_present(&root.join(AGENTS_FILE))?,
        meshlet_toml_present: root.join(CONFIG_FILE).exists(),
        meshlet_db_present: root.join(".meshlet").join("meshlet.db").exists(),
        okf_out_dir_present: root.join(&okf_dir).exists(),
        okf_out_dir: okf_dir,
        mcp_command_snippet: codex_mcp_config_snippet(),
    })
}

pub fn uninstall_codex_agent(root: impl AsRef<Path>) -> Result<CodexAgentUninstallReport> {
    let root = root.as_ref();
    Ok(CodexAgentUninstallReport {
        agent: "codex".to_string(),
        root: path_string(root),
        agents_md: remove_codex_agents(root)?,
        manual_cleanup: vec![
            "Remove `.codex/config.toml` `[mcp_servers.meshlet]` manually if you patched it."
                .to_string(),
            "Keep or remove `meshlet.toml`, `.meshlet/`, `.meshlet-okf/`, and `graphify-out/` per project policy."
                .to_string(),
        ],
    })
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

fn patch_codex_agents(root: &Path) -> Result<AgentFileReport> {
    let path = root.join(AGENTS_FILE);
    let text = read_text_or_empty(&path)?;
    let section = codex_agents_block();

    let (next, patched) = if has_marked_block(&text, AGENTS_BLOCK_START, AGENTS_BLOCK_END) {
        let next = replace_marked_block(&text, AGENTS_BLOCK_START, AGENTS_BLOCK_END, &section)?;
        let patched = next != text;
        (next, patched)
    } else {
        (append_block_text(text, &section), true)
    };

    if patched {
        fs::write(&path, next).with_context(|| format!("patch {}", path.display()))?;
    }

    Ok(AgentFileReport {
        path: AGENTS_FILE.to_string(),
        section_present: true,
        patched,
        removed: false,
    })
}

fn remove_codex_agents(root: &Path) -> Result<AgentFileReport> {
    let path = root.join(AGENTS_FILE);
    let text = read_text_or_empty(&path)?;
    let present = has_marked_block(&text, AGENTS_BLOCK_START, AGENTS_BLOCK_END);
    if !present {
        return Ok(AgentFileReport {
            path: AGENTS_FILE.to_string(),
            section_present: false,
            patched: false,
            removed: false,
        });
    }

    let next = remove_marked_block(&text, AGENTS_BLOCK_START, AGENTS_BLOCK_END)?;
    fs::write(&path, next).with_context(|| format!("patch {}", path.display()))?;
    Ok(AgentFileReport {
        path: AGENTS_FILE.to_string(),
        section_present: false,
        patched: true,
        removed: true,
    })
}

fn codex_agents_present(path: &Path) -> Result<bool> {
    let text = read_text_or_empty(path)?;
    Ok(has_marked_block(
        &text,
        AGENTS_BLOCK_START,
        AGENTS_BLOCK_END,
    ))
}

fn patch_codex_project_config(root: &Path) -> Result<CodexProjectConfigReport> {
    let Some(path) = safe_codex_project_config_path(root)? else {
        return Ok(codex_project_config_status(
            root,
            Some("project `.codex/config.toml` path is not safe"),
        ));
    };

    let text = read_text_or_empty(&path)?;
    if !has_marked_block(&text, CODEX_MCP_BLOCK_START, CODEX_MCP_BLOCK_END)
        && has_unmanaged_meshlet_mcp_table(&text)
    {
        return Ok(CodexProjectConfigReport {
            path: path_string(Path::new(CODEX_DIR).join(CODEX_CONFIG_FILE).as_path()),
            known_safe: true,
            patched: false,
            backup: None,
            skipped_reason: Some(
                "`[mcp_servers.meshlet]` already exists outside the Meshlet-managed block"
                    .to_string(),
            ),
        });
    }

    let block = codex_mcp_config_block();
    let next = if has_marked_block(&text, CODEX_MCP_BLOCK_START, CODEX_MCP_BLOCK_END) {
        replace_marked_block(&text, CODEX_MCP_BLOCK_START, CODEX_MCP_BLOCK_END, &block)?
    } else {
        append_block_text(text.clone(), &block)
    };

    if next == text {
        return Ok(CodexProjectConfigReport {
            path: path_string(Path::new(CODEX_DIR).join(CODEX_CONFIG_FILE).as_path()),
            known_safe: true,
            patched: false,
            backup: None,
            skipped_reason: None,
        });
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let backup = if path.exists() {
        let backup_path = path.with_file_name(CODEX_CONFIG_BACKUP_FILE);
        fs::copy(&path, &backup_path).with_context(|| format!("backup {}", path.display()))?;
        Some(path_string(
            Path::new(CODEX_DIR)
                .join(CODEX_CONFIG_BACKUP_FILE)
                .as_path(),
        ))
    } else {
        None
    };

    fs::write(&path, next).with_context(|| format!("patch {}", path.display()))?;
    Ok(CodexProjectConfigReport {
        path: path_string(Path::new(CODEX_DIR).join(CODEX_CONFIG_FILE).as_path()),
        known_safe: true,
        patched: true,
        backup,
        skipped_reason: None,
    })
}

fn codex_project_config_status(
    root: &Path,
    skipped_reason: Option<&str>,
) -> CodexProjectConfigReport {
    let known_safe = safe_codex_project_config_path(root)
        .ok()
        .flatten()
        .is_some();
    CodexProjectConfigReport {
        path: path_string(Path::new(CODEX_DIR).join(CODEX_CONFIG_FILE).as_path()),
        known_safe,
        patched: false,
        backup: None,
        skipped_reason: skipped_reason.map(str::to_string),
    }
}

fn safe_codex_project_config_path(root: &Path) -> Result<Option<PathBuf>> {
    let codex_dir = root.join(CODEX_DIR);
    if codex_dir.exists() {
        let metadata = fs::symlink_metadata(&codex_dir)
            .with_context(|| format!("inspect {}", codex_dir.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Ok(None);
        }
    }

    let config_path = codex_dir.join(CODEX_CONFIG_FILE);
    if config_path.exists() {
        let metadata = fs::symlink_metadata(&config_path)
            .with_context(|| format!("inspect {}", config_path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(None);
        }
    }

    Ok(Some(config_path))
}

fn append_block_text(mut text: String, block: &str) -> String {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(block);
    text
}

fn has_marked_block(text: &str, start: &str, end: &str) -> bool {
    text.contains(start) && text.contains(end)
}

fn replace_marked_block(text: &str, start: &str, end: &str, replacement: &str) -> Result<String> {
    let before_start = text
        .find(start)
        .with_context(|| format!("missing managed block start marker `{start}`"))?;
    let after_start = before_start + start.len();
    let after_end = text[after_start..]
        .find(end)
        .map(|index| after_start + index + end.len())
        .with_context(|| format!("missing managed block end marker `{end}`"))?;
    let mut next = String::new();
    next.push_str(text[..before_start].trim_end_matches('\n'));
    if !next.is_empty() {
        next.push_str("\n\n");
    }
    next.push_str(replacement);
    next.push_str(text[after_end..].trim_start_matches('\n'));
    Ok(next)
}

fn remove_marked_block(text: &str, start: &str, end: &str) -> Result<String> {
    let before_start = text
        .find(start)
        .with_context(|| format!("missing managed block start marker `{start}`"))?;
    let after_start = before_start + start.len();
    let after_end = text[after_start..]
        .find(end)
        .map(|index| after_start + index + end.len())
        .with_context(|| format!("missing managed block end marker `{end}`"))?;
    let mut next = String::new();
    next.push_str(text[..before_start].trim_end_matches('\n'));
    next.push_str(text[after_end..].trim_start_matches('\n'));
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    Ok(next)
}

fn has_unmanaged_meshlet_mcp_table(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line == "[mcp_servers.meshlet]" || line == "[mcp_servers.\"meshlet\"]"
    })
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

fn codex_agents_block() -> String {
    format!(
        "{AGENTS_BLOCK_START}\n## Meshlet Workflow\n- Inspect setup with `meshlet agent show codex`.\n- Refresh local context with `meshlet refresh --graphify --okf` after meaningful source or docs changes.\n- Serve local MCP with `meshlet serve --mcp stdio` when Codex needs Meshlet context tools.\n- Keep `meshlet.toml`, `.meshlet/`, `.meshlet-okf/`, and `graphify-out/` local unless project policy says otherwise.\n{AGENTS_BLOCK_END}\n",
    )
}

pub fn codex_mcp_config_snippet() -> String {
    "[mcp_servers.meshlet]\ncommand = \"meshlet\"\nargs = [\"serve\", \"--mcp\", \"stdio\"]\ncwd = \"..\"\nstartup_timeout_sec = 10\ntool_timeout_sec = 60\n".to_string()
}

fn codex_mcp_config_block() -> String {
    format!(
        "{CODEX_MCP_BLOCK_START}\n{}{CODEX_MCP_BLOCK_END}\n",
        codex_mcp_config_snippet()
    )
}

fn codex_recommended_commands() -> Vec<String> {
    vec![
        "meshlet init --adopt".to_string(),
        "meshlet refresh --graphify --okf".to_string(),
        "meshlet agent show codex".to_string(),
    ]
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
    fn codex_agent_install_patches_agents_idempotently() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(AGENTS_FILE), "# Agent Instructions\n")?;

        let first = install_codex_agent(dir.path(), &CodexAgentInstallOptions::default())?;
        let second = install_codex_agent(dir.path(), &CodexAgentInstallOptions::default())?;
        let text = fs::read_to_string(dir.path().join(AGENTS_FILE))?;

        assert!(first.agents_md.patched);
        assert!(!second.agents_md.patched);
        assert_eq!(text.matches(AGENTS_BLOCK_START).count(), 1);
        assert_eq!(text.matches(AGENTS_BLOCK_END).count(), 1);
        assert!(text.contains("## Meshlet Workflow"));
        Ok(())
    }

    #[test]
    fn codex_agent_show_reports_project_local_setup_contract() -> Result<()> {
        let dir = tempdir()?;
        let options = AdoptOptions::new("codex", ".meshlet-okf", "graphify-out", true);
        init_and_adopt(dir.path(), &options)?;
        install_codex_agent(dir.path(), &CodexAgentInstallOptions::default())?;

        let report = show_codex_agent(dir.path())?;
        let value = serde_json::to_value(&report)?;

        assert_eq!(value["agent"], "codex");
        assert_eq!(value["agents_md_section_present"], true);
        assert_eq!(value["meshlet_toml_present"], true);
        assert_eq!(value["meshlet_db_present"], true);
        assert_eq!(value["okf_out_dir"], ".meshlet-okf");
        assert_eq!(value["okf_out_dir_present"], true);
        assert!(
            value["mcp_command_snippet"]
                .as_str()
                .expect("mcp snippet")
                .contains("command = \"meshlet\"")
        );
        Ok(())
    }

    #[test]
    fn codex_agent_patch_config_is_idempotent_and_backed_up() -> Result<()> {
        let dir = tempdir()?;
        let codex_dir = dir.path().join(CODEX_DIR);
        fs::create_dir_all(&codex_dir)?;
        fs::write(codex_dir.join(CODEX_CONFIG_FILE), "model = \"gpt-5\"\n")?;
        let options = CodexAgentInstallOptions {
            patch_project_config: true,
            mcp: true,
        };

        let first = install_codex_agent(dir.path(), &options)?;
        let second = install_codex_agent(dir.path(), &options)?;
        let text = fs::read_to_string(codex_dir.join(CODEX_CONFIG_FILE))?;
        let backup = fs::read_to_string(codex_dir.join(CODEX_CONFIG_BACKUP_FILE))?;

        assert!(first.codex_project_config.patched);
        assert_eq!(
            first.codex_project_config.backup.as_deref(),
            Some(".codex/config.toml.meshlet.bak")
        );
        assert!(!second.codex_project_config.patched);
        assert!(second.codex_project_config.backup.is_none());
        assert_eq!(backup, "model = \"gpt-5\"\n");
        let _: toml::Value = toml::from_str(&text)?;
        assert!(text.contains("model = \"gpt-5\""));
        assert_eq!(text.matches(CODEX_MCP_BLOCK_START).count(), 1);
        assert_eq!(text.matches(CODEX_MCP_BLOCK_END).count(), 1);
        assert_eq!(text.matches("[mcp_servers.meshlet]").count(), 1);
        Ok(())
    }

    #[test]
    fn codex_agent_uninstall_removes_only_managed_agents_block() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(AGENTS_FILE),
            "# Agent Instructions\n\n## Meshlet\n- legacy local note\n",
        )?;
        install_codex_agent(dir.path(), &CodexAgentInstallOptions::default())?;

        let report = uninstall_codex_agent(dir.path())?;
        let text = fs::read_to_string(dir.path().join(AGENTS_FILE))?;

        assert!(report.agents_md.removed);
        assert!(!text.contains(AGENTS_BLOCK_START));
        assert!(!text.contains(AGENTS_BLOCK_END));
        assert!(text.contains("## Meshlet\n- legacy local note"));
        assert!(text.contains("# Agent Instructions"));
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
