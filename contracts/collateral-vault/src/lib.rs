#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

#[contract]
pub struct CollateralVault;

#[contractimpl]
impl CollateralVault {
    /// Initialize the vault with USDC token address and authorized position manager.
    pub fn initialize(env: Env, admin: Address, usdc_address: Address, position_manager: Address) {
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        env.storage().instance().set(&Symbol::new(&env, "usdc"), &usdc_address);
        env.storage().instance().set(&Symbol::new(&env, "pos_mgr"), &position_manager);
        env.storage().instance().set(&Symbol::new(&env, "total_deposited"), &0i128);
        env.storage().instance().set(&Symbol::new(&env, "total_locked"), &0i128);
        env.storage().instance().set(&Symbol::new(&env, "insurance"), &0i128);
    }

    /// Deposit USDC into the vault.
    pub fn deposit(env: Env, user: Address, amount: i128) {
        user.require_auth();
        assert!(amount > 0, "amount must be positive");

        let balance = Self::get_balance_internal(&env, &user);
        env.storage().persistent().set(&Self::bal_key(&env, &user), &(balance + amount));

        let total: i128 = env.storage().instance().get(&Symbol::new(&env, "total_deposited")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "total_deposited"), &(total + amount));

        // In production: transfer USDC from user to vault via token contract
        env.events().publish(
            (Symbol::new(&env, "deposit"), user),
            amount,
        );
    }

    /// Withdraw available (unlocked) USDC from the vault.
    pub fn withdraw(env: Env, user: Address, amount: i128) {
        user.require_auth();
        assert!(amount > 0, "amount must be positive");

        let balance = Self::get_balance_internal(&env, &user);
        assert!(balance >= amount, "insufficient available balance");

        env.storage().persistent().set(&Self::bal_key(&env, &user), &(balance - amount));

        let total: i128 = env.storage().instance().get(&Symbol::new(&env, "total_deposited")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "total_deposited"), &(total - amount));

        // In production: transfer USDC from vault to user
        env.events().publish(
            (Symbol::new(&env, "withdraw"), user),
            amount,
        );
    }

    /// Get available (unlocked) balance for a user.
    pub fn get_balance(env: Env, user: Address) -> i128 {
        Self::get_balance_internal(&env, &user)
    }

    /// Lock collateral for a position. Only callable by the position manager contract.
    pub fn lock_for_position(
        env: Env,
        caller: Address,
        user: Address,
        position_id: u128,
        amount: i128,
    ) {
        Self::require_position_manager(&env, &caller);

        let balance = Self::get_balance_internal(&env, &user);
        assert!(balance >= amount, "insufficient balance to lock");

        // Move from available to locked
        env.storage().persistent().set(&Self::bal_key(&env, &user), &(balance - amount));
        env.storage().persistent().set(&Self::lock_key(&env, position_id), &(user.clone(), amount));

        let total_locked: i128 = env.storage().instance().get(&Symbol::new(&env, "total_locked")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "total_locked"), &(total_locked + amount));

        env.events().publish(
            (Symbol::new(&env, "locked"), user),
            (position_id, amount),
        );
    }

    /// Release collateral from a closed position back to the user.
    pub fn release_from_position(
        env: Env,
        caller: Address,
        position_id: u128,
        user: Address,
        amount: i128,
    ) {
        Self::require_position_manager(&env, &caller);

        let (locked_user, locked_amount): (Address, i128) = env.storage().persistent().get(&Self::lock_key(&env, position_id)).unwrap();
        assert_eq!(locked_user, user, "user mismatch");
        assert!(amount <= locked_amount, "release exceeds locked amount");

        // Remove lock
        env.storage().persistent().remove(&Self::lock_key(&env, position_id));

        // Return to available balance
        let balance = Self::get_balance_internal(&env, &user);
        env.storage().persistent().set(&Self::bal_key(&env, &user), &(balance + amount));

        let total_locked: i128 = env.storage().instance().get(&Symbol::new(&env, "total_locked")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "total_locked"), &(total_locked - locked_amount));

        env.events().publish(
            (Symbol::new(&env, "released"), user),
            (position_id, amount),
        );
    }

    /// Seize collateral during liquidation. Splits between insurance fund and user.
    pub fn seize_collateral(
        env: Env,
        caller: Address,
        position_id: u128,
        amount: i128,
        to_insurance: i128,
        to_user: i128,
    ) {
        Self::require_position_manager(&env, &caller);

        let (user, locked_amount): (Address, i128) = env.storage().persistent().get(&Self::lock_key(&env, position_id)).unwrap();
        assert!(amount <= locked_amount, "seize exceeds locked");
        assert!(to_insurance + to_user == locked_amount, "split must equal locked amount");
        assert!(to_insurance >= 0, "insurance amount must be non-negative");
        assert!(to_user >= 0, "user amount must be non-negative");

        // Remove lock
        env.storage().persistent().remove(&Self::lock_key(&env, position_id));

        // Add penalty to insurance fund
        let insurance: i128 = env.storage().instance().get(&Symbol::new(&env, "insurance")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "insurance"), &(insurance + to_insurance));

        // Return remainder to user
        let balance = Self::get_balance_internal(&env, &user);
        env.storage().persistent().set(&Self::bal_key(&env, &user), &(balance + to_user));

        let total_locked: i128 = env.storage().instance().get(&Symbol::new(&env, "total_locked")).unwrap();
        env.storage().instance().set(&Symbol::new(&env, "total_locked"), &(total_locked - locked_amount));

        env.events().publish(
            (Symbol::new(&env, "seized"), user),
            (position_id, amount, to_insurance, to_user),
        );
    }

    /// Get the insurance fund balance.
    pub fn get_insurance_balance(env: Env) -> i128 {
        env.storage().instance().get(&Symbol::new(&env, "insurance")).unwrap_or(0)
    }

    /// Admin withdrawal from insurance fund (e.g., for rebalancing).
    pub fn withdraw_insurance(env: Env, admin: Address, amount: i128, to: Address) {
        admin.require_auth();
        Self::require_admin(&env, &admin);

        let insurance: i128 = env.storage().instance().get(&Symbol::new(&env, "insurance")).unwrap();
        assert!(amount <= insurance, "insufficient insurance fund");

        env.storage().instance().set(&Symbol::new(&env, "insurance"), &(insurance - amount));

        // In production: transfer USDC to `to`
        env.events().publish(
            (Symbol::new(&env, "insurance_withdraw"), to),
            amount,
        );
    }

    /// Get total USDC deposited in the vault.
    pub fn get_total_deposited(env: Env) -> i128 {
        env.storage().instance().get(&Symbol::new(&env, "total_deposited")).unwrap_or(0)
    }

    /// Get total USDC locked in positions.
    pub fn get_total_locked(env: Env) -> i128 {
        env.storage().instance().get(&Symbol::new(&env, "total_locked")).unwrap_or(0)
    }
}

impl CollateralVault {
    fn bal_key(env: &Env, user: &Address) -> Symbol {
        Symbol::new(env, &format!("bal_{}", user))
    }

    fn lock_key(env: &Env, pos_id: u128) -> Symbol {
        Symbol::new(env, &format!("lock_{}", pos_id))
    }

    fn get_balance_internal(env: &Env, user: &Address) -> i128 {
        env.storage().persistent().get(&Self::bal_key(env, user)).unwrap_or(0)
    }

    fn require_admin(env: &Env, addr: &Address) {
        let admin: Address = env.storage().instance().get(&Symbol::new(env, "admin")).unwrap();
        assert_eq!(*addr, admin, "not admin");
    }

    fn require_position_manager(env: &Env, addr: &Address) {
        let pm: Address = env.storage().instance().get(&Symbol::new(env, "pos_mgr")).unwrap();
        assert_eq!(*addr, pm, "not position manager");
    }
}
