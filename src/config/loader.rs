//! Configuration loading utilities

use super::{Config, ConfigError};
use std::path::Path;

/// Configuration loader with support for files, environment variables, and default profiles
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Config, ConfigError> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ConfigError::LoadError(format!("Failed to read {}: {}", path.display(), e))
        })?;

        let config: Config = toml::from_str(&contents)
            .map_err(|e| ConfigError::ParseError(format!("Failed to parse TOML: {}", e)))?;

        config.validate()?;

        Ok(config)
    }

    /// Load configuration from environment variables
    ///
    /// Expected environment variables:
    /// - NEO4J_URI
    /// - NEO4J_USER
    /// - NEO4J_PASSWORD
    /// - NEO4J_DATABASE
    /// - BATCH_SIZE
    /// - etc.
    pub fn from_env() -> Result<Config, ConfigError> {
        use config::{Config as ConfigBuilder, Environment};

        let builder = ConfigBuilder::builder()
            .add_source(Environment::with_prefix("BITCOIN_GRAPH"))
            .build()
            .map_err(|e| {
                ConfigError::LoadError(format!("Failed to load from environment: {}", e))
            })?;

        let config: Config = builder
            .try_deserialize()
            .map_err(|e| ConfigError::ParseError(format!("Failed to deserialize config: {}", e)))?;

        config.validate()?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.neo4j.uri, "bolt://localhost:7687");
        assert_eq!(config.neo4j.max_connections, 20);
        assert_eq!(config.ingestion.max_transaction_memory_mb, 600);
        assert_eq!(config.performance.utxo_cache_memory_mb, 140);
    }

    #[test]
    fn test_validation_rejects_zero_max_transaction_memory() {
        let mut config = Config::default();
        config.ingestion.max_transaction_memory_mb = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_rejects_min_greater_than_max_connections() {
        let mut config = Config::default();
        config.neo4j.min_connections = 20;
        config.neo4j.max_connections = 10;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_load_example_config() {
        let config = ConfigLoader::from_file("config.example/config.toml.example");
        assert!(
            config.is_ok(),
            "Failed to load example config: {:?}",
            config.err()
        );
        let config = config.unwrap();
        assert_eq!(config.bitcoin.blocks_dir, "/data/bitcoin/blocks");
        assert_eq!(config.ingestion.max_transaction_memory_mb, 600);
        assert_eq!(config.neo4j.max_connections, 20);
        assert_eq!(config.performance.utxo_cache_memory_mb, 140);
        assert_eq!(config.performance.utxo_prewarm_depth, 1_000_000);
        assert_eq!(config.performance.progress_report_interval, 500);
        assert_eq!(config.ingestion.validate_every_n_blocks, 10000);
    }
}
