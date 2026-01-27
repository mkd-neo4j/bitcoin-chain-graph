//! Drop and recreate Neo4j database
//!
//! Run with: cargo test --test drop_database -- --ignored --nocapture

use bitcoin_chain_graph::config::ConfigLoader;
use neo4rs::{ConfigBuilder, Graph};

#[tokio::test]
#[ignore]
async fn drop_and_recreate_database() {
    println!("Loading configuration...");
    let config = ConfigLoader::from_file("config/default.toml").expect("Failed to load config");

    println!("Connecting to system database...");

    let graph_config = ConfigBuilder::default()
        .uri(&config.neo4j.uri)
        .user(&config.neo4j.user)
        .password(&config.neo4j.password)
        .db("system") // Must connect to system to drop databases
        .build()
        .expect("Failed to build config");

    let graph = Graph::connect(graph_config)
        .await
        .expect("Failed to connect to Neo4j");

    println!(
        "\n⚠️  WARNING: This will drop and recreate the {} database!",
        config.neo4j.database
    );
    println!("   All data will be permanently deleted!");

    // Drop database
    let drop_query = format!("DROP DATABASE {} IF EXISTS", config.neo4j.database);
    println!("\nDropping database...");
    graph
        .run(neo4rs::query(&drop_query))
        .await
        .expect("Failed to drop database");

    // Wait a moment for drop to complete
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Recreate database
    let create_query = format!("CREATE DATABASE {}", config.neo4j.database);
    println!("Creating database...");
    graph
        .run(neo4rs::query(&create_query))
        .await
        .expect("Failed to create database");

    // Wait for database to be online
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    println!("✅ Database dropped and recreated successfully!");
    println!("\nVerify with:");
    println!(
        "  USE {}; MATCH (n) RETURN count(n);",
        config.neo4j.database
    );
}
