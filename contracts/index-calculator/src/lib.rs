#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};
use stellanest_types::{CityConfig, DataSource, IndexSnapshot};

/// Storage keys
mod keys {
    use soroban_sdk::Symbol;
    pub fn admin(env: &Env) -> Symbol { Symbol::new(env, "admin") }
    pub fn city(env: &Env, city: &Symbol) -> Symbol {
        // city_{code} — stores CityConfig
        Symbol::new(env, &format!("city_{}", city))
    }
    pub fn current(env: &Env, city: &Symbol) -> Symbol {
        Symbol::new(env, &format!("cur_{}", city))
    }
    pub fn oracle(env: &Env, addr: &Address) -> Symbol {
        Symbol::new(env, &format!("oracle_{}", addr))
    }
    pub fn city_list(env: &Env) -> Symbol { Symbol::new(env, "cities") }
}

const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60;

#[contract]
pub struct IndexCalculator;

#[contractimpl]
impl IndexCalculator {
    /// Initialize the contract with an admin address.
    pub fn initialize(env: Env, admin: Address) {
        assert!(!env.storage().instance().has(&keys::admin(&env)), "already initialized");
        env.storage().instance().set(&keys::admin(&env), &admin);
        env.storage().instance().set(&keys::city_list(&env), &Vec::<Symbol>::new(&env));
    }

    /// Register a new city index.
    pub fn add_city(
        env: Env,
        admin: Address,
        city: Symbol,
        name: Symbol,
        country: Symbol,
        base_value: i128,
    ) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let config = CityConfig {
            city: city.clone(),
            name,
            country,
            base_value,
            sources: Vec::new(&env),
            status: Symbol::new(&env, "active"),
        };

        env.storage().persistent().set(&keys::city(&env, &city), &config);

        // Store initial index snapshot
        let snapshot = IndexSnapshot {
            city: city.clone(),
            value: base_value,
            change_24h: 0,
            change_30d: 0,
            source_count: 0,
            timestamp: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&keys::current(&env, &city), &snapshot);

        // Add to city list
        let mut cities: Vec<Symbol> = env.storage().instance().get(&keys::city_list(&env)).unwrap();
        cities.push_back(city);
        env.storage().instance().set(&keys::city_list(&env), &cities);
    }

    /// Add a data source for a city index.
    pub fn add_data_source(
        env: Env,
        admin: Address,
        city: Symbol,
        source_name: Symbol,
        weight: u32,
    ) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let mut config: CityConfig = env.storage().persistent().get(&keys::city(&env, &city)).unwrap();
        let source = DataSource {
            name: source_name,
            weight,
            last_value: 0,
            last_updated: 0,
        };
        config.sources.push_back(source);
        env.storage().persistent().set(&keys::city(&env, &city), &config);
    }

    /// Authorize an oracle address to submit price updates.
    pub fn add_oracle(env: Env, admin: Address, oracle: Address) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        env.storage().persistent().set(&keys::oracle(&env, &oracle), &true);
    }

    /// Update a specific data source value for a city. Recalculates the weighted index.
    pub fn update_index(
        env: Env,
        oracle: Address,
        city: Symbol,
        source_name: Symbol,
        value: i128,
        timestamp: u64,
    ) {
        oracle.require_auth();
        Self::require_oracle(&env, &oracle);

        let mut config: CityConfig = env.storage().persistent().get(&keys::city(&env, &city)).unwrap();

        // Find and update the source
        let mut found = false;
        let mut updated_sources = Vec::new(&env);
        for i in 0..config.sources.len() {
            let mut src = config.sources.get(i).unwrap();
            if src.name == source_name {
                src.last_value = value;
                src.last_updated = timestamp;
                found = true;
            }
            updated_sources.push_back(src);
        }
        assert!(found, "source not found");
        config.sources = updated_sources;

        // Recalculate weighted index
        let new_value = Self::calculate_weighted_index(&env, &config.sources);

        // Get previous snapshot for change calculation
        let prev: IndexSnapshot = env.storage().persistent().get(&keys::current(&env, &city)).unwrap();
        let change_24h = if prev.value > 0 {
            ((new_value - prev.value) * 10000 / prev.value) as i32
        } else {
            0
        };

        let snapshot = IndexSnapshot {
            city: city.clone(),
            value: new_value,
            change_24h,
            change_30d: prev.change_30d, // Updated by off-chain job
            source_count: config.sources.len() as u32,
            timestamp: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&keys::current(&env, &city), &snapshot);
        env.storage().persistent().set(&keys::city(&env, &city), &config);

        // Emit event
        env.events().publish(
            (Symbol::new(&env, "index_updated"), city),
            (new_value, change_24h, snapshot.source_count),
        );
    }

    /// Batch update multiple city/source/value tuples in a single call.
    pub fn batch_update(
        env: Env,
        oracle: Address,
        updates: Vec<(Symbol, Symbol, i128, u64)>,
    ) {
        oracle.require_auth();
        Self::require_oracle(&env, &oracle);

        for i in 0..updates.len() {
            let (city, source, value, ts) = updates.get(i).unwrap();
            Self::update_index(env.clone(), oracle.clone(), city, source, value, ts);
        }
    }

    /// Get the current index snapshot for a city.
    pub fn get_current(env: Env, city: Symbol) -> IndexSnapshot {
        env.storage().persistent().get(&keys::current(&env, &city)).unwrap()
    }

    /// Get the configuration for a city.
    pub fn get_city_config(env: Env, city: Symbol) -> CityConfig {
        env.storage().persistent().get(&keys::city(&env, &city)).unwrap()
    }

    /// Get all registered city codes.
    pub fn get_all_cities(env: Env) -> Vec<Symbol> {
        env.storage().instance().get(&keys::city_list(&env)).unwrap()
    }

    /// Pause a city index (stops oracle updates).
    pub fn pause_city(env: Env, admin: Address, city: Symbol) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        let mut config: CityConfig = env.storage().persistent().get(&keys::city(&env, &city)).unwrap();
        config.status = Symbol::new(&env, "paused");
        env.storage().persistent().set(&keys::city(&env, &city), &config);
    }

    /// Resume a paused city index.
    pub fn resume_city(env: Env, admin: Address, city: Symbol) {
        admin.require_auth();
        Self::require_admin(&env, &admin);
        let mut config: CityConfig = env.storage().persistent().get(&keys::city(&env, &city)).unwrap();
        config.status = Symbol::new(&env, "active");
        env.storage().persistent().set(&keys::city(&env, &city), &config);
    }
}

impl IndexCalculator {
    fn require_admin(env: &Env, addr: &Address) {
        let admin: Address = env.storage().instance().get(&keys::admin(env)).unwrap();
        assert_eq!(*addr, admin, "not admin");
    }

    fn require_oracle(env: &Env, addr: &Address) {
        let is_oracle: bool = env.storage().persistent().get(&keys::oracle(env, addr)).unwrap_or(false);
        assert!(is_oracle, "not authorized oracle");
    }

    fn calculate_weighted_index(env: &Env, sources: &Vec<DataSource>) -> i128 {
        let mut weighted_sum: i128 = 0;
        let mut total_weight: u32 = 0;
        let now = env.ledger().timestamp();

        for i in 0..sources.len() {
            let src = sources.get(i).unwrap();
            // Skip stale sources (older than 7 days)
            if src.last_updated > 0 && (now - src.last_updated) <= SEVEN_DAYS {
                weighted_sum += src.last_value * src.weight as i128;
                total_weight += src.weight;
            }
        }

        if total_weight == 0 {
            return 0;
        }

        weighted_sum / total_weight as i128
    }
}
