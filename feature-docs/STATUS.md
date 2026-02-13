# Feature Status Dashboard

Updated by agents after each stage transition.

## utxo-lookup-outside-transaction — testing
- **Agent**: test-writer
- **Tests**: 8 tests written, all failing (expected)
- **Criteria covered**: 6/7 acceptance (AC7 log ordering not testable without log capture), 3/3 edge cases
- **MockWriter**: Instrumented with call_log for ordering verification

## utxo-neo4j-fallback — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 15/15 passing (84 unit + 134 domain + 20 parser + 21 integration)
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/utxo-neo4j-fallback
- **Review**: APPROVED — all 8 ACs verified, 5 edge cases covered, rework #2 fixes confirmed, no regressions

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

## utxo-cache-shutdown-height — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 8/8 passing
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/utxo-cache-shutdown-height
- **Review**: APPROVED — all 5 ACs verified, main.rs integration correct, no convention violations

## drop-isspent-precompute-outputid — completed
- **Agent**: code-reviewer (approved)
- **Branch**: feat/drop-isspent-precompute-outputid
- **Review**: APPROVED — pre-dates STATUS.md tracking

## utxo-cache-persistence — completed
- **Agent**: code-reviewer (approved)
- **Branch**: feat/utxo-cache-persistence
- **Review**: APPROVED — pre-dates STATUS.md tracking

## neo4j-atomic-batch-transactions — completed
- **Agent**: code-reviewer (approved)
- **Tests**: 12/12 passing
- **Verify**: type check PASS, lint PASS, tests PASS
- **Branch**: feat/neo4j-atomic-batch-transactions
- **Review**: APPROVED — no critical/warning issues, all 5 ACs verified
