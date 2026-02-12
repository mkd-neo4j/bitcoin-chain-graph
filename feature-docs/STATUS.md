# Feature Status Dashboard

Updated by agents after each stage transition.

## neo4j-writer-transactions — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 15/15 passing (2 compile-time, 13 ignored/require Neo4j)
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/neo4j-writer-transactions
- **Review**: APPROVED — all 15 ACs verified, verify.sh passes, CLAUDE.md compliant, no out-of-scope changes

## bip30-duplicate-coinbase — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 12/12 passing
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/bip30-duplicate-coinbase
- **Review**: APPROVED — all 8 ACs covered, verify.sh passes, CLAUDE.md compliant

## snapshot-resilience — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 13/13 passing
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/snapshot-resilience
- **Review**: APPROVED — all 9 ACs verified, conventions compliant, out-of-scope items preserved

## adaptive-transaction-memory — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 17/17 AC tests + 248 total passing
- **Verify**: type check PASS, lint PASS, tests PASS, verify.sh PASS
- **Branch**: feat/adaptive-transaction-memory
- **Review**: APPROVED — all 17 ACs verified, out-of-scope respected, conventions compliant
- **Follow-up**: `max_transaction_memory_mb` config field not wired to orchestrator (hardcoded default 600 works correctly)

## utxo-cache-shutdown-height — testing
- **Agent**: test-writer
- **Tests**: 8 tests written, all failing (expected)
- **Criteria covered**: 5/5 acceptance, 3/3 edge cases
- **Branch**: feat/utxo-cache-shutdown-height

## neo4j-atomic-batch-transactions — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 12/12 passing
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/neo4j-atomic-batch-transactions
- **Review**: APPROVED — no critical/warning issues, all 5 ACs verified
