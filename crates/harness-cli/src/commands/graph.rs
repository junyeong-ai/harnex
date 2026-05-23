use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::ListResponse;
use harness_core::error::{Error, Result};
use harness_core::graph::{NodeRef, NodexClient};

use super::{load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum GraphCommand {
    /// Probe nodex binary; return version string
    Version,
    /// Inverse links: which nodes reference this node
    Backlinks { node_id: String },
    /// Nodes that have no incoming edges
    Orphans,
    /// Nodes that have not been touched within nodex's stale window
    Stale,
    /// Nodes filtered by kind
    Nodes {
        #[arg(long)]
        kind: String,
    },
    /// Structural delta between two git refs (added/removed nodes,
    /// status transitions, field changes)
    Diff {
        ref_a: String,
        ref_b: String,
    },
}

pub fn run<W: Write>(cmd: GraphCommand, out: &mut W) -> Result<ExitCode> {
    let (_config, _config_path, working_dir) = load_config()?;
    let client = NodexClient::anchored(&working_dir).ok_or_else(|| Error::GraphSpawnFailure {
        message: "nodex binary not found on PATH (install nodex to use `harness graph`)".into(),
    })?;

    match cmd {
        GraphCommand::Version => {
            #[derive(serde::Serialize)]
            struct V {
                version: String,
            }
            write_envelope_success(out, V { version: client.version()? })?;
        }
        GraphCommand::Backlinks { node_id } => {
            let nodes: Vec<NodeRef> = client.backlinks(&node_id)?;
            write_envelope_success(out, ListResponse::new(nodes))?;
        }
        GraphCommand::Orphans => {
            let nodes = client.orphans()?;
            write_envelope_success(out, ListResponse::new(nodes))?;
        }
        GraphCommand::Stale => {
            let nodes = client.stale()?;
            write_envelope_success(out, ListResponse::new(nodes))?;
        }
        GraphCommand::Nodes { kind } => {
            let nodes = client.nodes_of_kind(&kind)?;
            write_envelope_success(out, ListResponse::new(nodes))?;
        }
        GraphCommand::Diff { ref_a, ref_b } => {
            let diff = client.diff(&ref_a, &ref_b)?;
            write_envelope_success(out, diff)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
