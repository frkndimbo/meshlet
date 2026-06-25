use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use meshlet::{Meshlet, find_project_root, parse_json_arg, run_mcp_stdio};

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
    Init,
    Status,
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
    Serve {
        #[arg(long)]
        mcp: Option<String>,
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
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    Show {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    Rebuild,
    Nodes {
        #[arg(long)]
        kind: Option<String>,
    },
    Edges {
        #[arg(long)]
        from: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SkillCommand {
    Add { manifest_path: PathBuf },
    List,
    Show { name: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => {
            let root = std::env::current_dir()?;
            let meshlet = Meshlet::init(&root)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "initialized",
                    "root": meshlet.root(),
                }))?
            );
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
        Command::Event { command } => {
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            match command {
                EventCommand::Append {
                    event_type,
                    actor,
                    json,
                } => {
                    let payload = parse_json_arg(&json)?;
                    let event = meshlet.append_event(&event_type, &actor, payload)?;
                    print_json(&event)?;
                }
                EventCommand::List { limit } => print_json(&meshlet.list_events(limit)?)?,
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
                GraphCommand::Nodes { kind } => print_json(&meshlet.graph_nodes(kind.as_deref())?)?,
                GraphCommand::Edges { from } => print_json(&meshlet.graph_edges(from.as_deref())?)?,
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
        Command::Serve { mcp } => {
            if mcp.as_deref() != Some("stdio") {
                anyhow::bail!("only `meshlet serve --mcp stdio` is supported");
            }
            let root = find_project_root()?;
            let meshlet = Meshlet::open(&root)?;
            run_mcp_stdio(meshlet)?;
        }
    }
    Ok(())
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
