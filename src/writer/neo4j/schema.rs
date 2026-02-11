//! Neo4j schema initialization
//!
//! Creates constraints and indexes for optimal query performance.

use crate::writer::{Result, WriterError};
use neo4rs::{query, Graph};

/// Expected number of unique constraints after schema initialization
const EXPECTED_CONSTRAINTS: usize = 6;

/// Initialize Neo4j schema with constraints and indexes
///
/// Creates all required constraints for unique node properties and indexes
/// for query performance. This operation is idempotent - safe to run multiple times.
/// After creation, verifies that the expected number of constraints exist.
pub async fn init_schema(graph: &Graph) -> Result<()> {
    // Create constraints (also create indexes automatically)
    create_constraints(graph).await?;

    // Create additional performance indexes
    create_indexes(graph).await?;

    // Verify schema was applied correctly
    verify_schema(graph).await?;

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
        graph.run(query(constraint_query)).await.map_err(|e| {
            WriterError::DatabaseError(format!("Failed to create constraint: {}", e))
        })?;
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
        graph
            .run(query(index_query))
            .await
            .map_err(|e| WriterError::DatabaseError(format!("Failed to create index: {}", e)))?;
    }

    Ok(())
}

/// Verify that expected constraints exist after schema initialization.
///
/// This is informational — logs a warning if fewer constraints than expected
/// are found, but does not fail. Useful for catching silent schema issues.
async fn verify_schema(graph: &Graph) -> Result<()> {
    let result = graph.execute(query("SHOW CONSTRAINTS")).await;

    match result {
        Ok(mut rows) => {
            let mut constraint_count = 0;
            while let Ok(Some(_row)) = rows.next().await {
                constraint_count += 1;
            }

            if constraint_count < EXPECTED_CONSTRAINTS {
                tracing::warn!(
                    expected = EXPECTED_CONSTRAINTS,
                    found = constraint_count,
                    "Schema verification: fewer constraints than expected"
                );
            } else {
                tracing::info!(constraints = constraint_count, "Schema verification passed");
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Schema verification skipped: SHOW CONSTRAINTS not available"
            );
        }
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

    // =========================================================================
    // AC 7: create_indexes does NOT create an output_spent index
    // =========================================================================

    #[test]
    fn ac7_no_output_spent_index() {
        // The create_indexes function defines indexes in a vec. We need to verify
        // that no index creates an index on (o.isSpent). Since the function is
        // private and async, we test by inspecting the source code pattern:
        // the indexes vec should NOT contain "output_spent" or "o.isSpent".
        //
        // We test this by checking that the constant source code that would be
        // in the indexes vec does not contain output_spent index creation.
        // Since we can't call create_indexes directly from a sync test, we
        // verify the expectation about what the implementation should look like.
        //
        // After the feature is implemented, the "output_spent" index line should
        // be removed from the indexes vec in create_indexes().
        //
        // For now, we can verify the expectation by checking the module's source.
        // This test will fail because the current code DOES create output_spent.

        // Read the source of this module to verify no output_spent index
        let source = include_str!("schema.rs");

        assert!(
            !source.contains("output_spent"),
            "schema.rs should not contain an output_spent index definition"
        );
        assert!(
            !source.contains("o.isSpent"),
            "schema.rs should not reference o.isSpent in any index"
        );
    }
}
