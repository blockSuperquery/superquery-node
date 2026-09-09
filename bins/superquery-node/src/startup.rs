//! Startup: logging, connections, and the checks that must pass before any block
//! is indexed.
//!
//! Guide Milestone 1. The ordering is chosen so the cheapest and most likely
//! failures surface first — a typo in a flag should not cost a database round
//! trip to discover.

use anyhow::{Context, Result};
use superquery_config::{DbConfig, NodeConfig};
use superquery_store::{Checkpoint, CheckpointStore, Database};
use tracing_subscriber::EnvFilter;

/// Everything the node needs, assembled and verified.
pub struct NodeContext {
    /// Resolved database settings.
    pub db_config: DbConfig,
    /// A live connection pool.
    pub database: Database,
    /// The project directory.
    pub project_path: String,
    /// RPC endpoints, in preference order.
    pub rpc_endpoints: Vec<String>,
}

impl NodeContext {
    /// Log what this run is about to do.
    ///
    /// Printed once at startup so a support question can be answered from the
    /// first few lines of a log rather than by asking what flags were used.
    pub fn log_summary(&self) {
        tracing::info!(
            project = %self.project_path,
            database = %self.db_config.redacted(),
            schema = %self.db_config.schema,
            endpoints = self.rpc_endpoints.len(),
            "superquery-node starting"
        );
        for (index, endpoint) in self.rpc_endpoints.iter().enumerate() {
            tracing::info!(index, endpoint = %redact_endpoint(endpoint), "rpc endpoint");
        }
    }

    /// The checkpoint this run would resume from, creating the bookkeeping tables
    /// if they are absent.
    ///
    /// Returns `None` on a fresh schema. Idempotent, so it is safe on every start.
    pub async fn resume_position(&self) -> Result<Option<Checkpoint>> {
        self.database
            .ensure_schema()
            .await
            .map_err(anyhow::Error::new)
            .with_context(|| format!("could not create schema '{}'", self.db_config.schema))?;

        let checkpoints = CheckpointStore::new(&self.db_config.schema)
            .map_err(anyhow::Error::new)
            .context("invalid schema name")?;

        checkpoints
            .ensure_tables(&self.database)
            .await
            .map_err(anyhow::Error::new)
            .context("could not create the node's bookkeeping tables")?;

        checkpoints
            .load(&self.database)
            .await
            .map_err(anyhow::Error::new)
            .context("could not read the checkpoint")
    }
}

/// Install the tracing subscriber.
///
/// `RUST_LOG` wins when set, so an operator can raise the level for one subsystem
/// without restarting with different flags.
pub fn init_tracing(config: &NodeConfig) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .with_context(|| format!("invalid --log-level '{}'", config.log_level))?;

    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if config.log_json {
        builder.json().init();
    } else {
        builder.init();
    }
    Ok(())
}

/// Connect and verify everything, in cheapest-failure-first order.
pub async fn bootstrap(config: &NodeConfig) -> Result<NodeContext> {
    // 1. Project path — a local check, so it costs nothing to do first.
    let project_path = resolve_project(&config.project)?;

    // 2. Database configuration, then an actual round trip. Configuration errors
    //    are far more common than an unreachable server, so parse before dialling.
    let db_config = DbConfig::resolve(config.database_url.as_deref())
        .map_err(anyhow::Error::new)
        .context("could not resolve database configuration")?;

    let database = Database::connect(&db_config)
        .map_err(anyhow::Error::new)
        .context("could not build the database connection pool")?;

    database
        .ping()
        .await
        .map_err(anyhow::Error::new)
        .with_context(|| format!("could not reach Postgres at {}", db_config.redacted()))?;
    tracing::debug!("postgres reachable");

    // 3. RPC endpoints.
    let rpc_endpoints = resolve_endpoints(&config.rpc_urls)?;

    Ok(NodeContext {
        db_config,
        database,
        project_path,
        rpc_endpoints,
    })
}

/// Check the project path exists and is a directory.
fn resolve_project(path: &str) -> Result<String> {
    let resolved = std::path::Path::new(path);
    if !resolved.exists() {
        anyhow::bail!("project path '{path}' does not exist");
    }
    if !resolved.is_dir() {
        anyhow::bail!("project path '{path}' is not a directory (expected a built project)");
    }
    Ok(path.to_string())
}

/// Validate the RPC endpoint list.
fn resolve_endpoints(endpoints: &[String]) -> Result<Vec<String>> {
    if endpoints.is_empty() {
        anyhow::bail!("no RPC endpoint configured: pass --rpc-url or set SUPERQUERY_RPC_URL");
    }
    for endpoint in endpoints {
        if !endpoint.starts_with("http://")
            && !endpoint.starts_with("https://")
            && !endpoint.starts_with("ws://")
            && !endpoint.starts_with("wss://")
        {
            anyhow::bail!(
                "RPC endpoint '{endpoint}' has no supported scheme \
                 (expected http, https, ws or wss)"
            );
        }
    }
    Ok(endpoints.to_vec())
}

/// Strip credentials and API keys from an endpoint before logging it.
///
/// Provider URLs routinely carry the API key in the path, and logs get pasted
/// into issue trackers.
fn redact_endpoint(endpoint: &str) -> String {
    match endpoint.split_once("://") {
        Some((scheme, rest)) => {
            let host_and_path = rest.split_once('@').map(|(_, h)| h).unwrap_or(rest);
            match host_and_path.split_once('/') {
                Some((host, path)) if !path.is_empty() => format!("{scheme}://{host}/***"),
                _ => format!("{scheme}://{host_and_path}"),
            }
        }
        None => "***".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_project_path_is_rejected() {
        let err = resolve_project("/definitely/not/here").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn a_file_is_not_a_project_directory() {
        let dir = std::env::temp_dir().join("superquery-startup-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-a-project");
        std::fs::write(&file, b"x").unwrap();

        let err = resolve_project(file.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("not a directory"));

        assert!(resolve_project(dir.to_str().unwrap()).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_endpoint_list_names_the_flag_to_use() {
        let err = resolve_endpoints(&[]).unwrap_err();
        assert!(err.to_string().contains("--rpc-url"));
    }

    #[test]
    fn endpoint_schemes_are_checked() {
        assert!(resolve_endpoints(&["https://eth.example".into()]).is_ok());
        assert!(resolve_endpoints(&["wss://eth.example".into()]).is_ok());

        let err = resolve_endpoints(&["eth.example".into()]).unwrap_err();
        assert!(err.to_string().contains("no supported scheme"));
    }

    #[test]
    fn api_keys_are_stripped_before_logging() {
        // Provider URLs carry keys in the path, and logs get shared.
        assert_eq!(
            redact_endpoint("https://eth-mainnet.g.alchemy.com/v2/SECRETKEY"),
            "https://eth-mainnet.g.alchemy.com/***"
        );
        assert_eq!(
            redact_endpoint("https://user:pass@node.example/rpc"),
            "https://node.example/***"
        );
        // Nothing sensitive to strip.
        assert_eq!(
            redact_endpoint("https://eth.example"),
            "https://eth.example"
        );
        assert_eq!(redact_endpoint("garbage"), "***");
    }
}
