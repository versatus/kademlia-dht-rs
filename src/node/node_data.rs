use crate::key::Key;
use serde_derive::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Debug;
use std::net::{SocketAddr, UdpSocket};

/// A struct that contains the address and id of a node.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct NodeData {
    /// The id of the node.
    pub id: Key,

    /// The address of the node.
    pub addr: SocketAddr,

    /// Address the node uses for gossiping.
    pub udp_gossip_addr: SocketAddr,
}

impl NodeData {
    pub fn new(id: Key, addr: SocketAddr, udp_gossip_addr: SocketAddr) -> Self {
        NodeData {
            id,
            addr,
            udp_gossip_addr,
        }
    }
}

/// A struct that contains a `NodeData` and a distance.
#[derive(Eq, Clone, Debug)]
pub struct NodeDataDistancePair(pub NodeData, pub Key);

impl PartialEq for NodeDataDistancePair {
    fn eq(&self, other: &NodeDataDistancePair) -> bool {
        self.0.eq(&other.0)
    }
}

impl PartialOrd for NodeDataDistancePair {
    fn partial_cmp(&self, other: &NodeDataDistancePair) -> Option<Ordering> {
        Some(other.1.cmp(&self.1))
    }
}

impl Ord for NodeDataDistancePair {
    fn cmp(&self, other: &NodeDataDistancePair) -> Ordering {
        other.1.cmp(&self.1)
    }
}
