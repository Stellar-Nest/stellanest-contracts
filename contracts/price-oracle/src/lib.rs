#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};
use stellanest_types::AggregatedPrice;

const STALENESS_THRESHOLD: u64 = 24 * 60 * 60; // 24 hours
const MIN_CONFIDENCE: u32 = 5000; // 50%

#[contract]
pub struct PriceOracle;

#[contractimpl]
impl PriceOracle {
    /// Initialize with admin and minimum oracle count for valid aggregation.
    pub fn initialize(env: Env, admin: Address, min_oracles: u32) {
        assert!(!env.storage().instance().has(&Symbol::new(&env, "admin")), "already initialized");
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        env.storage().instance().set(&Symbol::new(&env, "min_oracles"), &min_oracles);
        env.storage().instance().set(&Symbol::new(&env, "oracle_count"), &0u32);
    }

    /// Register an oracle with a weight for price aggregation.
    pub fn add_oracle(env: Env, admin: Address, oracle: Address, weight: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        env.storage().persistent().set(&Self::oracle_key(&env, &oracle), &weight);

        let count: u32 = env.storage().instance().get(&Symbol::new(&env, "oracle_count")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "oracle_count"), &(count + 1));
    }

    /// Remove an oracle.
    pub fn remove_oracle(env: Env, admin: Address, oracle: Address) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        env.storage().persistent().remove(&Self::oracle_key(&env, &oracle));

        let count: u32 = env.storage().instance().get(&Symbol::new(&env, "oracle_count")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "oracle_count"), &(count.saturating_sub(1)));
    }

    /// Set the minimum number of oracle submissions required for valid aggregation.
    pub fn set_min_oracles(env: Env, admin: Address, min: u32) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&Symbol::new(&env, "min_oracles"), &min);
    }

    /// Submit a price for a city. Oracle must be registered.
    pub fn submit_price(
        env: Env,
        oracle: Address,
        city: Symbol,
        price: i128,
        confidence: u32,
        timestamp: u64,
    ) {
        oracle.require_auth();
        let weight: u32 = env.storage().persistent().get(&Self::oracle_key(&env, &oracle)).unwrap_or(0);
        assert!(weight > 0, "oracle not registered");

        // Store submission
        let sub_key = Self::sub_key(&env, &oracle, &city);
        env.storage().temporary().set(&sub_key, &(price, confidence, timestamp));

        // Try to aggregate
        Self::try_aggregate(&env, &city);

        env.events().publish(
            (Symbol::new(&env, "price_submitted"), city),
            (oracle, price, confidence),
        );
    }

    /// Get the aggregated price for a city.
    pub fn get_price(env: Env, city: Symbol) -> AggregatedPrice {
        let agg_key = Symbol::new(&env, &format!("agg_{}", city));
        env.storage().persistent().get(&agg_key).unwrap_or(AggregatedPrice {
            city,
            price: 0,
            confidence: 0,
            oracle_count: 0,
            last_updated: 0,
        })
    }

    /// Get aggregated prices for all cities.
    pub fn get_all_prices(env: Env, cities: Vec<Symbol>) -> Vec<(Symbol, AggregatedPrice)> {
        let mut results = Vec::new(&env);
        for i in 0..cities.len() {
            let city = cities.get(i).unwrap();
            let price = Self::get_price(env.clone(), city.clone());
            results.push_back((city, price));
        }
        results
    }

    /// Get the number of registered oracles.
    pub fn get_oracle_count(env: Env) -> u32 {
        env.storage().instance().get(&Symbol::new(&env, "oracle_count")).unwrap_or(0)
    }
}

impl PriceOracle {
    fn oracle_key(env: &Env, addr: &Address) -> Symbol {
        Symbol::new(env, &format!("oracle_{}", addr))
    }

    fn sub_key(env: &Env, oracle: &Address, city: &Symbol) -> Symbol {
        Symbol::new(env, &format!("sub_{}_{}", oracle, city))
    }

    fn require_admin(env: &Env, addr: &Address) {
        let admin: Address = env.storage().instance().get(&Symbol::new(env, "admin")).unwrap();
        assert_eq!(*addr, admin, "not admin");
    }

    /// Collect submissions and compute weighted median.
    fn try_aggregate(env: &Env, city: &Symbol) {
        let now = env.ledger().timestamp();
        let min_oracles: u32 = env.storage().instance().get(&Symbol::new(env, "min_oracles")).unwrap();

        // Collect fresh submissions (within 1 hour window)
        let one_hour = 3600u64;
        // In production, iterate over registered oracles.
        // For scaffolding, we emit an event that the off-chain aggregator reads.
        // The on-chain aggregation is triggered after enough submissions arrive.

        // Simplified: store that aggregation was attempted
        let agg_key = Symbol::new(env, &format!("agg_{}", city));
        let existing: AggregatedPrice = env.storage().persistent().get(&agg_key).unwrap_or(AggregatedPrice {
            city: city.clone(),
            price: 0,
            confidence: 0,
            oracle_count: 0,
            last_updated: 0,
        });

        // Mark as needing aggregation — off-chain keeper will call aggregate()
        env.events().publish(
            (Symbol::new(env, "aggregate_needed"), city.clone()),
            now,
        );
    }
}
