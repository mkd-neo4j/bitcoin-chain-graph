//! Clear Neo4j database
//!
//! Run with: cargo test --test clear_neo4j -- --ignored --nocapture

use bitcoin_chain_graph::config::ConfigLoader;
use neo4rs::{ConfigBuilder, Graph};

#[tokio::test]
#[ignore]
async fn clear_database() {
    println!("Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml").expect("Failed to load config");

    println!("Connecting to Neo4j at {}...", config.neo4j.uri);

    let graph_config = ConfigBuilder::default()
        .uri(&config.neo4j.uri)
        .user(&config.neo4j.user)
        .password(&config.neo4j.password)
        .db(&*config.neo4j.database)
        .build()
        .expect("Failed to build config");

    let graph = Graph::connect(graph_config)
        .await
        .expect("Failed to connect to Neo4j");

    println!("Clearing all data from Neo4j...");

    // Delete in batches with CALL {} syntax to ensure transaction commits
    // The CALL {} IN TRANSACTIONS syntax commits after each batch
    let query = "MATCH (n)
                 CALL { WITH n DETACH DELETE n }
                 IN TRANSACTIONS OF 1000 ROWS";

    match graph.run(neo4rs::query(query)).await {
        Ok(_) => {
            println!("✅ Database cleared successfully!");

            // Verify the delete worked
            let verify_query = "MATCH (n) RETURN count(n) as count";
            if let Ok(mut result) = graph.execute(neo4rs::query(verify_query)).await {
                if let Some(row) = result.next().await.expect("Failed to get row") {
                    let count: i64 = row.get("count").unwrap_or(0);
                    println!("   Verified: {} nodes remaining", count);
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to clear database: {}", e);
        }
    }
}
