use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use meshlet::{
    DEFAULT_LIMIT, Meshlet, OutputMode,
    config::{AdoptOptions, adopt_project},
    find_project_root, parse_json_arg, parse_output_mode_arg, parse_safety_profile_arg,
    parse_visibility_arg, run_mcp_stdio_with_profile,
};

#[derive(Debug, Parser)]
#[command(
    name = "meshlet",
    version,
    about = "Local context mesh for agent workflows"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        adopt: bool,
        #[arg(long, default_value = "codex")]
        agent: String,
        #[arg(long, default_value = ".meshlet-okf")]
        okf: PathBuf,
        #[arg(long, default_value = "graphify-out")]
        graphify_out: PathBuf,
        #[arg(long)]
        no_patch_agents: bool,
    },
    Status,
    Verify,
    Query {
        q: String,
        #[arg(long, default_value = "all")]
        kind: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "compact")]
        mode: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Event {
        #[command(subcommand)]
        command: EventCommand,
    },
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Mailbox {
        #[command(subcommand)]
        command: MailboxCommand,
    },
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    Doctor {
        #[command(subcommand)]
        command: DoctorCommand,
    },
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
    Okf {
        #[command(subcommand)]
        command: OkfCommand,
    },
    Serve {
        #[arg(long)]
        mcp: Option<String>,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
enum EventCommand {
    Append {
        #[arg(long = "type")]
        event_type: String,
        #[arg(long, default_value = "cli")]
        actor: String,
        #[arg(long)]
        json: String,
        #[arg(long, default_value = "private")]
        visibility: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long, default_value = "full")]
        mode: String,
    },
    Show {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum DoctorCommand {
    Public,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    Public {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "json")]
        format: String,
    },
}

#[derive(Debug, Subcommand)]
enum OkfCommand {
    Doctor { bundle_dir: PathBuf },
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    Rebuild,
    Nodes {
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
    },
    Edges {
        #[arg(long)]
        from: Option<String>,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
    },
    Namespaces,
    Import {
        graph_path: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        namespace: String,
    },
}

#[derive(Debug, Subcommand)]
enum SkillCommand {
    Add { manifest_path: PathBuf },
    List,
    Show { name: String },
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    Create {
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        title: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long, default_value = "private")]
        visibility: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Update {
        id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long, default_value = "private")]
        visibility: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    List {
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Show {
        id: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Timeline {
        id: String,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
enum MailboxCommand {
    Send {
        #[arg(long = "from")]
        from_agent: String,
        #[arg(long = "to")]
        to_agent: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        reply_to: Option<String>,
        #[arg(long, default_value = "private")]
        visibility: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Inbox {
        agent: String,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Outbox {
        agent: String,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
enum EvidenceCommand {
    Attach {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        sha256: Option<String>,
    },
    Verify {
        id: String,
    },
    List {
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: u32,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
    Show {
        id: String,
        #[arg(long, default_value = "local-trusted")]
        profile: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init {
            adopt,
            agent,
            okf,
            graphify_out,
            no_patch_agents,
        } => {
            let root = std::env::current_dir()?;
            let state_was_present = root.join(".meshlet").join("meshlet.db").exists();
            let meshlet = Meshlet::init(&root)?;
            if adopt {
                let options = AdoptOptions::new(agent, okf, graphify_out, !no_patch_agents);
                let mut report = adopt_project(&root, &options)?;
                report.record_state_db(state_was_present);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "adopted",
                        "root": meshlet.root(),
                        "state": report.state,
                        "config": report.config,
                        "okf": report.okf,
                        "created": report.created,
                        "patched": report.patched,
                        "already_present": report.already_present,
                        "next": report.next,
                    }))?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "initialized",
                        "root": meshlet.root(),
                    }))?
                );
            }
        }
        Command::Status => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "root": meshlet.root(),
                    "events": meshlet.event_count()?,
                    "skills": meshlet.list_skills()?.len(),
                }))?
            );
        }
        Command::Verify => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            print_json(&meshlet.verify_event_chain()?)?;
        }
        Command::Query {
            q,
            kind,
            namespace,
            limit,
            mode,
            profile,
        } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            print_json(&meshlet.query_scoped_view(
                &q,
                Some(&kind),
                namespace.as_deref(),
                limit,
                parse_output_mode_arg(&mode)?,
                parse_safety_profile_arg(&profile)?,
            )?)?;
        }
        Command::Event { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                EventCommand::Append {
                    event_type,
                    actor,
                    json,
                    visibility,
                    profile,
                } => {
                    let payload = parse_json_arg(&json)?;
                    let event = meshlet.append_event_with_options(
                        &event_type,
                        &actor,
                        payload,
                        parse_visibility_arg(&visibility)?,
                        parse_safety_profile_arg(&profile)?,
                    )?;
                    print_json(&event)?;
                }
                EventCommand::List { limit, mode } => {
                    let events = meshlet.list_events(limit)?;
                    if parse_output_mode_arg(&mode)? == OutputMode::Compact {
                        print_json(&serde_json::json!({
                            "events": events.into_iter().map(|event| serde_json::json!({
                                "id": event.id,
                                "type": event.event_type,
                                "created_at": event.created_at,
                                "actor": event.actor,
                                "visibility": event.visibility.as_str(),
                                "hash": event.hash,
                            })).collect::<Vec<_>>()
                        }))?
                    } else {
                        print_json(&events)?
                    }
                }
                EventCommand::Show { id } => print_json(&meshlet.show_event(&id)?)?,
            }
        }
        Command::Graph { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                GraphCommand::Rebuild => {
                    meshlet.rebuild_graph()?;
                    print_json(&serde_json::json!({ "status": "rebuilt" }))?;
                }
                GraphCommand::Nodes { kind, limit } => {
                    print_json(&meshlet.graph_nodes_limited(kind.as_deref(), limit)?)?
                }
                GraphCommand::Edges { from, limit } => {
                    print_json(&meshlet.graph_edges_limited(from.as_deref(), limit)?)?
                }
                GraphCommand::Namespaces => print_json(&meshlet.graph_namespaces()?)?,
                GraphCommand::Import {
                    graph_path,
                    source,
                    namespace,
                } => print_json(&meshlet.import_graph_file(graph_path, &source, &namespace)?)?,
            }
        }
        Command::Skill { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                SkillCommand::Add { manifest_path } => {
                    let event = meshlet.add_skill_manifest(manifest_path)?;
                    print_json(&event)?;
                }
                SkillCommand::List => print_json(&meshlet.list_skills()?)?,
                SkillCommand::Show { name } => print_json(&meshlet.show_skill(&name)?)?,
            }
        }
        Command::Task { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                TaskCommand::Create {
                    task_id,
                    title,
                    status,
                    assignee,
                    note,
                    visibility,
                    profile,
                } => print_json(&meshlet.create_task(
                    task_id.as_deref(),
                    &title,
                    status.as_deref(),
                    assignee.as_deref(),
                    note.as_deref(),
                    parse_visibility_arg(&visibility)?,
                    parse_safety_profile_arg(&profile)?,
                )?)?,
                TaskCommand::Update {
                    id,
                    status,
                    assignee,
                    note,
                    visibility,
                    profile,
                } => print_json(&meshlet.update_task(
                    &id,
                    status.as_deref(),
                    assignee.as_deref(),
                    note.as_deref(),
                    parse_visibility_arg(&visibility)?,
                    parse_safety_profile_arg(&profile)?,
                )?)?,
                TaskCommand::List { limit, profile } => print_json(
                    &meshlet.list_tasks_scoped(limit, parse_safety_profile_arg(&profile)?)?,
                )?,
                TaskCommand::Show { id, profile } => print_json(
                    &meshlet.show_task_scoped(&id, parse_safety_profile_arg(&profile)?)?,
                )?,
                TaskCommand::Timeline { id, limit, profile } => print_json(
                    &meshlet.task_timeline(&id, limit, parse_safety_profile_arg(&profile)?)?,
                )?,
            }
        }
        Command::Mailbox { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                MailboxCommand::Send {
                    from_agent,
                    to_agent,
                    summary,
                    task_id,
                    body,
                    reply_to,
                    visibility,
                    profile,
                } => print_json(&meshlet.send_agent_message(
                    &from_agent,
                    &to_agent,
                    &summary,
                    task_id.as_deref(),
                    body.as_deref(),
                    reply_to.as_deref(),
                    parse_visibility_arg(&visibility)?,
                    parse_safety_profile_arg(&profile)?,
                )?)?,
                MailboxCommand::Inbox {
                    agent,
                    limit,
                    profile,
                } => print_json(&meshlet.list_mailbox(
                    &agent,
                    "inbox",
                    limit,
                    parse_safety_profile_arg(&profile)?,
                )?)?,
                MailboxCommand::Outbox {
                    agent,
                    limit,
                    profile,
                } => print_json(&meshlet.list_mailbox(
                    &agent,
                    "outbox",
                    limit,
                    parse_safety_profile_arg(&profile)?,
                )?)?,
            }
        }
        Command::Evidence { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                EvidenceCommand::Attach {
                    path,
                    task_id,
                    sha256,
                } => print_json(&meshlet.attach_evidence_file(
                    path,
                    task_id.as_deref(),
                    sha256.as_deref(),
                )?)?,
                EvidenceCommand::Verify { id } => print_json(&meshlet.verify_evidence(&id)?)?,
                EvidenceCommand::List { limit, profile } => print_json(
                    &meshlet.list_evidence_scoped(limit, parse_safety_profile_arg(&profile)?)?,
                )?,
                EvidenceCommand::Show { id, profile } => print_json(
                    &meshlet.show_evidence_scoped(&id, parse_safety_profile_arg(&profile)?)?,
                )?,
            }
        }
        Command::Doctor { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                DoctorCommand::Public => print_json(&meshlet.public_doctor()?)?,
            }
        }
        Command::Export { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                ExportCommand::Public { out, limit, format } => {
                    let value = match format.as_str() {
                        "json" => {
                            let value = meshlet.public_export(limit)?;
                            std::fs::write(&out, serde_json::to_string_pretty(&value)?)?;
                            serde_json::json!({
                                "status": "exported",
                                "format": "json",
                                "path": out,
                            })
                        }
                        "okf" => {
                            let value = meshlet.public_export_okf(&out, limit)?;
                            serde_json::json!({
                                "status": "exported",
                                "format": "okf",
                                "path": out,
                                "documents": value["documents"],
                            })
                        }
                        _ => anyhow::bail!("export format must be json or okf"),
                    };
                    print_json(&value)?;
                }
            }
        }
        Command::Okf { command } => match command {
            OkfCommand::Doctor { bundle_dir } => print_json(&Meshlet::okf_doctor(bundle_dir)?)?,
        },
        Command::Serve { mcp, profile } => {
            if mcp.as_deref() != Some("stdio") {
                anyhow::bail!("only `meshlet serve --mcp stdio` is supported");
            }
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            run_mcp_stdio_with_profile(meshlet, parse_safety_profile_arg(&profile)?)?;
        }
    }
    Ok(())
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
