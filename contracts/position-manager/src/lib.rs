#![no_std]
use soroban_sdk::{contract, contractclient, contractimpl, Address, Env, Symbol, Vec};
use stellanest_types::{
    CloseResult, Direction, LiquidateResult, Position, PositionStatus,
};

/// Client trait for cross-contract calls to the CollateralVault contract.
#[contractclient(name = "CollateralVaultClient")]
pub trait CollateralVault {
    fn lock_for_position(env: Env, caller: Address, user: Address, position_id: u128, amount: i128);
    fn release_from_position(env: Env, caller: Address, position_id: u128, user: Address, amount: i128);
    fn seize_collateral(env: Env, caller: Address, position_id: u128, amount: i128, to_insurance: i128, to_user: i128);
}

/// Health factor thresholds (basis points)
const LIQUIDATION_THRESHOLD: i128 = 12000; // 1.2x
const WARNING_THRESHOLD: i128 = 13000;     // 1.3x
const MAINTENANCE_RATIO: i128 = 8000;      // 0.8x (80%)
const LIQUIDATION_PENALTY: i128 = 500;     // 5% (500 bps)
const BP_DENOMINATOR: i128 = 10000;

#[contract]
pub struct PositionManager;

#[contractimpl]
impl PositionManager {
    /// Initialize the contract with references to other contracts.
    pub fn initialize(
        env: Env,
        admin: Address,
        vault_address: Address,
        oracle_address: Address,
        index_address: Address,
    ) {
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        env.storage().instance().set(&Symbol::new(&env, "vault"), &vault_address);
        env.storage().instance().set(&Symbol::new(&env, "oracle"), &oracle_address);
        env.storage().instance().set(&Symbol::new(&env, "index"), &index_address);
        env.storage().instance().set(&Symbol::new(&env, "next_id"), &1u128);
    }

    /// Open a long position on a city index.
    pub fn open_long(
        env: Env,
        user: Address,
        city: Symbol,
        collateral: i128,
        leverage: u32,
    ) -> Position {
        user.require_auth();
        assert!(collateral > 0, "collateral must be positive");
        assert!(leverage >= 1 && leverage <= 10, "leverage must be 1-10");

        let current_price = Self::get_index_price(&env, &city);
        let size = collateral * leverage as i128;

        // Liquidation price for long: entry * (1 - 1/leverage * maintenance)
        let liq_price = current_price * (BP_DENOMINATOR - BP_DENOMINATOR / leverage as i128 * MAINTENANCE_RATIO / BP_DENOMINATOR) / BP_DENOMINATOR;

        let position = Self::create_position(
            &env,
            user,
            city,
            Direction::Long,
            leverage,
            current_price,
            collateral,
            size,
            liq_price,
        );

        // Lock collateral in vault
        Self::lock_collateral(&env, &position.user, position.id, collateral);

        env.events().publish(
            (Symbol::new(&env, "position_opened"), position.city.clone()),
            (position.id, position.user.clone(), position.direction, position.size),
        );

        position
    }

    /// Open a short position on a city index.
    pub fn open_short(
        env: Env,
        user: Address,
        city: Symbol,
        collateral: i128,
        leverage: u32,
    ) -> Position {
        user.require_auth();
        assert!(collateral > 0, "collateral must be positive");
        assert!(leverage >= 1 && leverage <= 10, "leverage must be 1-10");

        let current_price = Self::get_index_price(&env, &city);
        let size = collateral * leverage as i128;

        // Liquidation price for short: entry * (1 + 1/leverage * maintenance)
        let liq_price = current_price * (BP_DENOMINATOR + BP_DENOMINATOR / leverage as i128 * MAINTENANCE_RATIO / BP_DENOMINATOR) / BP_DENOMINATOR;

        let position = Self::create_position(
            &env,
            user,
            city,
            Direction::Short,
            leverage,
            current_price,
            collateral,
            size,
            liq_price,
        );

        Self::lock_collateral(&env, &position.user, position.id, collateral);

        env.events().publish(
            (Symbol::new(&env, "position_opened"), position.city.clone()),
            (position.id, position.user.clone(), position.direction, position.size),
        );

        position
    }

    /// Close an open position and settle P&L.
    pub fn close_position(env: Env, user: Address, position_id: u128) -> CloseResult {
        user.require_auth();

        let mut position: Position = env.storage().persistent().get(&Self::pos_key(&env, position_id)).unwrap();
        assert_eq!(position.user, user, "not position owner");
        assert_eq!(position.status, PositionStatus::Open, "position not open");

        let exit_price = Self::get_index_price(&env, &position.city);

        // Calculate P&L
        let pnl = match position.direction {
            Direction::Long => {
                position.size * (exit_price - position.entry_price) / position.entry_price
            }
            Direction::Short => {
                position.size * (position.entry_price - exit_price) / position.entry_price
            }
        };

        let collateral_returned = position.collateral + pnl;
        assert!(collateral_returned >= 0, "negative collateral — use liquidation");

        // Update position
        position.status = PositionStatus::Closed;
        env.storage().persistent().set(&Self::pos_key(&env, position_id), &position);

        // Release collateral + P&L from vault
        Self::release_collateral(&env, position_id, &position.user, collateral_returned);

        let result = CloseResult {
            position_id,
            entry_price: position.entry_price,
            exit_price,
            pnl,
            collateral_returned,
        };

        env.events().publish(
            (Symbol::new(&env, "position_closed"), position.city),
            (position_id, pnl, collateral_returned),
        );

        result
    }

    /// Liquidate an undercollateralized position. Called by keeper bots.
    pub fn liquidate(env: Env, liquidator: Address, position_id: u128) -> LiquidateResult {
        liquidator.require_auth();

        let mut position: Position = env.storage().persistent().get(&Self::pos_key(&env, position_id)).unwrap();
        assert_eq!(position.status, PositionStatus::Open, "position not open");

        let health = Self::calculate_health(&env, &position);
        assert!(health < LIQUIDATION_THRESHOLD, "position is healthy");

        let current_price = Self::get_index_price(&env, &position.city);

        // Calculate penalty
        let penalty = position.collateral * LIQUIDATION_PENALTY / BP_DENOMINATOR;
        let collateral_seized = position.collateral;
        let to_insurance = penalty;
        let to_user = collateral_seized - penalty;

        // Update position
        position.status = PositionStatus::Liquidated;
        env.storage().persistent().set(&Self::pos_key(&env, position_id), &position);

        // Seize collateral via vault
        Self::seize_collateral(&env, position_id, collateral_seized, to_insurance, to_user, &position.user);

        let result = LiquidateResult {
            position_id,
            collateral_seized,
            penalty,
            to_insurance_fund: to_insurance,
        };

        env.events().publish(
            (Symbol::new(&env, "liquidation"), position.city),
            (position_id, collateral_seized, penalty),
        );

        result
    }

    /// Check the health factor of a position.
    pub fn check_health(env: Env, position_id: u128) -> i128 {
        let position: Position = env.storage().persistent().get(&Self::pos_key(&env, position_id)).unwrap();
        Self::calculate_health(&env, &position)
    }

    /// Batch check health factors for multiple positions.
    pub fn batch_check_health(env: Env, position_ids: Vec<u128>) -> Vec<(u128, i128)> {
        let mut results = Vec::new(&env);
        for i in 0..position_ids.len() {
            let pid = position_ids.get(i).unwrap();
            let hf = Self::check_health(env.clone(), pid);
            results.push_back((pid, hf));
        }
        results
    }

    /// Get a position by ID.
    pub fn get_position(env: Env, position_id: u128) -> Position {
        env.storage().persistent().get(&Self::pos_key(&env, position_id)).unwrap()
    }

    /// Get all position IDs for a user.
    pub fn get_user_positions(env: Env, user: Address) -> Vec<u128> {
        let key = Symbol::new(&env, &format!("user_{}", user));
        env.storage().persistent().get(&key).unwrap_or(Vec::new(&env))
    }

    /// Get total open interest for a city.
    pub fn get_total_open_interest(env: Env, city: Symbol) -> i128 {
        let key = Symbol::new(&env, &format!("oi_{}", city));
        env.storage().persistent().get(&key).unwrap_or(0)
    }
}

impl PositionManager {
    fn pos_key(env: &Env, id: u128) -> Symbol {
        Symbol::new(env, &format!("pos_{}", id))
    }

    fn get_index_price(env: &Env, city: &Symbol) -> i128 {
        // In production, this calls the oracle contract.
        // For now, read from storage (set by oracle during testing).
        let oracle_key = Symbol::new(env, &format!("price_{}", city));
        env.storage().persistent().get(&oracle_key).unwrap_or(0)
    }

    fn create_position(
        env: &Env,
        user: Address,
        city: Symbol,
        direction: Direction,
        leverage: u32,
        entry_price: i128,
        collateral: i128,
        size: i128,
        liquidation_price: i128,
    ) -> Position {
        let next_id: u128 = env.storage().instance().get(&Symbol::new(env, "next_id")).unwrap();
        env.storage().instance().set(&Symbol::new(env, "next_id"), &(next_id + 1));

        let position = Position {
            id: next_id,
            user: user.clone(),
            city,
            direction,
            leverage,
            entry_price,
            collateral,
            size,
            liquidation_price,
            status: PositionStatus::Open,
            opened_at: env.ledger().timestamp(),
            funding_paid: 0,
        };

        env.storage().persistent().set(&Self::pos_key(env, next_id), &position);

        // Add to user's position list
        let user_key = Symbol::new(env, &format!("user_{}", user));
        let mut user_positions: Vec<u128> = env.storage().persistent().get(&user_key).unwrap_or(Vec::new(env));
        user_positions.push_back(next_id);
        env.storage().persistent().set(&user_key, &user_positions);

        position
    }

    fn calculate_health(env: &Env, position: &Position) -> i128 {
        let current_price = Self::get_index_price(env, &position.city);
        if current_price == 0 || position.collateral == 0 {
            return 0;
        }

        let pnl = match position.direction {
            Direction::Long => {
                position.size * (current_price - position.entry_price) / position.entry_price
            }
            Direction::Short => {
                position.size * (position.entry_price - current_price) / position.entry_price
            }
        };

        let equity = position.collateral + pnl;
        let maintenance_required = position.collateral * MAINTENANCE_RATIO / BP_DENOMINATOR;

        if maintenance_required == 0 {
            return i128::MAX;
        }

        equity * BP_DENOMINATOR / maintenance_required
    }

    fn lock_collateral(env: &Env, user: &Address, position_id: u128, amount: i128) {
        let vault_addr: Address = env.storage().instance().get(&Symbol::new(env, "vault")).unwrap();
        let vault_client = CollateralVaultClient::new(env, &vault_addr);
        let contract_addr = env.current_contract_address();
        vault_client.lock_for_position(&contract_addr, user, &position_id, &amount);

        env.events().publish(
            (Symbol::new(env, "collateral_locked"), user.clone()),
            (position_id, amount),
        );
    }

    fn release_collateral(env: &Env, position_id: u128, user: &Address, amount: i128) {
        let vault_addr: Address = env.storage().instance().get(&Symbol::new(env, "vault")).unwrap();
        let vault_client = CollateralVaultClient::new(env, &vault_addr);
        let contract_addr = env.current_contract_address();
        vault_client.release_from_position(&contract_addr, &position_id, user, &amount);

        env.events().publish(
            (Symbol::new(env, "collateral_released"), user.clone()),
            (position_id, amount),
        );
    }

    fn seize_collateral(
        env: &Env,
        position_id: u128,
        total: i128,
        to_insurance: i128,
        to_user: i128,
        user: &Address,
    ) {
        let vault_addr: Address = env.storage().instance().get(&Symbol::new(env, "vault")).unwrap();
        let vault_client = CollateralVaultClient::new(env, &vault_addr);
        let contract_addr = env.current_contract_address();
        vault_client.seize_collateral(&contract_addr, &position_id, &total, &to_insurance, &to_user);

        env.events().publish(
            (Symbol::new(env, "collateral_seized"), user.clone()),
            (position_id, total, to_insurance, to_user),
        );
    }
}
