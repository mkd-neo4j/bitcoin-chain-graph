# Bitcoin Address Derivation from scriptPubKey

How to extract Bitcoin addresses from transaction outputs by parsing scriptPubKey locking scripts.

---

## Overview

Bitcoin addresses don't exist explicitly in the blockchain. Instead, each output has a **scriptPubKey** (locking script) that defines the conditions for spending. Addresses are a human-readable encoding of these scripts.

During ingestion, we must:
1. Parse the scriptPubKey to detect the script type
2. Extract the relevant hash/pubkey from the script
3. Encode it using the appropriate address format
4. Handle edge cases where no address can be derived

---

## Script Types and Detection

### Detection Strategy

Parse the scriptPubKey byte sequence and match against known patterns:

| Script Type | Pattern | Description |
|-------------|---------|-------------|
| **P2PKH** | `OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG` | Pay to Public Key Hash (legacy) |
| **P2SH** | `OP_HASH160 <20 bytes> OP_EQUAL` | Pay to Script Hash |
| **P2WPKH** | `OP_0 <20 bytes>` | Pay to Witness Public Key Hash (SegWit v0) |
| **P2WSH** | `OP_0 <32 bytes>` | Pay to Witness Script Hash (SegWit v0) |
| **P2TR** | `OP_1 <32 bytes>` | Pay to Taproot (SegWit v1) |
| **NULL_DATA** | `OP_RETURN <data>` | Unspendable data output |
| **P2PK** | `<pubkey> OP_CHECKSIG` | Pay to Public Key (obsolete) |
| **UNKNOWN** | Any other pattern | Non-standard or future script types |

**Implementation note:** Bitcoin opcodes are single bytes. `OP_DUP = 0x76`, `OP_HASH160 = 0xa9`, etc. See [Bitcoin Script Opcodes](https://en.bitcoin.it/wiki/Script#Opcodes).

---

## Address Extraction by Script Type

### P2PKH (Pay to Public Key Hash)

**Script pattern:**
```
OP_DUP OP_HASH160 <pubKeyHash> OP_EQUALVERIFY OP_CHECKSIG
76     a9         14 <20 bytes>  88            ac
```

**Extraction:**
1. Extract 20-byte `pubKeyHash` from script
2. Add version byte: `0x00` (mainnet) or `0x6f` (testnet)
3. Compute double SHA-256 checksum of versioned hash
4. Take first 4 bytes of checksum
5. Concatenate: `version + pubKeyHash + checksum`
6. Encode result in Base58

**Result:** Address starting with `1` (mainnet) or `m`/`n` (testnet)

**Example:** `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa` (Satoshi's genesis block address)

---

### P2SH (Pay to Script Hash)

**Script pattern:**
```
OP_HASH160 <scriptHash> OP_EQUAL
a9         14 <20 bytes> 87
```

**Extraction:**
1. Extract 20-byte `scriptHash` from script
2. Add version byte: `0x05` (mainnet) or `0xc4` (testnet)
3. Compute double SHA-256 checksum
4. Take first 4 bytes of checksum
5. Concatenate: `version + scriptHash + checksum`
6. Encode result in Base58

**Result:** Address starting with `3` (mainnet) or `2` (testnet)

**Example:** `3J98t1WpEZ73CNmYviecrnyiWrnqRhWNLy`

---

### P2WPKH (Pay to Witness Public Key Hash) - SegWit v0

**Script pattern:**
```
OP_0 <pubKeyHash>
00   14 <20 bytes>
```

**Extraction:**
1. Extract 20-byte `pubKeyHash` from script
2. Witness version: `0`
3. Encode using Bech32 with HRP (Human-Readable Part):
   - `bc` for mainnet
   - `tb` for testnet
4. Bech32 encoding of witness version + pubKeyHash

**Result:** Address starting with `bc1q` (mainnet) or `tb1q` (testnet)

**Example:** `bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4`

**Note:** All characters lowercase, uses Bech32 alphabet (no `1`, `b`, `i`, `o`)

---

### P2WSH (Pay to Witness Script Hash) - SegWit v0

**Script pattern:**
```
OP_0 <scriptHash>
00   20 <32 bytes>
```

**Extraction:**
1. Extract 32-byte `scriptHash` from script
2. Witness version: `0`
3. Encode using Bech32 with HRP `bc` (mainnet) or `tb` (testnet)
4. Bech32 encoding of witness version + scriptHash

**Result:** Address starting with `bc1q` (mainnet) - distinguishable from P2WPKH by longer length (62 chars vs 42)

**Example:** `bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3`

---

### P2TR (Pay to Taproot) - SegWit v1

**Script pattern:**
```
OP_1 <tweakedPubKey>
51   20 <32 bytes>
```

**Extraction:**
1. Extract 32-byte `tweakedPubKey` from script
2. Witness version: `1`
3. Encode using Bech32m (modified Bech32 for witness v1+) with HRP `bc` (mainnet) or `tb` (testnet)
4. Bech32m encoding of witness version + tweakedPubKey

**Result:** Address starting with `bc1p` (mainnet) or `tb1p` (testnet)

**Example:** `bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr`

**Note:** Uses Bech32m (different checksum constant than Bech32)

---

### NULL_DATA (OP_RETURN)

**Script pattern:**
```
OP_RETURN <data>
6a        <variable length data>
```

**Extraction:** **No address** - these outputs are provably unspendable.

**Action during ingestion:**
- Set `scriptType = 'NULL_DATA'`
- Do NOT create Address node
- Do NOT create LOCKED_TO relationship
- Common use cases: timestamping, metadata, protocol messages (Omni Layer, etc.)

---

### P2PK (Pay to Public Key) - Obsolete

**Script pattern:**
```
<pubKey> OP_CHECKSIG
```

Where `<pubKey>` is either:
- 65 bytes (uncompressed): `04 <x-coordinate 32 bytes> <y-coordinate 32 bytes>`
- 33 bytes (compressed): `02|03 <x-coordinate 32 bytes>`

**Extraction:**
1. Extract the public key
2. Compute HASH160 of public key: `RIPEMD160(SHA256(pubKey))`
3. Encode as P2PKH address (version `0x00` + hash + checksum in Base58)

**Note:** P2PK was used in early Bitcoin blocks (including genesis and early coinbase outputs). Satoshi's mining outputs used P2PK.

**Result:** Address starting with `1` (same encoding as P2PKH)

---

### UNKNOWN / Non-Standard Scripts

Any script that doesn't match known patterns:
- Set `scriptType = 'UNKNOWN'`
- Do NOT create Address node
- Do NOT create LOCKED_TO relationship
- Store raw `scriptPubKey` for future analysis

**Examples of non-standard scripts:**
- Multi-signature (bare multisig, not wrapped in P2SH)
- Time-locked scripts
- Custom/experimental scripts
- Future script types not yet defined

---

## Implementation Pseudocode

```python
def extract_address_from_scriptPubKey(scriptPubKey_hex: str) -> tuple[str, str]:
    """
    Returns (address, script_type)
    Returns (None, script_type) if no address can be derived
    """
    script_bytes = bytes.fromhex(scriptPubKey_hex)

    # Check P2PKH pattern
    if (len(script_bytes) == 25 and
        script_bytes[0] == 0x76 and  # OP_DUP
        script_bytes[1] == 0xa9 and  # OP_HASH160
        script_bytes[2] == 0x14 and  # Push 20 bytes
        script_bytes[23] == 0x88 and # OP_EQUALVERIFY
        script_bytes[24] == 0xac):   # OP_CHECKSIG

        pubkey_hash = script_bytes[3:23]
        address = base58_encode_with_checksum(b'\x00' + pubkey_hash)
        return (address, 'P2PKH')

    # Check P2SH pattern
    if (len(script_bytes) == 23 and
        script_bytes[0] == 0xa9 and  # OP_HASH160
        script_bytes[1] == 0x14 and  # Push 20 bytes
        script_bytes[22] == 0x87):   # OP_EQUAL

        script_hash = script_bytes[2:22]
        address = base58_encode_with_checksum(b'\x05' + script_hash)
        return (address, 'P2SH')

    # Check P2WPKH pattern (SegWit v0, 20 bytes)
    if (len(script_bytes) == 22 and
        script_bytes[0] == 0x00 and  # OP_0
        script_bytes[1] == 0x14):    # Push 20 bytes

        pubkey_hash = script_bytes[2:22]
        address = bech32_encode('bc', 0, pubkey_hash)  # 'bc' for mainnet
        return (address, 'P2WPKH')

    # Check P2WSH pattern (SegWit v0, 32 bytes)
    if (len(script_bytes) == 34 and
        script_bytes[0] == 0x00 and  # OP_0
        script_bytes[1] == 0x20):    # Push 32 bytes

        script_hash = script_bytes[2:34]
        address = bech32_encode('bc', 0, script_hash)
        return (address, 'P2WSH')

    # Check P2TR pattern (SegWit v1, 32 bytes)
    if (len(script_bytes) == 34 and
        script_bytes[0] == 0x51 and  # OP_1
        script_bytes[1] == 0x20):    # Push 32 bytes

        tweaked_pubkey = script_bytes[2:34]
        address = bech32m_encode('bc', 1, tweaked_pubkey)  # Note: bech32m not bech32
        return (address, 'P2TR')

    # Check OP_RETURN pattern
    if len(script_bytes) > 0 and script_bytes[0] == 0x6a:  # OP_RETURN
        return (None, 'NULL_DATA')

    # Check P2PK pattern (obsolete but exists in early blocks)
    if len(script_bytes) in [67, 35]:  # 65-byte or 33-byte pubkey + OP_CHECKSIG
        if script_bytes[-1] == 0xac:  # OP_CHECKSIG
            pubkey = script_bytes[1:-1] if script_bytes[0] in [0x41, 0x21] else script_bytes[:-1]
            pubkey_hash = hash160(pubkey)  # RIPEMD160(SHA256(pubkey))
            address = base58_encode_with_checksum(b'\x00' + pubkey_hash)
            return (address, 'P2PK')

    # Unknown/non-standard script
    return (None, 'UNKNOWN')
```

---

## Encoding Reference

### Base58 Encoding (P2PKH, P2SH, P2PK)

**Algorithm:**
1. Prepend version byte to data
2. Compute checksum: `SHA256(SHA256(version + data))`
3. Take first 4 bytes of checksum
4. Concatenate: `version + data + checksum`
5. Encode result in Base58

**Base58 alphabet:** `123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz`
(Excludes `0`, `O`, `I`, `l` to avoid ambiguity)

**Libraries:**
- Python: `base58` package
- JavaScript: `bs58` package
- Rust: `bs58` crate

---

### Bech32 Encoding (P2WPKH, P2WSH)

**Algorithm:**
1. Convert witness version and hash to 5-bit groups
2. Compute Bech32 checksum
3. Concatenate HRP + separator + data + checksum
4. Encode using Bech32 alphabet

**Bech32 alphabet:** `qpzry9x8gf2tvdw0s3jn54khce6mua7l`

**Libraries:**
- Python: `bech32` package or `segwit_addr` module
- JavaScript: `bech32` package
- Rust: `bech32` crate

**Reference:** [BIP-173](https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki)

---

### Bech32m Encoding (P2TR)

**Difference from Bech32:** Modified checksum constant (`0x2bc830a3` instead of `1`)

**Algorithm:** Same as Bech32 but with different checksum calculation for witness version ≥ 1.

**Libraries:** Same as Bech32 (ensure library supports Bech32m for witness v1+)

**Reference:** [BIP-350](https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki)

---

## Testing Address Derivation

### Test Vectors

**Genesis block coinbase output (P2PK):**
- scriptPubKey: `4104678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5fac`
- Expected address: `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa`
- Script type: P2PK

**Early P2PKH example:**
- scriptPubKey: `76a91489abcdefabbaabbaabbaabbaabbaabbaabbaabba88ac`
- Extract bytes 3-23: `89abcdefabbaabbaabbaabbaabbaabbaabbaabba`
- Expected address starts with `1` (exact address depends on hash)
- Script type: P2PKH

**SegWit example (P2WPKH):**
- scriptPubKey: `0014751e76e8199196d454941c45d1b3a323f1433bd6`
- Extract bytes 2-22: `751e76e8199196d454941c45d1b3a323f1433bd6`
- Expected address: `bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4`
- Script type: P2WPKH

**Test strategy:**
1. Use known addresses from blockchain explorers
2. Parse their scriptPubKeys
3. Verify your derivation produces the same address
4. Test all script types: P2PKH, P2SH, P2WPKH, P2WSH, P2TR

---

## Edge Cases and Considerations

### 1. Mainnet vs. Testnet
- Use correct version bytes and HRPs
- Mainnet: P2PKH=`0x00`, P2SH=`0x05`, Bech32 HRP=`bc`
- Testnet: P2PKH=`0x6f`, P2SH=`0xc4`, Bech32 HRP=`tb`

### 2. Uncompressed vs. Compressed Public Keys
- P2PK outputs may have 65-byte (uncompressed) or 33-byte (compressed) pubkeys
- Both hash to different addresses!
- Early Bitcoin used uncompressed; modern wallets use compressed

### 3. Multisig Scripts
- Bare multisig (not wrapped in P2SH) has no single address
- Treat as UNKNOWN or extract multiple pubkeys and store separately
- Most modern multisig uses P2SH or P2WSH wrappers

### 4. Null Data Length
- OP_RETURN outputs can carry up to 80 bytes of data (consensus limit)
- No address derivation needed

### 5. Future Witness Versions
- SegWit supports witness versions 0-16 (OP_0 through OP_16)
- Only v0 (P2WPKH, P2WSH) and v1 (P2TR) are currently defined
- v2-v16 reserved for future upgrades
- If encountered, mark as UNKNOWN until specification is published

---

## Implementation Checklist

- [ ] Implement Base58Check encoding/decoding
- [ ] Implement Bech32 encoding (for SegWit v0)
- [ ] Implement Bech32m encoding (for SegWit v1 Taproot)
- [ ] Detect and parse P2PKH scripts
- [ ] Detect and parse P2SH scripts
- [ ] Detect and parse P2WPKH scripts
- [ ] Detect and parse P2WSH scripts
- [ ] Detect and parse P2TR scripts
- [ ] Handle P2PK (obsolete but present in early blocks)
- [ ] Handle OP_RETURN (NULL_DATA) - no address
- [ ] Handle unknown/non-standard scripts - no address
- [ ] Test with genesis block and early blocks (P2PK)
- [ ] Test with modern blocks (SegWit, Taproot)
- [ ] Use correct network parameters (mainnet vs testnet)

---

## Recommended Libraries

### Python
- **bitcoinlib**: Comprehensive Bitcoin library with address encoding
- **python-bitcoinlib**: Low-level Bitcoin protocol implementation
- **base58**: Base58Check encoding
- **bech32**: Bech32/Bech32m encoding

### JavaScript/TypeScript
- **bitcoinjs-lib**: Full Bitcoin library with address utilities
- **bs58**: Base58 encoding
- **bech32**: Bech32/Bech32m encoding

### Rust
- **rust-bitcoin**: Bitcoin library with full address support
- **bs58**: Base58 encoding
- **bech32**: Bech32/Bech32m encoding

---

## References

- [Bitcoin Developer Reference - Addresses](https://developer.bitcoin.org/devguide/payment_processing.html#verifying-payment)
- [Bitcoin Script Opcodes](https://en.bitcoin.it/wiki/Script#Opcodes)
- [BIP-13: Address Format for P2SH](https://github.com/bitcoin/bips/blob/master/bip-0013.mediawiki)
- [BIP-141: Segregated Witness](https://github.com/bitcoin/bips/blob/master/bip-0141.mediawiki)
- [BIP-173: Bech32 (SegWit v0)](https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki)
- [BIP-341: Taproot](https://github.com/bitcoin/bips/blob/master/bip-0341.mediawiki)
- [BIP-350: Bech32m (SegWit v1+)](https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki)
