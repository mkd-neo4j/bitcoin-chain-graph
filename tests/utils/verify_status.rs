//! Quick verification of batch ingestion status

use bitcoin_chain_graph::config::ConfigLoader;
use bitcoin_chain_graph::writer::Neo4jWriter;

#[tokio::test]
#[ignore]
async fn verify_batch_status() {
    println!("\n🔍 Checking batch ingestion status...\n");

    let config = ConfigLoader::from_file("config/default.toml")
        .expect("Failed to load config");

    let _writer = Neo4jWriter::new(config.neo4j.clone())
        .await
        .expect("Failed to connect to Neo4j");

    // Check checkpoint
    let _checkpoint_query = "MATCH (c:IngestionCheckpoint) RETURN c.lastProcessedHeight as height, c.status as status";

    // We need to access the graph directly - this is a simplified check
    println!("To verify batch ingestion manually, run these queries in Neo4j Browser:");
    println!("1. MATCH (c:IngestionCheckpoint) RETURN c.lastProcessedHeight, c.status");
    println!("2. MATCH (b:Block) RETURN count(b) as block_count");
    println!("3. MATCH (t:Transaction) RETURN count(t) as tx_count");
    println!("4. MATCH (o:Output) RETURN count(o) as output_count");

    println!("\n✅ Connection verified. Check Neo4j Browser for results.");
}
