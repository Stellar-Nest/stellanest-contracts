#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};
use stellanest_types::PositionMetadata;

#[contract]
pub struct IndexTokenFactory;

#[contractimpl]
impl IndexTokenFactory {
    /// Initialize the factory with an admin.
    pub fn initialize(env: Env, admin: Address) {
        assert!(!env.storage().instance().has(&Symbol::new(&env, "admin")), "already initialized");
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        env.storage().instance().set(&Symbol::new(&env, "city_count"), &0u32);
    }

    /// Create a new Stellar asset for a city index. Returns the token contract address.
    /// The asset code follows the pattern: SRE_{CITY} (e.g., SRE_NYC).
    pub fn create_city_token(
        env: Env,
        admin: Address,
        city: Symbol,
        name: Symbol,
        decimals: u32,
    ) -> Address {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        // In production: deploy a Stellar asset contract via SEP-0049.
        // For scaffolding, store the mapping and emit an event.
        let token_key = Symbol::new(&env, &format!("token_{}", city));

        // Placeholder address — in production this would be the deployed contract
        let placeholder_addr = admin.clone(); // placeholder
        env.storage().persistent().set(&token_key, &placeholder_addr);

        // Track all cities
        let count: u32 = env.storage().instance().get(&Symbol::new(&env, "city_count")).unwrap();
        let list_key = Symbol::new(&env, "city_tokens");
        let mut cities: Vec<Symbol> = env.storage().instance().get(&list_key).unwrap_or(Vec::new(&env));
        cities.push_back(city.clone());
        env.storage().instance().set(&list_key, &cities);
        env.storage().instance().set(&Symbol::new(&env, "city_count"), &(count + 1));

        env.events().publish(
            (Symbol::new(&env, "city_token_created"), city),
            (name, decimals),
        );

        placeholder_addr
    }

    /// Mint index tokens to a user (after depositing collateral).
    pub fn mint_index_tokens(
        env: Env,
        admin: Address,
        city: Symbol,
        to: Address,
        amount: i128,
    ) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        // In production: call the token contract's mint function.
        env.events().publish(
            (Symbol::new(&env, "index_tokens_minted"), city),
            (to, amount),
        );
    }

    /// Burn index tokens (when user closes position).
    pub fn burn_index_tokens(
        env: Env,
        admin: Address,
        city: Symbol,
        from: Address,
        amount: i128,
    ) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        // In production: call the token contract's burn function.
        env.events().publish(
            (Symbol::new(&env, "index_tokens_burned"), city),
            (from, amount),
        );
    }

    /// Mint a position NFT (SEP-0048) representing an open position.
    pub fn mint_position_nft(
        env: Env,
        admin: Address,
        to: Address,
        position_id: u128,
        metadata: PositionMetadata,
    ) -> u128 {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        // In production: deploy or mint via SEP-0048 NFT contract.
        let token_id = position_id; // Use position ID as token ID for simplicity

        let nft_key = Symbol::new(&env, &format!("nft_{}", token_id));
        env.storage().persistent().set(&nft_key, &(to.clone(), metadata.clone()));

        env.events().publish(
            (Symbol::new(&env, "position_nft_minted"), to),
            (token_id, metadata.city, metadata.direction, metadata.leverage),
        );

        token_id
    }

    /// Burn a position NFT (when position is closed or liquidated).
    pub fn burn_position_nft(env: Env, admin: Address, token_id: u128) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let nft_key = Symbol::new(&env, &format!("nft_{}", token_id));
        env.storage().persistent().remove(&nft_key);

        env.events().publish(
            Symbol::new(&env, "position_nft_burned"),
            token_id,
        );
    }

    /// Get the token contract address for a city index.
    pub fn get_city_token(env: Env, city: Symbol) -> Address {
        let token_key = Symbol::new(&env, &format!("token_{}", city));
        env.storage().persistent().get(&token_key).unwrap()
    }

    /// Get all city token mappings.
    pub fn get_all_city_tokens(env: Env) -> Vec<(Symbol, Address)> {
        let list_key = Symbol::new(&env, "city_tokens");
        let cities: Vec<Symbol> = env.storage().instance().get(&list_key).unwrap_or(Vec::new(&env));
        let mut result = Vec::new(&env);
        for i in 0..cities.len() {
            let city = cities.get(i).unwrap();
            let addr = Self::get_city_token(env.clone(), city.clone());
            result.push_back((city, addr));
        }
        result
    }
}

impl IndexTokenFactory {
    fn require_admin(env: &Env, addr: &Address) {
        let admin: Address = env.storage().instance().get(&Symbol::new(env, "admin")).unwrap();
        assert_eq!(*addr, admin, "not admin");
    }
}
