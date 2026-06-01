#![no_std]
use soroban_sdk::{contracttype, Symbol};

/// Index snapshot — the core data point for a city real estate index.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexSnapshot {
    pub city: Symbol,
    pub value: i128,        // Index value in basis points (10000 = 1.0x = $100 base)
    pub change_24h: i32,    // 24h change in basis points
    pub change_30d: i32,    // 30d change in basis points
    pub source_count: u32,  // Number of data sources contributing
    pub timestamp: u64,     // Unix timestamp of snapshot
}

/// Data source configuration for a city index.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataSource {
    pub name: Symbol,
    pub weight: u32,        // Weight in basis points (10000 = 100%)
    pub last_value: i128,
    pub last_updated: u64,
}

/// City configuration and metadata.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CityConfig {
    pub city: Symbol,
    pub name: Symbol,
    pub country: Symbol,
    pub base_value: i128,   // Initial index value at launch
    pub sources: soroban_sdk::Vec<DataSource>,
    pub status: Symbol,     // "active", "paused", "delisted"
}

/// Position direction.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Long,
    Short,
}

/// Position lifecycle status.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionStatus {
    Open,
    Closed,
    Liquidated,
}

/// A leveraged position on a city index.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub id: u128,
    pub user: soroban_sdk::Address,
    pub city: Symbol,
    pub direction: Direction,
    pub leverage: u32,
    pub entry_price: i128,      // Index value at open
    pub collateral: i128,       // USDC locked
    pub size: i128,             // Position size = collateral * leverage
    pub liquidation_price: i128,
    pub status: PositionStatus,
    pub opened_at: u64,
    pub funding_paid: i128,
}

/// Result of closing a position.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseResult {
    pub position_id: u128,
    pub entry_price: i128,
    pub exit_price: i128,
    pub pnl: i128,
    pub collateral_returned: i128,
}

/// Result of a liquidation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiquidateResult {
    pub position_id: u128,
    pub collateral_seized: i128,
    pub penalty: i128,
    pub to_insurance_fund: i128,
}

/// Aggregated oracle price for a city.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregatedPrice {
    pub city: Symbol,
    pub price: i128,        // Weighted median
    pub confidence: u32,    // 0-10000 basis points
    pub oracle_count: u32,
    pub last_updated: u64,
}

/// Individual oracle price submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceSubmission {
    pub oracle: soroban_sdk::Address,
    pub city: Symbol,
    pub price: i128,
    pub confidence: u32,
    pub timestamp: u64,
}

/// Metadata for position NFTs (SEP-0048).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionMetadata {
    pub position_id: u128,
    pub city: Symbol,
    pub direction: Direction,
    pub leverage: u32,
    pub entry_price: i128,
    pub collateral: i128,
    pub opened_at: u64,
}
