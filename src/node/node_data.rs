use crate::key::Key;
use serde_derive::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Debug;
use std::net::SocketAddr;

/// A struct that contains the address and id of a node.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct NodeData<D>
where
    D: Debug + Clone + Eq + PartialEq + serde::Serialize,
{
    /// The id of the node.
    pub id: Key,

    /// The address of the node.
    pub addr: SocketAddr,

    /// Address the node uses for gossiping.
    pub udp_gossip_addr: SocketAddr,

    pub metadata: Option<D>,
}

impl<D> NodeData<D>
where
    D: Debug + Clone + Eq + PartialEq + serde::Serialize,
{
    pub fn new(
        id: Key,
        addr: SocketAddr,
        udp_gossip_addr: SocketAddr,
        metadata: Option<D>,
    ) -> Self {
        NodeData {
            id,
            addr,
            udp_gossip_addr,
            metadata,
        }
    }
}

/// A struct that contains a `NodeData` and a distance.
#[derive(Eq, Clone, Debug)]
pub struct NodeDataDistancePair<D>(pub NodeData<D>, pub Key)
where
    D: Debug + Clone + Eq + PartialEq + serde::Serialize;

impl<D> PartialEq for NodeDataDistancePair<D>
where
    D: Debug + Clone + Eq + PartialEq + serde::Serialize,
{
    fn eq(&self, other: &NodeDataDistancePair<D>) -> bool {
        self.0.eq(&other.0)
    }
}

impl<D> PartialOrd for NodeDataDistancePair<D>
where
    D: Debug + Clone + Eq + PartialEq + serde::Serialize,
{
    fn partial_cmp(&self, other: &NodeDataDistancePair<D>) -> Option<Ordering> {
        Some(other.1.cmp(&self.1))
    }
}

impl<D> Ord for NodeDataDistancePair<D>
where
    D: Debug + Clone + Eq + PartialEq + serde::Serialize,
{
    fn cmp(&self, other: &NodeDataDistancePair<D>) -> Ordering {
        other.1.cmp(&self.1)
    }
}
