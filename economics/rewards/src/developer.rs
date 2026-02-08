use std::collections::HashMap;

use crate::types::*;

/// Developer rewards system: tracks contract deployments and distributes
/// accumulated fees to contract deployers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DeveloperRewards {
    /// Registry of deployed contracts keyed by contract address.
    registry: HashMap<ContractAddress, ContractRegistry>,
    /// Pending (undistributed) rewards per contract.
    pending: HashMap<ContractAddress, u64>,
    /// Total amount distributed across all time.
    total_distributed: u64,
    /// Current block height for distribution events.
    current_height: u64,
}

impl DeveloperRewards {
    /// Create a new empty developer rewards system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the current block height.
    pub fn set_height(&mut self, height: u64) {
        self.current_height = height;
    }

    /// Register a newly deployed contract.
    pub fn register_contract(
        &mut self,
        contract_address: ContractAddress,
        deployer: PublicKey,
        height: u64,
    ) -> Result<(), RewardsError> {
        if self.registry.contains_key(&contract_address) {
            return Err(RewardsError::AlreadyRegistered(contract_address));
        }

        self.registry.insert(
            contract_address,
            ContractRegistry {
                contract_address,
                deployer,
                deploy_height: height,
                total_fees_earned: 0,
            },
        );

        Ok(())
    }

    /// Record a fee earned by a contract. The fee is accumulated until
    /// `distribute_rewards()` is called.
    pub fn record_fee(
        &mut self,
        contract_address: ContractAddress,
        fee_amount: u64,
    ) -> Result<(), RewardsError> {
        if fee_amount == 0 {
            return Err(RewardsError::ZeroFeeAmount);
        }

        let entry = self
            .registry
            .get_mut(&contract_address)
            .ok_or(RewardsError::ContractNotFound(contract_address))?;

        entry.total_fees_earned = entry
            .total_fees_earned
            .checked_add(fee_amount)
            .ok_or(RewardsError::Overflow)?;

        *self.pending.entry(contract_address).or_insert(0) = self
            .pending
            .get(&contract_address)
            .unwrap_or(&0)
            .checked_add(fee_amount)
            .ok_or(RewardsError::Overflow)?;

        Ok(())
    }

    /// Look up the deployer of a contract.
    pub fn get_developer(&self, contract_address: &ContractAddress) -> Option<PublicKey> {
        self.registry.get(contract_address).map(|e| e.deployer)
    }

    /// Get the total accumulated (but not yet distributed) rewards for a developer.
    pub fn get_accumulated_rewards(&self, developer: &PublicKey) -> u64 {
        self.pending
            .iter()
            .filter_map(|(addr, amount)| {
                self.registry
                    .get(addr)
                    .filter(|entry| &entry.deployer == developer)
                    .map(|_| *amount)
            })
            .sum()
    }

    /// Distribute all pending rewards, returning a list of reward events.
    /// After distribution, pending balances are zeroed out.
    pub fn distribute_rewards(&mut self) -> Vec<RewardEvent> {
        let mut events = Vec::new();

        let pending: Vec<(ContractAddress, u64)> = self
            .pending
            .drain()
            .filter(|(_, amount)| *amount > 0)
            .collect();

        for (contract_address, amount) in pending {
            if let Some(entry) = self.registry.get(&contract_address) {
                events.push(RewardEvent {
                    contract: contract_address,
                    developer: entry.deployer,
                    amount,
                    height: self.current_height,
                });
                self.total_distributed = self.total_distributed.saturating_add(amount);
            }
        }

        events
    }

    /// Get the total amount distributed across all time.
    pub fn total_distributed(&self) -> u64 {
        self.total_distributed
    }

    /// Get the contract registry entry.
    pub fn get_contract(&self, address: &ContractAddress) -> Option<&ContractRegistry> {
        self.registry.get(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> ContractAddress {
        let mut a = [0u8; 32];
        a[0] = n;
        a
    }

    fn dev(n: u8) -> PublicKey {
        let mut k = [0u8; 32];
        k[31] = n;
        k
    }

    #[test]
    fn test_register_contract() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();

        let entry = rewards.get_contract(&addr(1)).unwrap();
        assert_eq!(entry.deployer, dev(1));
        assert_eq!(entry.deploy_height, 100);
        assert_eq!(entry.total_fees_earned, 0);
    }

    #[test]
    fn test_register_duplicate() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();
        let result = rewards.register_contract(addr(1), dev(2), 200);
        assert!(matches!(result, Err(RewardsError::AlreadyRegistered(_))));
    }

    #[test]
    fn test_record_fee() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();
        rewards.record_fee(addr(1), 5000).unwrap();

        let entry = rewards.get_contract(&addr(1)).unwrap();
        assert_eq!(entry.total_fees_earned, 5000);
    }

    #[test]
    fn test_record_fee_zero() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();
        let result = rewards.record_fee(addr(1), 0);
        assert_eq!(result, Err(RewardsError::ZeroFeeAmount));
    }

    #[test]
    fn test_record_fee_unknown_contract() {
        let mut rewards = DeveloperRewards::new();
        let result = rewards.record_fee(addr(99), 1000);
        assert!(matches!(result, Err(RewardsError::ContractNotFound(_))));
    }

    #[test]
    fn test_get_developer() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();

        assert_eq!(rewards.get_developer(&addr(1)), Some(dev(1)));
        assert_eq!(rewards.get_developer(&addr(99)), None);
    }

    #[test]
    fn test_get_accumulated_rewards() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();
        rewards.register_contract(addr(2), dev(1), 200).unwrap();
        rewards.register_contract(addr(3), dev(2), 300).unwrap();

        rewards.record_fee(addr(1), 1000).unwrap();
        rewards.record_fee(addr(2), 2000).unwrap();
        rewards.record_fee(addr(3), 3000).unwrap();

        // Dev 1 has contracts 1 and 2.
        assert_eq!(rewards.get_accumulated_rewards(&dev(1)), 3000);
        // Dev 2 has contract 3.
        assert_eq!(rewards.get_accumulated_rewards(&dev(2)), 3000);
        // Unknown dev.
        assert_eq!(rewards.get_accumulated_rewards(&dev(99)), 0);
    }

    #[test]
    fn test_distribute_rewards() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();
        rewards.register_contract(addr(2), dev(2), 200).unwrap();

        rewards.record_fee(addr(1), 1000).unwrap();
        rewards.record_fee(addr(2), 2000).unwrap();

        rewards.set_height(500);
        let events = rewards.distribute_rewards();

        assert_eq!(events.len(), 2);

        let e1 = events.iter().find(|e| e.contract == addr(1)).unwrap();
        assert_eq!(e1.developer, dev(1));
        assert_eq!(e1.amount, 1000);
        assert_eq!(e1.height, 500);

        let e2 = events.iter().find(|e| e.contract == addr(2)).unwrap();
        assert_eq!(e2.developer, dev(2));
        assert_eq!(e2.amount, 2000);
        assert_eq!(e2.height, 500);

        assert_eq!(rewards.total_distributed(), 3000);
    }

    #[test]
    fn test_distribute_clears_pending() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();
        rewards.record_fee(addr(1), 1000).unwrap();

        rewards.distribute_rewards();
        assert_eq!(rewards.get_accumulated_rewards(&dev(1)), 0);

        // Distributing again yields no events.
        let events = rewards.distribute_rewards();
        assert!(events.is_empty());
    }

    #[test]
    fn test_multiple_fee_records() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();

        rewards.record_fee(addr(1), 100).unwrap();
        rewards.record_fee(addr(1), 200).unwrap();
        rewards.record_fee(addr(1), 300).unwrap();

        assert_eq!(rewards.get_accumulated_rewards(&dev(1)), 600);

        let entry = rewards.get_contract(&addr(1)).unwrap();
        assert_eq!(entry.total_fees_earned, 600);
    }

    #[test]
    fn test_total_distributed_accumulates() {
        let mut rewards = DeveloperRewards::new();
        rewards.register_contract(addr(1), dev(1), 100).unwrap();

        rewards.record_fee(addr(1), 1000).unwrap();
        rewards.distribute_rewards();
        assert_eq!(rewards.total_distributed(), 1000);

        rewards.record_fee(addr(1), 500).unwrap();
        rewards.distribute_rewards();
        assert_eq!(rewards.total_distributed(), 1500);
    }
}
