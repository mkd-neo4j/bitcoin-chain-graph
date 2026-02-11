---
name: new-module
description: Scaffold a new Rust module following project conventions
allowed-tools: Read, Write, Edit, Bash, Glob, Grep
---

# /new-module — Scaffold a New Rust Module

Scaffolds a new Rust module following this project's conventions.

## Arguments

The user provides: `<layer> <module_name>`

Where `<layer>` is one of: `domain`, `parser`, `writer`, `config`

Example: `/new-module domain analytics`

## Steps

### 1. Create the module file

Create `src/<layer>/<module_name>.rs` with:

```rust
//! <Module description — ask user or infer from name>
//!
//! <Detailed explanation of what this module provides>

use crate::<layer>::<relevant_imports>;

/// <Type description>
#[derive(Clone, Debug)]
pub struct <TypeName> {
    /// <field description>
    pub field: Type,
}
```

### 2. Register in parent module

Add `pub mod <module_name>;` to `src/<layer>/mod.rs`.

If the module exports public types, consider adding specific re-exports.

### 3. Create test file

Create `tests/<layer>/<module_name>.rs` with:

```rust
//! Tests for <module_name> module

use bitcoin_chain_graph::<layer>::<TypeName>;

#[test]
fn test_<module_name>_basic() {
    // Basic construction/validation test
}
```

### 4. Register test module

Add to `tests/<layer>.rs`:

```rust
#[path = "<layer>/<module_name>.rs"]
mod <module_name>;
```

### 5. Verify

```bash
cargo check
cargo test <module_name>
cargo fmt
```
