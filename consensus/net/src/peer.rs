use libp2p::PeerId;
use std::collections::HashMap;

/// Information about a connected peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub address: Option<String>,
    pub last_seen: u64,
    /// Reputation score: positive is good, negative is bad.
    pub score: i64,
}

/// Tracks connected peers and their reputation.
pub struct PeerManager {
    peers: HashMap<PeerId, PeerInfo>,
    /// Score below which a peer is considered banned.
    ban_threshold: i64,
}

impl PeerManager {
    pub fn new(ban_threshold: i64) -> Self {
        Self {
            peers: HashMap::new(),
            ban_threshold,
        }
    }

    /// Register a new peer or update an existing one's last_seen.
    pub fn add_peer(&mut self, peer_id: PeerId, address: Option<String>, now: u64) {
        self.peers
            .entry(peer_id)
            .and_modify(|info| {
                info.last_seen = now;
                if let Some(ref addr) = address {
                    info.address = Some(addr.clone());
                }
            })
            .or_insert(PeerInfo {
                peer_id,
                address,
                last_seen: now,
                score: 0,
            });
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.remove(peer_id)
    }

    /// Adjust a peer's score by delta. Returns the new score.
    pub fn adjust_score(&mut self, peer_id: &PeerId, delta: i64) -> Option<i64> {
        self.peers.get_mut(peer_id).map(|info| {
            info.score = info.score.saturating_add(delta);
            info.score
        })
    }

    /// Check if a peer is banned (score below threshold).
    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map_or(false, |info| info.score < self.ban_threshold)
    }

    /// Get info for a specific peer.
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Return all connected peer IDs.
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers.keys().copied().collect()
    }

    /// Number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Prune peers not seen since `cutoff` timestamp.
    pub fn prune_stale(&mut self, cutoff: u64) -> Vec<PeerId> {
        let stale: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, info)| info.last_seen < cutoff)
            .map(|(id, _)| *id)
            .collect();
        for id in &stale {
            self.peers.remove(id);
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_peer_id() -> PeerId {
        PeerId::random()
    }

    #[test]
    fn test_add_and_get_peer() {
        let mut pm = PeerManager::new(-100);
        let peer = random_peer_id();
        pm.add_peer(peer, Some("/ip4/127.0.0.1/tcp/9000".into()), 1000);

        let info = pm.get_peer(&peer).expect("peer should exist");
        assert_eq!(info.peer_id, peer);
        assert_eq!(info.score, 0);
        assert_eq!(info.last_seen, 1000);
    }

    #[test]
    fn test_add_peer_updates_last_seen() {
        let mut pm = PeerManager::new(-100);
        let peer = random_peer_id();
        pm.add_peer(peer, None, 1000);
        pm.add_peer(peer, None, 2000);

        let info = pm.get_peer(&peer).unwrap();
        assert_eq!(info.last_seen, 2000);
        assert_eq!(pm.peer_count(), 1);
    }

    #[test]
    fn test_remove_peer() {
        let mut pm = PeerManager::new(-100);
        let peer = random_peer_id();
        pm.add_peer(peer, None, 1000);
        assert_eq!(pm.peer_count(), 1);

        let removed = pm.remove_peer(&peer);
        assert!(removed.is_some());
        assert_eq!(pm.peer_count(), 0);
    }

    #[test]
    fn test_adjust_score() {
        let mut pm = PeerManager::new(-100);
        let peer = random_peer_id();
        pm.add_peer(peer, None, 1000);

        let score = pm.adjust_score(&peer, 10).unwrap();
        assert_eq!(score, 10);

        let score = pm.adjust_score(&peer, -25).unwrap();
        assert_eq!(score, -15);
    }

    #[test]
    fn test_ban_threshold() {
        let mut pm = PeerManager::new(-50);
        let peer = random_peer_id();
        pm.add_peer(peer, None, 1000);

        assert!(!pm.is_banned(&peer));
        pm.adjust_score(&peer, -51);
        assert!(pm.is_banned(&peer));
    }

    #[test]
    fn test_prune_stale() {
        let mut pm = PeerManager::new(-100);
        let p1 = random_peer_id();
        let p2 = random_peer_id();
        let p3 = random_peer_id();

        pm.add_peer(p1, None, 100);
        pm.add_peer(p2, None, 500);
        pm.add_peer(p3, None, 1000);

        let pruned = pm.prune_stale(600);
        assert_eq!(pruned.len(), 2);
        assert!(pruned.contains(&p1));
        assert!(pruned.contains(&p2));
        assert_eq!(pm.peer_count(), 1);
        assert!(pm.get_peer(&p3).is_some());
    }

    #[test]
    fn test_connected_peers() {
        let mut pm = PeerManager::new(-100);
        let p1 = random_peer_id();
        let p2 = random_peer_id();
        pm.add_peer(p1, None, 100);
        pm.add_peer(p2, None, 200);

        let peers = pm.connected_peers();
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&p1));
        assert!(peers.contains(&p2));
    }
}
