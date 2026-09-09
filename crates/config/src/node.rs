//! [`NodeConfig`] — the node's runtime settings and CLI surface.
//!
//! The flag set the guide's Milestone 1 specifies is the spine:
//!
//! ```bash
//! superquery-node --project ./project --database-url postgres://... --rpc-url https://...
//! ```
//!
//! Everything else has a default that lets that command run. Tuning knobs are
//! grouped by the subsystem they belong to and carry the same names those
//! subsystems use, so a flag is traceable to the code it affects.

use clap::Parser;

/// How much history the store keeps per entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum HistoricalMode {
    /// Version entities by block height. Required for rewind (guide Milestone 11).
    #[default]
    Height,
    /// Version entities by block timestamp. Used for multi-chain alignment.
    Timestamp,
    /// Keep only current state. Cheapest, but reorgs cannot be rewound.
    Disabled,
}

impl HistoricalMode {
    /// Whether entity versions are retained, and therefore whether rewind is
    /// possible.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, HistoricalMode::Disabled)
    }
}

/// Node configuration, parsed from CLI flags and environment variables.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "superquery-node",
    version,
    about = "SuperQuery blockchain indexing engine",
    long_about = "Indexes blockchain data into PostgreSQL according to a SuperQuery project.\n\
                  Query the result with superquery-query."
)]
pub struct NodeConfig {
    // --- Milestone 1: the three flags that define a run ---
    /// Path to the built SuperQuery project directory.
    #[arg(short, long, env = "SUPERQUERY_PROJECT")]
    pub project: String,

    /// Postgres connection URL. Falls back to the `DB_*` environment variables.
    #[arg(short, long, env = "DATABASE_URL")]
    pub database_url: Option<String>,

    /// Chain RPC endpoint(s). Repeat the flag to supply failover endpoints.
    #[arg(
        short,
        long = "rpc-url",
        env = "SUPERQUERY_RPC_URL",
        value_delimiter = ','
    )]
    pub rpc_urls: Vec<String>,

    // --- Range selection ---
    /// Height to start indexing from. Defaults to the project's start block.
    #[arg(long)]
    pub start_height: Option<u64>,

    /// Height to stop at. Runs indefinitely when unset.
    #[arg(long)]
    pub end_height: Option<u64>,

    // --- Fetch scheduler (guide Milestone 5) ---
    /// Blocks fetched per batch.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..))]
    pub batch_size: u32,

    /// Maximum concurrent in-flight block fetches.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_in_flight: u32,

    /// Per-RPC-request timeout, in seconds.
    #[arg(long, default_value_t = 30)]
    pub rpc_timeout_secs: u64,

    /// Attempts per RPC request before the block fails.
    #[arg(long, default_value_t = 5)]
    pub rpc_max_retries: u32,

    // --- Dispatcher (guide Milestone 7) ---
    /// Blocks that may sit decoded and awaiting commit. Bounds memory.
    #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..))]
    pub queue_capacity: u32,

    /// Mapping worker count. Defaults to the machine's parallelism.
    #[arg(long)]
    pub workers: Option<u32>,

    // --- Store (guide Milestone 2) ---
    /// Entity history retention.
    #[arg(long, value_enum, default_value_t = HistoricalMode::Height)]
    pub historical: HistoricalMode,

    /// Row cap applied to unbounded `getByField` queries from mappings.
    #[arg(long, default_value_t = 100)]
    pub query_limit: u32,

    // --- Finality (guide Milestone 11) ---
    /// Index unfinalized blocks and handle reorgs. Lower latency, needs rewind.
    #[arg(long, default_value_t = false)]
    pub unfinalized_blocks: bool,

    /// Blocks behind the head treated as final, for chains without a finality
    /// gadget.
    #[arg(long, default_value_t = 200)]
    pub finality_confirmations: u64,

    // --- Mapping runtime (guide Milestone 9) ---
    /// Wasm fuel budget per handler invocation. Bounds runaway mappings.
    #[arg(long, default_value_t = 10_000_000_000)]
    pub mapping_fuel: u64,

    /// Wall-clock timeout per handler invocation, in milliseconds.
    #[arg(long, default_value_t = 5_000)]
    pub mapping_timeout_ms: u64,

    /// Memory ceiling per mapping instance, in MiB.
    #[arg(long, default_value_t = 256)]
    pub mapping_memory_mb: u32,

    // --- Admin surface (guide Milestone 13) ---
    /// Address for `/health`, `/ready` and `/metrics`.
    #[arg(long, default_value = "127.0.0.1:3000")]
    pub admin_addr: String,

    /// Disable the admin/metrics server.
    #[arg(long, default_value_t = false)]
    pub no_admin: bool,

    // --- Observability ---
    /// Log level: `trace`, `debug`, `info`, `warn` or `error`.
    #[arg(long, default_value = "info", env = "SUPERQUERY_LOG_LEVEL")]
    pub log_level: String,

    /// Emit JSON logs instead of human-readable ones.
    #[arg(long, default_value_t = false)]
    pub log_json: bool,
}

impl NodeConfig {
    /// Build a config with every default applied, for tests and embedding.
    pub fn with_defaults(project: impl Into<String>) -> Self {
        // Parsing the required flag is the only way to guarantee this stays in
        // step with the clap definition above.
        Self::try_parse_from(["superquery-node", "--project", &project.into()])
            .expect("default NodeConfig must parse")
    }

    /// Worker count, resolving the default to the machine's parallelism.
    pub fn resolved_workers(&self) -> usize {
        self.workers.map(|w| w as usize).unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        })
    }

    /// Validate cross-field constraints clap cannot express on its own.
    ///
    /// Called once at startup so a contradictory invocation fails immediately
    /// rather than part-way through a run.
    pub fn validate(&self) -> Result<(), String> {
        if let (Some(start), Some(end)) = (self.start_height, self.end_height) {
            if end < start {
                return Err(format!(
                    "--end-height ({end}) is below --start-height ({start})"
                ));
            }
        }
        if self.unfinalized_blocks && !self.historical.is_enabled() {
            return Err("--unfinalized-blocks requires entity history for rewind; \
                 remove --historical disabled"
                .to_string());
        }
        if self.queue_capacity < self.batch_size {
            return Err(format!(
                "--queue-capacity ({}) is below --batch-size ({}), which would \
                 stall the dispatcher",
                self.queue_capacity, self.batch_size
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_1_invocation_parses() {
        let c = NodeConfig::try_parse_from([
            "superquery-node",
            "--project",
            "./erc20-indexer/dist",
            "--database-url",
            "postgres://localhost/app",
            "--rpc-url",
            "https://eth.example",
        ])
        .expect("guide Milestone 1 command line must parse");

        assert_eq!(c.project, "./erc20-indexer/dist");
        assert_eq!(c.database_url.as_deref(), Some("postgres://localhost/app"));
        assert_eq!(c.rpc_urls, vec!["https://eth.example"]);
    }

    #[test]
    fn project_is_the_only_required_flag() {
        let c = NodeConfig::with_defaults("./project");
        assert_eq!(c.batch_size, 100);
        assert_eq!(c.historical, HistoricalMode::Height);
        assert!(!c.unfinalized_blocks);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn multiple_rpc_endpoints() {
        let c = NodeConfig::try_parse_from([
            "superquery-node",
            "--project",
            "./p",
            "--rpc-url",
            "https://a.example",
            "--rpc-url",
            "https://b.example",
        ])
        .unwrap();
        assert_eq!(c.rpc_urls.len(), 2);

        // Comma-separated works too, for env-var supplied lists.
        let c = NodeConfig::try_parse_from([
            "superquery-node",
            "--project",
            "./p",
            "--rpc-url",
            "https://a.example,https://b.example",
        ])
        .unwrap();
        assert_eq!(c.rpc_urls.len(), 2);
    }

    #[test]
    fn rejects_inverted_height_range() {
        let mut c = NodeConfig::with_defaults("./p");
        c.start_height = Some(500);
        c.end_height = Some(100);
        assert!(c.validate().unwrap_err().contains("below --start-height"));
    }

    #[test]
    fn rejects_reorg_handling_without_history() {
        let mut c = NodeConfig::with_defaults("./p");
        c.unfinalized_blocks = true;
        c.historical = HistoricalMode::Disabled;
        assert!(c
            .validate()
            .unwrap_err()
            .contains("requires entity history"));
    }

    #[test]
    fn rejects_a_queue_smaller_than_a_batch() {
        let mut c = NodeConfig::with_defaults("./p");
        c.batch_size = 500;
        c.queue_capacity = 100;
        assert!(c.validate().unwrap_err().contains("stall the dispatcher"));
    }

    #[test]
    fn zero_batch_size_is_rejected_by_the_parser() {
        assert!(NodeConfig::try_parse_from([
            "superquery-node",
            "--project",
            "./p",
            "--batch-size",
            "0",
        ])
        .is_err());
    }

    #[test]
    fn workers_default_to_available_parallelism() {
        let c = NodeConfig::with_defaults("./p");
        assert!(c.resolved_workers() >= 1);

        let mut c2 = c.clone();
        c2.workers = Some(4);
        assert_eq!(c2.resolved_workers(), 4);
    }

    #[test]
    fn cli_definition_is_internally_valid() {
        use clap::CommandFactory;
        NodeConfig::command().debug_assert();
    }
}
