pub mod node_data;

use crate::key::Key;
use crate::node::node_data::{NodeData, NodeDataDistancePair};
use crate::protocol::{Message, Protocol, Request, RequestPayload, Response, ResponsePayload};
use crate::routing::{RoutingBucket, RoutingTable};
use crate::storage::Storage;
use crate::{
    BUCKET_REFRESH_INTERVAL, CONCURRENCY_PARAM, KEY_LENGTH, PING_TIME_INTERVAL, REPLICATION_PARAM,
    REQUEST_TIMEOUT, RETRY_ATTEMPTS, SAMPLE_PERCENTAGE_BUCKETS_TO_PING,
    SAMPLE_PERCENTAGE_NODES_TO_PING,
};
use rand::seq::SliceRandom;
use sha3::{Digest, Sha3_256};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt::Debug;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::{cmp, io};
use tracing::{debug, error, info, warn};

/// A node in the Kademlia DHT.
#[derive(Debug, Clone)]
pub struct Node<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    node_data: Arc<NodeData<D>>,
    routing_table: Arc<Mutex<RoutingTable<D>>>,
    storage: Arc<Mutex<Storage>>,
    pending_requests: Arc<Mutex<HashMap<Key, Sender<Response<D>>>>>,
    protocol: Arc<Protocol<D>>,
    is_active: Arc<AtomicBool>,
    // request_timeout: Option<u64>,
}

impl<D> Node<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    /// Constructs a new `Node` on a specific ip and port, and bootstraps the node with an existing
    /// node if `bootstrap` is not `None`.
    pub fn new(
        id: Option<Key>,
        addr: SocketAddr,
        udp_gossip_addr: SocketAddr,
        bootstrap: Option<NodeData<D>>,
        metadata: Option<D>,
    ) -> Result<Self, io::Error> {
        let socket = UdpSocket::bind(addr).map_err(|err| {
            error!("Error: could not bind to address: {}", err);
            err
        })?;

        let addr = socket.local_addr().map_err(|err| {
            error!(
                "Error occurred while fetching local addr from socket :{}",
                err
            );
            err
        })?;

        let node_data = Arc::new(NodeData {
            id: id.unwrap_or_else(Key::rand),
            addr,
            udp_gossip_addr,
            metadata,
        });

        let mut routing_table = RoutingTable::new(Arc::clone(&node_data));
        let (message_tx, message_rx) = channel();
        let protocol = Protocol::new(socket, message_tx);

        // directly use update_node as update_routing_table is async
        if let Some(bootstrap_data) = bootstrap {
            routing_table.update_node(bootstrap_data);
        }

        let mut ret = Node {
            node_data,
            routing_table: Arc::new(Mutex::new(routing_table)),
            storage: Arc::new(Mutex::new(Storage::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            protocol: Arc::new(protocol),
            is_active: Arc::new(AtomicBool::new(true)),
        };

        ret.start_message_handler(message_rx);
        ret.start_bucket_refresher();
        ret.bootstrap_routing_table();
        ret.check_nodes_liveness();

        Ok(ret)
    }

    pub fn routing_table(&self) -> Result<RoutingTable<D>, String> {
        self.routing_table
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| "Error: Failed to acquire lock".to_string())
    }

    fn clone_into_array<A, T>(slice: &[T]) -> A
    where
        A: Sized + Default + AsMut<[T]>,
        T: Clone,
    {
        let mut a = Default::default();
        <A as AsMut<[T]>>::as_mut(&mut a).clone_from_slice(slice);
        a
    }

    pub fn get_key(key: &str) -> Key {
        let mut hasher = Sha3_256::default();
        hasher.input(key.as_bytes());
        Key(Node::clone_into_array(hasher.result().as_slice()))
    }

    fn check_nodes_liveness(&self) {
        let mut node = self.clone();

        thread::spawn(move || loop {
            let routing_table = match node.routing_table.lock() {
                Ok(table) => table.clone(),
                Err(err) => {
                    error!("Failed to obtain lock on routing table {}", err);
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            let sample_size =
                (routing_table.buckets.len() as f64 * SAMPLE_PERCENTAGE_BUCKETS_TO_PING) as usize;

            let sampled_buckets: Vec<RoutingBucket> = routing_table
                .buckets
                .choose_multiple(&mut rand::thread_rng(), sample_size)
                .cloned()
                .collect();

            for bucket in sampled_buckets {
                let nodes = bucket.nodes;
                let nodes_sample_size =
                    (nodes.len() as f64 * SAMPLE_PERCENTAGE_NODES_TO_PING) as usize;

                let sampled_nodes: Vec<NodeData> = nodes
                    .choose_multiple(&mut rand::thread_rng(), nodes_sample_size)
                    .cloned()
                    .collect();

                for request in sampled_nodes {
                    node.rpc_ping(&request);
                }
            }

            drop(routing_table);
            thread::sleep(Duration::from_secs(PING_TIME_INTERVAL));
        });
    }

    /// Starts a thread that listens to responses.
    fn start_message_handler(&self, rx: Receiver<Message<D>>) {
        let mut node = self.clone();
        thread::spawn(move || {
            for request in rx.iter() {
                match request {
                    Message::Request(request) => node.handle_request(&request),
                    Message::Response(response) => node.handle_response(&response),
                    Message::Kill => {
                        node.is_active.store(false, Ordering::Release);
                        debug!("{} - Killed message handler", node.node_data.addr);
                        break;
                    }
                }
            }
        });
    }

    /// Starts a thread that refreshes stale routing buckets.
    fn start_bucket_refresher(&self) {
        let mut node = self.clone();

        thread::spawn(move || {
            thread::sleep(Duration::from_secs(BUCKET_REFRESH_INTERVAL));

            while node.is_active.load(Ordering::Acquire) {
                let stale_indexes = {
                    let routing_table = match node.routing_table.lock() {
                        Ok(routing_table) => routing_table,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    routing_table.get_stale_indexes()
                };

                for index in stale_indexes {
                    node.lookup_nodes(&Key::rand_in_range(index), true);
                }
                thread::sleep(Duration::from_secs(BUCKET_REFRESH_INTERVAL));
            }
            warn!("{} - Killed bucket refresher", node.node_data.addr);
        });
    }

    /// Bootstraps the routing table using an existing node. The node first looks up its id to
    /// identify the closest nodes to it. Then it refreshes all routing buckets by looking up a
    /// random key in the buckets' range.
    fn bootstrap_routing_table(&mut self) {
        let target_key = self.node_data.id;
        self.lookup_nodes(&target_key, true);
        let bucket_size = self
            .routing_table
            .lock()
            .map_or(0, |routing_table| routing_table.size());

        for i in 0..bucket_size {
            self.lookup_nodes(&Key::rand_in_range(i), true);
        }
    }

    /// Upserts the routing table. If the node cannot be inserted into the routing table, it
    /// removes and pings the least recently seen node. If the least recently seen node responds,
    /// it will be readded into the routing table, and the current node will be ignored.
    fn update_routing_table(&mut self, node_data: NodeData<D>) {
        debug!("{} updating {}", self.node_data.addr, node_data.addr);
        let mut node = self.clone();
        thread::spawn(move || {
            let lrs_node_opt = {
                let mut routing_table = match node.routing_table.lock() {
                    Ok(routing_table) => routing_table,
                    Err(poisoned) => poisoned.into_inner(),
                };
                if !routing_table.update_node(node_data.clone()) {
                    routing_table.remove_lrs(&node_data.id)
                } else {
                    None
                }
            };

            // Ping the lrs node and move to front of bucket if active
            if let Some(lrs_node) = lrs_node_opt {
                node.rpc_ping(&lrs_node);
                let mut routing_table = match node.routing_table.lock() {
                    Ok(routing_table) => routing_table,
                    Err(poisoned) => poisoned.into_inner(),
                };
                routing_table.update_node(node_data);
            }
        });
    }

    /// Handles a request RPC.
    fn handle_request(&mut self, request: &Request<D>) {
        info!(
            "{} - Receiving request from {} {:#?}",
            self.node_data.addr, request.sender.addr, request.payload,
        );
        self.clone().update_routing_table(request.sender.clone());
        let receiver = (*self.node_data).clone();
        let payload = match request.payload.clone() {
            RequestPayload::Ping => ResponsePayload::Pong,
            RequestPayload::Store(key, value) => {
                if let Ok(mut storage_lock) = self.storage.lock() {
                    storage_lock.insert(key, value);
                }
                ResponsePayload::Pong
            }
            RequestPayload::FindNode(key) => {
                let mut attempt = 0;
                loop {
                    if let Ok(routing_table_lock) = self.routing_table.lock() {
                        let closest_nodes =
                            routing_table_lock.get_closest_nodes(&key, REPLICATION_PARAM);
                        break ResponsePayload::Nodes(closest_nodes);
                    } else {
                        attempt += 1;
                        if attempt >= RETRY_ATTEMPTS {
                            break ResponsePayload::Error(
                                "Failed to acquire lock on routing table".to_string(),
                            );
                        }
                    }
                }
            }
            RequestPayload::FindValue(key) => {
                if let Ok(mut storage_lock) = self.storage.lock() {
                    if let Some(value) = storage_lock.get(&key) {
                        ResponsePayload::Value(value.clone())
                    } else {
                        let mut attempt = 0;
                        loop {
                            if let Ok(routing_table_lock) = self.routing_table.lock() {
                                let closest_nodes =
                                    routing_table_lock.get_closest_nodes(&key, REPLICATION_PARAM);
                                break ResponsePayload::Nodes(closest_nodes);
                            } else {
                                attempt += 1;
                                if attempt >= RETRY_ATTEMPTS {
                                    break ResponsePayload::Error(
                                        "Failed to acquire lock on routing table".to_string(),
                                    );
                                }
                            }
                        }
                    }
                } else {
                    ResponsePayload::Error("Failed to acquire lock on storage".to_string())
                }
            }
        };

        self.protocol.send_message(
            &Message::Response(Response {
                request: request.clone(),
                receiver,
                payload,
            }),
            &request.sender,
        )
    }

    /// Handles a response RPC. If the id in the response does not match any outgoing request, then
    /// the response will be ignored.
    fn handle_response(&mut self, response: &Response<D>) {
        self.clone().update_routing_table(response.receiver.clone());
        if let Ok(pending_requests) = self.pending_requests.lock() {
            let Response { ref request, .. } = response.clone();
            if let Some(sender) = pending_requests.get(&request.id) {
                debug!(
                    "{} - Receiving response from {} {:#?}",
                    self.node_data.addr, response.receiver.addr, response.payload,
                );
                if let Err(err) = sender.send(response.clone()) {
                    error!("Error sending response: {}", err);
                }
            } else {
                warn!(
                    "{} - Original request not found; irrelevant response or expired request.",
                    self.node_data.addr
                );
            }
        }
    }

    /// Sends a request RPC.
    fn send_request(&mut self, dest: &NodeData<D>, payload: RequestPayload) -> Option<Response<D>> {
        debug!(
            "{} - Sending request to {} {:#?}",
            self.node_data.addr, dest.addr, payload
        );

        let (response_tx, response_rx) = channel();
        let mut token = Key::rand();

        let result = self
            .pending_requests
            .lock()
            .and_then(|mut pending_requests| {
                while pending_requests.contains_key(&token) {
                    token = Key::rand();
                }
                pending_requests.insert(token, response_tx);
                drop(pending_requests);

                self.protocol.send_message(
                    &Message::Request(Request {
                        id: token,
                        sender: (*self.node_data).clone(),
                        payload,
                    }),
                    dest,
                );
                Ok(response_rx.recv_timeout(Duration::from_millis(REQUEST_TIMEOUT)))
            });

        match result {
            Ok(Ok(response)) => {
                if let Ok(mut pending_requests) = self.pending_requests.lock() {
                    pending_requests.remove(&token);
                }
                Some(response)
            }
            Ok(Err(_)) => {
                warn!(
                    "{} - Request to {} timed out after waiting for {} milliseconds",
                    self.node_data.addr, dest.addr, REQUEST_TIMEOUT
                );
                if let Ok(mut pending_requests) = self.pending_requests.lock() {
                    pending_requests.remove(&token);
                }
                if let Ok(mut routing_table) = self.routing_table.lock() {
                    routing_table.remove_node(dest);
                }
                None
            }
            Err(err) => {
                error!("Failed to acquire lock on pending_requests: {}", err);
                None
            }
        }
    }

    /// Sends a `PING` RPC.
    pub fn rpc_ping(&mut self, dest: &NodeData<D>) -> Option<Response<D>> {
        self.send_request(dest, RequestPayload::Ping)
    }

    /// Sends a `STORE` RPC.
    fn rpc_store(&mut self, dest: &NodeData<D>, key: Key, value: String) -> Option<Response<D>> {
        self.send_request(dest, RequestPayload::Store(key, value))
    }

    /// Sends a `FIND_NODE` RPC.
    fn rpc_find_node(&mut self, dest: &NodeData<D>, key: &Key) -> Option<Response<D>> {
        self.send_request(dest, RequestPayload::FindNode(*key))
    }

    /// Sends a `FIND_VALUE` RPC.
    fn rpc_find_value(&mut self, dest: &NodeData<D>, key: &Key) -> Option<Response<D>> {
        self.send_request(dest, RequestPayload::FindValue(*key))
    }

    /// Spawns a thread that sends either a `FIND_NODE` or a `FIND_VALUE` RPC.
    fn spawn_find_rpc(
        mut self,
        dest: NodeData<D>,
        key: Key,
        sender: Sender<Option<Response<D>>>,
        find_node: bool,
    ) {
        thread::spawn(move || {
            let find_err = {
                if find_node {
                    sender.send(self.rpc_find_node(&dest, &key)).is_err()
                } else {
                    sender.send(self.rpc_find_value(&dest, &key)).is_err()
                }
            };

            if find_err {
                warn!("Receiver closed channel before rpc returned.");
            }
        });
    }

    /// Iteratively looks up nodes to determine the closest nodes to `key`. The search begins by
    /// selecting `CONCURRENCY_PARAM` nodes in the routing table and adding it to a shortlist. It
    /// then sends out either `FIND_NODE` or `FIND_VALUE` RPCs to `CONCURRENCY_PARAM` nodes not yet
    /// queried in the shortlist. The node will continue to fill its shortlist until it did not find
    /// a closer node for a round of RPCs or if runs out of nodes to query. Finally, it will query
    /// the remaining nodes in its shortlist until there are no remaining nodes or if it has found
    /// `REPLICATION_PARAM` active nodes.
    fn lookup_nodes(&mut self, key: &Key, find_node: bool) -> ResponsePayload<D> {
        let routing_table_result = self.routing_table.lock().map_err(|e| {
            ResponsePayload::Error(
                format!("Failed to acquire lock on routing table {}", e).to_string(),
            )
        });
        match routing_table_result {
            Ok(routing_table) => {
                let closest_nodes = routing_table.get_closest_nodes(key, CONCURRENCY_PARAM);
                drop(routing_table);
                let mut closest_distance = Key::new([255u8; KEY_LENGTH]);
                for node_data in &closest_nodes {
                    closest_distance = cmp::min(closest_distance, key.xor(&node_data.id))
                }
                // initialize found nodes, queried nodes, and priority queue
                let mut found_nodes: HashSet<NodeData> =
                    closest_nodes.clone().into_iter().collect();
                found_nodes.insert((*self.node_data).clone());
                let mut queried_nodes = HashSet::new();
                queried_nodes.insert((*self.node_data).clone());

                let mut queue: BinaryHeap<NodeDataDistancePair> = BinaryHeap::from(
                    closest_nodes
                        .into_iter()
                        .map(|node_data| {
                            NodeDataDistancePair(node_data.clone(), node_data.id.xor(key))
                        })
                        .collect::<Vec<NodeDataDistancePair>>(),
                );

                let (tx, rx) = channel();

                let mut concurrent_thread_count = 0;

                // spawn initial find requests
                for _ in 0..CONCURRENCY_PARAM {
                    if !queue.is_empty() {
                        if let Some(item) = queue.pop() {
                            self.clone()
                                .spawn_find_rpc(item.0, *key, tx.clone(), find_node);
                            concurrent_thread_count += 1;
                        }
                    }
                }

                // loop until we could not find a closer node for a round or if no threads are running
                while concurrent_thread_count > 0 {
                    while concurrent_thread_count < CONCURRENCY_PARAM && !queue.is_empty() {
                        if let Some(item) = queue.pop() {
                            self.clone()
                                .spawn_find_rpc(item.0, *key, tx.clone(), find_node);
                            concurrent_thread_count += 1;
                        }
                    }

                    let mut is_terminated = true;
                    if let Ok(response_opt) = rx.recv() {
                        concurrent_thread_count -= 1;
                        match response_opt {
                            Some(Response {
                                payload: ResponsePayload::Nodes(nodes),
                                receiver,
                                ..
                            }) => {
                                queried_nodes.insert(receiver);
                                for node_data in nodes {
                                    let curr_distance = node_data.id.xor(key);

                                    if !found_nodes.contains(&node_data) {
                                        if curr_distance < closest_distance {
                                            closest_distance = curr_distance;
                                            is_terminated = false;
                                        }

                                        found_nodes.insert(node_data.clone());
                                        let dist = node_data.id.xor(key);
                                        let next = NodeDataDistancePair(node_data.clone(), dist);
                                        queue.push(next.clone());
                                    }
                                }
                            }
                            Some(Response {
                                payload: ResponsePayload::Value(value),
                                ..
                            }) => return ResponsePayload::Value(value),
                            _ => is_terminated = false,
                        }
                        if is_terminated {
                            break;
                        }
                        debug!("CURRENT CLOSEST DISTANCE IS {:?}", closest_distance);
                    }
                }

                debug!(
                    "{} TERMINATED LOOKUP BECAUSE NOT CLOSER OR NO THREADS WITH DISTANCE {:?}",
                    self.node_data.addr, closest_distance,
                );

                // loop until no threads are running or if we found REPLICATION_PARAM active nodes
                while queried_nodes.len() < REPLICATION_PARAM {
                    while concurrent_thread_count < CONCURRENCY_PARAM && !queue.is_empty() {
                        if let Some(item) = queue.pop() {
                            self.clone()
                                .spawn_find_rpc(item.0, *key, tx.clone(), find_node);
                        }
                        concurrent_thread_count += 1;
                    }
                    if concurrent_thread_count == 0 {
                        break;
                    }

                    if let Some(response_opt) = rx.recv().unwrap_or(None) {
                        concurrent_thread_count -= 1;
                        match response_opt {
                            Response {
                                payload: ResponsePayload::Nodes(nodes),
                                receiver,
                                ..
                            } => {
                                queried_nodes.insert(receiver);
                                for node_data in nodes {
                                    if !found_nodes.contains(&node_data) {
                                        found_nodes.insert(node_data.clone());
                                        let dist = node_data.id.xor(key);
                                        let next = NodeDataDistancePair(node_data.clone(), dist);
                                        queue.push(next.clone());
                                    }
                                }
                            }
                            Response {
                                payload: ResponsePayload::Value(value),
                                ..
                            } => return ResponsePayload::Value(value),
                            _ => {}
                        }
                    }
                }

                let mut ret: Vec<NodeData> = queried_nodes.into_iter().collect();
                ret.sort_by_key(|node_data| node_data.id.xor(key));
                ret.truncate(REPLICATION_PARAM);
                debug!("{} -  CLOSEST NODES ARE {:#?}", self.node_data.addr, ret);
                ResponsePayload::Nodes(ret)
            }
            Err(e) => e,
        }
    }

    /// Inserts a key-value pair into the DHT.
    pub fn insert(&mut self, key: Key, value: &str) {
        if let ResponsePayload::Nodes(nodes) = self.lookup_nodes(&key, true) {
            for dest in nodes {
                let mut node = self.clone();
                let key_clone = key;
                let value_clone = value.to_string();
                thread::spawn(move || {
                    node.rpc_store(&dest, key_clone, value_clone);
                });
            }
        }
    }

    /// Gets the value associated with a particular key in the DHT. Returns `None` if the key was
    /// not found.
    pub fn get(&mut self, key: &Key) -> Option<String> {
        if let ResponsePayload::Value(value) = self.lookup_nodes(key, false) {
            Some(value)
        } else {
            None
        }
    }

    /// Returns a reference to the `RoutingTable` associated with the node.
    pub fn get_routing_table(&self) -> RoutingTable<D> {
        // TODO: address unwrap later
        self.routing_table.try_lock().unwrap().clone()
    }

    /// Returns the `NodeData` associated with the node.
    pub fn node_data(&self) -> NodeData<D> {
        (*self.node_data).clone()
    }

    /// Kills the current node and all active threads.
    pub fn kill(&self) {
        self.protocol.send_message(&Message::Kill, &self.node_data);
    }
}
