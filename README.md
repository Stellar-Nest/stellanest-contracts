# Stellanest — Soroban Smart Contracts

Soroban smart contracts for Stellanest, a decentralized real estate index trading platform on Stellar.

## Contracts

| Contract | Description |
|---|---|
| `index-calculator` | Stores city real estate indices, calculates weighted averages from oracle data |
| `position-manager` | Manages leveraged long/short positions with collateralization and liquidation |
| `collateral-vault` | Holds USDC collateral for all positions, manages insurance fund |
| `price-oracle` | Multi-oracle price aggregation with weighted median and staleness checks |
| `index-token-factory` | Creates Stellar assets for city indices and position NFTs (SEP-0048) |

## Shared Types

The `types/` crate contains shared data structures used across all contracts:
- `IndexSnapshot`, `DataSource`, `CityConfig`
- `Position`, `Direction`, `PositionStatus`, `CloseResult`, `LiquidateResult`
- `AggregatedPrice`, `PriceSubmission`
- `PositionMetadata`

## Build

```bash
# Install Soroban CLI
cargo install --locked soroban-cli

# Build all contracts
cargo build --release

# Build a specific contract
cargo build --release -p stellanest-index-calculator
```

## Test

```bash
cargo test
```

## Deploy (Testnet)

```bash
# Configure network
soroban network add testnet \
  --rpc-url https://soroban-testnet.stellar.org:443 \
  --network-passphrase "Test SDF Network ; September 2015"

# Deploy each contract
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/stellanest_index_calculator.wasm \
  --network testnet
```

## Contract Architecture

```
                         ┌─────────────────┐
                         │  Stellanest API  │
                         └────────┬────────┘
                                  │
           ┌──────────────────────┼──────────────────────┐
           │                      │                      │
           ▼                      ▼                      ▼
┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐
│ Index Calculator  │  │ Position Manager │  │  Price Oracle     │
│                  │  │                  │  │                  │
│ update_index()   │  │ open_long()      │  │ submit_price()   │
│ get_current()    │  │ open_short()     │  │ get_price()      │
│ get_history()    │  │ close_position() │  │                  │
│                  │  │ liquidate()      │  │                  │
└────────┬─────────┘  └────────┬─────────┘  └────────┬─────────┘
         │                     │                     │
         │                     ▼                     │
         │           ┌──────────────────┐            │
         │           │ Collateral Vault │            │
         │           │                  │            │
         │           │ deposit()        │            │
         │           │ withdraw()       │            │
         │           │ lock_for_pos()   │            │
         │           │ seize()          │            │
         │           └──────────────────┘            │
         ▼                                           ▼
┌─────────────────────────────────────────────────────────────┐
│                      Stellar Network                         │
│  DEX (SDEX) · USDC · Index Token Factory · Position NFTs    │
└─────────────────────────────────────────────────────────────┘
```
