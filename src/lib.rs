mod key;
mod node;
mod protocol;
mod routing;
mod storage;

pub use self::key::Key;
pub use self::node::node_data::NodeData;
pub use self::node::Node;
use serde_derive::{Deserialize, Serialize};

/// The number of bytes in a key.
const KEY_LENGTH: usize = 32;

/// The maximum length of the message in bytes.
const MESSAGE_LENGTH: usize = 8196;

/// The maximum number of k-buckets in the routing table.
const ROUTING_TABLE_SIZE: usize = KEY_LENGTH * 8;

/// The maximum number of entries in a k-bucket.
const REPLICATION_PARAM: usize = 20;

/// The maximum number of active RPCs during `lookup_nodes`.
const CONCURRENCY_PARAM: usize = 3;

/// Request timeout time in milliseconds
const REQUEST_TIMEOUT: u64 = 5000;

/// Key-value pair expiration time in seconds
const KEY_EXPIRATION: u64 = 2000;

/// Bucket refresh interval in seconds
const BUCKET_REFRESH_INTERVAL: u64 = 3600;

// Sample Percentage Of Buckets to Ping.
const SAMPLE_PERCENTAGE_BUCKETS_TO_PING: f64 = 0.3;

/// Sample Percentage Of Nodes in buckets to Ping.
const SAMPLE_PERCENTAGE_NODES_TO_PING: f64 = 0.3;

/// Ping Nodes after every N seconds
const PING_TIME_INTERVAL: u64 = 10;

/// `RETRY_ATTEMPTS` is a constant that specifies the maximum number of retry attempts for any operation
const RETRY_ATTEMPTS: i32 = 3;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum NodeType{
    Bootstrap,
    Farmer,
    Harvester,
    Miner,
    Other(String)
}