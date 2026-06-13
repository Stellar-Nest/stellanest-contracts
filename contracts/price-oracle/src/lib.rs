#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};
use stellanest_types::{AggregatedPrice, PriceSubmission};

#[contract]
pub struct PriceOracle;

#[contractimpl]
impl PriceOracle {
    /// Initialize with admin and minimum oracle count for valid aggregation.
    pub fn initialize(env: Env, admin: Address, min_oracles: u32) {
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

        // Build submission
        let submission = PriceSubmission {
            oracle: oracle.clone(),
            city: city.clone(),
            price,
            confidence,
            timestamp,
        };

        // Store in city-level submissions Vec (update existing or append)
        let submissions_key = Symbol::new(&env, &format!("submissions_{}", city));
        let submissions: Vec<PriceSubmission> = env.storage().persistent()
            .get(&submissions_key)
            .unwrap_or(Vec::new(&env));

        let mut updated = Vec::new(&env);
        let mut found = false;
        for i in 0..submissions.len() {
            let sub = submissions.get(i).unwrap();
            if sub.oracle == oracle {
                updated.push_back(submission.clone());
                found = true;
            } else {
                updated.push_back(sub);
            }
        }
        if !found {
            updated.push_back(submission);
        }
        env.storage().persistent().set(&submissions_key, &updated);

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

    fn require_admin(env: &Env, addr: &Address) {
        let admin: Address = env.storage().instance().get(&Symbol::new(env, "admin")).unwrap();
        assert_eq!(*addr, admin, "not admin");
    }

    /// Collect submissions and compute weighted median.
    fn try_aggregate(env: &Env, city: &Symbol) {
        let now = env.ledger().timestamp();
        let min_oracles: u32 = env.storage().instance().get(&Symbol::new(env, "min_oracles")).unwrap();
        let one_hour = 3600u64;

        // Read all submissions for this city
        let submissions_key = Symbol::new(env, &format!("submissions_{}", city));
        let submissions: Vec<PriceSubmission> = env.storage().persistent()
            .get(&submissions_key)
            .unwrap_or(Vec::new(env));

        // Collect fresh submissions (within 1 hour window) into price/confidence pairs
        let mut recent_prices = Vec::<(i128, u32)>::new(env);
        let mut total_confidence: u32 = 0;

        for i in 0..submissions.len() {
            let sub = submissions.get(i).unwrap();
            if now - sub.timestamp < one_hour {
                recent_prices.push_back((sub.price, sub.confidence));
                total_confidence += sub.confidence;
            }
        }

        // Need minimum number of oracle submissions to aggregate
        if recent_prices.len() < min_oracles {
            return;
        }

        // Bubble sort by price for median calculation
        let len = recent_prices.len();
        for i in 0..len {
            for j in 0..len - 1 - i {
                let a = recent_prices.get(j).unwrap();
                let b = recent_prices.get(j + 1).unwrap();
                if a.0 > b.0 {
                    recent_prices.set(j, b);
                    recent_prices.set(j + 1, a);
                }
            }
        }

        // Weighted median: walk through sorted prices accumulating confidence weights
        let mid_weight = total_confidence / 2;
        let mut running_weight: u32 = 0;
        let mut median_price = recent_prices.get(0).unwrap().0;

        for i in 0..recent_prices.len() {
            let (price, conf) = recent_prices.get(i).unwrap();
            running_weight += conf;
            if running_weight >= mid_weight {
                median_price = price;
                break;
            }
        }

        // Confidence is average of submission confidences
        let avg_confidence = total_confidence / recent_prices.len();

        // Write aggregated price
        let agg = AggregatedPrice {
            city: city.clone(),
            price: median_price,
            confidence: avg_confidence,
            oracle_count: recent_prices.len(),
            last_updated: now,
        };

        let agg_key = Symbol::new(env, &format!("agg_{}", city));
        env.storage().persistent().set(&agg_key, &agg);

        // Emit event
        env.events().publish(
            (Symbol::new(env, "price_aggregated"), city.clone()),
            agg,
        );
    }
}
