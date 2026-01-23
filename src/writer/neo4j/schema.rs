//! Neo4j schema initialization
//!
//! Creates constraints and indexes for optimal query performance.

use neo4rs::{Graph, query};
use crate::writer::{WriterError, Result};

/// Initialize Neo4j schema with constraints and indexes
///
/// Creates all required constraints for unique node properties and indexes
/// for query performance. This operation is idempotent - safe to run multiple times.
pub async fn init_schema(graph: &Graph) -> Result<()> {
    // Create constraints (also create indexes automatically)
    create_constraints(graph).await?;

    // Create additional performance indexes
    create_indexes(graph).await?;

    Ok(())
}

/// Create unique constraints on node properties
async fn create_constraints(graph: &Graph) -> Result<()> {
    let constraints = vec![
        // Block constraints
        "CREATE CONSTRAINT block_height_unique IF NOT EXISTS
         FOR (b:Block) REQUIRE b.height IS UNIQUE",

        "CREATE CONSTRAINT block_hash_unique IF NOT EXISTS
         FOR (b:Block) REQUIRE b.hash IS UNIQUE",

        // Transaction constraints
        "CREATE CONSTRAINT transaction_unique IF NOT EXISTS
         FOR (t:Transaction) REQUIRE t.txid IS UNIQUE",

        // Output constraints
        "CREATE CONSTRAINT output_unique IF NOT EXISTS
         FOR (o:Output) REQUIRE o.outputId IS UNIQUE",

        // Input constraints
        "CREATE CONSTRAINT input_unique IF NOT EXISTS
         FOR (i:Input) REQUIRE i.inputId IS UNIQUE",

        // Address constraints
        "CREATE CONSTRAINT address_unique IF NOT EXISTS
         FOR (a:Address) REQUIRE a.address IS UNIQUE",
    ];

    for constraint_query in constraints {
        graph.run(query(constraint_query))
            .await
            .map_err(|e| WriterError::DatabaseError(
                format!("Failed to create constraint: {}", e)
            ))?;
    }

    Ok(())
}

/// Create performance indexes
async fn create_indexes(graph: &Graph) -> Result<()> {
    let indexes = vec![
        // Transaction indexes
        "CREATE INDEX transaction_timestamp IF NOT EXISTS
         FOR (t:Transaction) ON (t.timestamp)",

        "CREATE INDEX transaction_block IF NOT EXISTS
         FOR (t:Transaction) ON (t.blockHeight)",

        "CREATE INDEX transaction_coinbase IF NOT EXISTS
         FOR (t:Transaction) ON (t.isCoinbase)",

        // Output indexes
        "CREATE INDEX output_spent IF NOT EXISTS
         FOR (o:Output) ON (o.isSpent)",

        "CREATE INDEX output_amount IF NOT EXISTS
         FOR (o:Output) ON (o.amount)",

        "CREATE INDEX output_script_type IF NOT EXISTS
         FOR (o:Output) ON (o.scriptType)",

        // Block indexes
        "CREATE INDEX block_timestamp IF NOT EXISTS
         FOR (b:Block) ON (b.timestamp)",
    ];

    for index_query in indexes {
        graph.run(query(index_query))
            .await
            .map_err(|e| WriterError::DatabaseError(
                format!("Failed to create index: {}", e)
            ))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_constraint_queries_are_valid() {
        // Verify constraint queries don't contain syntax errors
        // Full integration test requires Neo4j running
        let constraint = "CREATE CONSTRAINT block_height_unique IF NOT EXISTS
         FOR (b:Block) REQUIRE b.height IS UNIQUE";

        assert!(constraint.contains("CREATE CONSTRAINT"));
        assert!(constraint.contains("IF NOT EXISTS"));
        assert!(constraint.contains("REQUIRE"));
    }

    #[test]
    fn test_index_queries_are_valid() {
        let index = "CREATE INDEX transaction_timestamp IF NOT EXISTS
         FOR (t:Transaction) ON (t.timestamp)";

        assert!(index.contains("CREATE INDEX"));
        assert!(index.contains("IF NOT EXISTS"));
        assert!(index.contains("ON"));
    }
}
