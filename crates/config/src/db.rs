//! Postgres connection configuration.
//!
//! Two ways in, because two audiences want different things:
//!
//! - `--database-url postgres://…`, the single flag the guide's Milestone 1 CLI
//!   specifies, and what container platforms hand you;
//! - discrete `DB_HOST`/`DB_PORT`/`DB_USER`/`DB_PASS`/`DB_DATABASE` env vars,
//!   matching SubQuery's [`db.module.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/db.module.ts)
//!   so existing deployments keep working.
//!
//! A URL, when given, wins over the discrete variables.

use thiserror::Error;
use url::Url;

/// Failures while resolving database configuration.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DbConfigError {
    /// The connection URL could not be parsed.
    #[error("invalid database url: {0}")]
    InvalidUrl(String),

    /// The URL used a scheme other than `postgres`/`postgresql`.
    #[error("unsupported database scheme '{0}': expected postgres:// or postgresql://")]
    UnsupportedScheme(String),

    /// The URL carried no database name.
    #[error("database url has no database name (expected postgres://host/dbname)")]
    MissingDatabase,
}

/// Resolved Postgres connection settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConfig {
    /// Server host.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// Role to connect as.
    pub username: String,
    /// Password for `username`.
    pub password: String,
    /// Database name.
    pub database: String,
    /// Postgres schema holding this project's tables. One schema per project, so
    /// several projects can share a database.
    pub schema: String,
}

impl Default for DbConfig {
    fn default() -> Self {
        // Defaults match SubQuery's db.module.ts so existing setups carry over.
        Self {
            host: "127.0.0.1".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: "postgres".to_string(),
            database: "postgres".to_string(),
            schema: "public".to_string(),
        }
    }
}

impl DbConfig {
    /// Parse a `postgres://user:pass@host:port/dbname` URL.
    ///
    /// Absent components fall back to [`DbConfig::default`]. A `?schema=` query
    /// parameter selects the project schema.
    pub fn from_url(raw: &str) -> Result<Self, DbConfigError> {
        let url = Url::parse(raw).map_err(|e| DbConfigError::InvalidUrl(e.to_string()))?;

        match url.scheme() {
            "postgres" | "postgresql" => {}
            other => return Err(DbConfigError::UnsupportedScheme(other.to_string())),
        }

        let d = Self::default();
        let database = url.path().trim_start_matches('/').to_string();
        if database.is_empty() {
            return Err(DbConfigError::MissingDatabase);
        }

        let username = match url.username() {
            "" => d.username,
            u => decode(u),
        };
        let password = url.password().map(decode).unwrap_or(d.password);
        let schema = url
            .query_pairs()
            .find(|(k, _)| k == "schema")
            .map(|(_, v)| v.to_string())
            .unwrap_or(d.schema);

        Ok(Self {
            host: url.host_str().unwrap_or(&d.host).to_string(),
            port: url.port().unwrap_or(d.port),
            username,
            password,
            database,
            schema,
        })
    }

    /// Read the discrete `DB_*` environment variables, falling back to defaults.
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            host: std::env::var("DB_HOST").unwrap_or(d.host),
            port: std::env::var("DB_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.port),
            username: std::env::var("DB_USER").unwrap_or(d.username),
            password: std::env::var("DB_PASS").unwrap_or(d.password),
            database: std::env::var("DB_DATABASE").unwrap_or(d.database),
            schema: std::env::var("DB_SCHEMA").unwrap_or(d.schema),
        }
    }

    /// Resolve from an optional `--database-url`, else from the environment.
    pub fn resolve(database_url: Option<&str>) -> Result<Self, DbConfigError> {
        match database_url {
            Some(url) => Self::from_url(url),
            None => Ok(Self::from_env()),
        }
    }

    /// A libpq keyword/value connection string.
    ///
    /// Note this renders the password in clear text; never log the result.
    pub fn connection_string(&self) -> String {
        format!(
            "host={} port={} user={} password={} dbname={}",
            self.host, self.port, self.username, self.password, self.database
        )
    }

    /// A redacted description safe to log.
    pub fn redacted(&self) -> String {
        format!(
            "postgres://{}@{}:{}/{} (schema={})",
            self.username, self.host, self.port, self.database, self.schema
        )
    }
}

/// Percent-decode a URL userinfo component, leaving it unchanged if it is not
/// valid UTF-8 once decoded.
fn decode(s: &str) -> String {
    percent_decode(s).unwrap_or_else(|| s.to_string())
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16)?;
            let lo = (bytes[i + 2] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_url() {
        let c = DbConfig::from_url("postgres://alice:s3cret@db.internal:6543/indexer").unwrap();
        assert_eq!(c.host, "db.internal");
        assert_eq!(c.port, 6543);
        assert_eq!(c.username, "alice");
        assert_eq!(c.password, "s3cret");
        assert_eq!(c.database, "indexer");
        assert_eq!(c.schema, "public");
    }

    #[test]
    fn postgresql_scheme_is_accepted() {
        assert!(DbConfig::from_url("postgresql://localhost/app").is_ok());
    }

    #[test]
    fn missing_parts_fall_back_to_defaults() {
        let c = DbConfig::from_url("postgres://localhost/app").unwrap();
        assert_eq!(c.username, "postgres");
        assert_eq!(c.password, "postgres");
        assert_eq!(c.port, 5432);
        assert_eq!(c.database, "app");
    }

    #[test]
    fn schema_comes_from_the_query_string() {
        let c = DbConfig::from_url("postgres://localhost/app?schema=erc20").unwrap();
        assert_eq!(c.schema, "erc20");
    }

    #[test]
    fn percent_encoded_credentials_are_decoded() {
        // A password containing '@' and '/' must be encoded in the URL.
        let c = DbConfig::from_url("postgres://user:p%40ss%2Fword@localhost/app").unwrap();
        assert_eq!(c.password, "p@ss/word");
    }

    #[test]
    fn rejects_bad_urls() {
        assert!(matches!(
            DbConfig::from_url("mysql://localhost/app"),
            Err(DbConfigError::UnsupportedScheme(_))
        ));
        assert_eq!(
            DbConfig::from_url("postgres://localhost"),
            Err(DbConfigError::MissingDatabase)
        );
        assert!(matches!(
            DbConfig::from_url("not a url"),
            Err(DbConfigError::InvalidUrl(_))
        ));
    }

    #[test]
    fn redacted_output_hides_the_password() {
        let c = DbConfig::from_url("postgres://alice:s3cret@localhost/app").unwrap();
        let shown = c.redacted();
        assert!(!shown.contains("s3cret"), "password leaked: {shown}");
        assert!(shown.contains("alice"));
        assert!(shown.contains("app"));
    }
}
