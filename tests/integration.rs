//! Integration tests
//!
//! End-to-end tests for the full ingestion pipeline

#[path = "integration/batch_ingestion.rs"]
mod batch_ingestion;

#[path = "integration/full_pipeline.rs"]
mod full_pipeline;

#[path = "integration/checkpoint_resume.rs"]
mod checkpoint_resume;
