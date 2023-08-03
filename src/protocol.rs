use crate::key::Key;
use crate::node::node_data::NodeData;
use crate::MESSAGE_LENGTH;
use bincode;
use serde_derive::{Deserialize, Serialize};
use std::net::UdpSocket;
use std::str;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use tracing::{error, warn};

/// An enum representing a request RPC.
///
/// Each request RPC also carries a randomly generated key. The response to the RPC must contain
/// the same randomly generated key or else it will be ignored.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    pub id: Key,
    pub sender: NodeData<D>,
    pub payload: RequestPayload,
}

/// An enum representing the payload to a request RPC.
///
/// As stated in the Kademlia paper, the four possible RPCs are `PING`, `STORE`, `FIND_NODE`, and
/// `FIND_VALUE`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RequestPayload {
    Ping,
    Store(Key, String),
    FindNode(Key),
    FindValue(Key),
}

/// An enum representing the response to a request RPC.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    pub request: Request<D>,
    pub receiver: NodeData<D>,
    pub payload: ResponsePayload<D>,
}

/// An enum representing the payload to a response RPC.
///
/// As stated in the Kademlia paper, a response to a request could be a list of nodes, a value, or
/// a pong.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ResponsePayload<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    Nodes(Vec<NodeData<D>>),
    Value(String),
    Pong,
    Error(String),
}

/// An enum that represents a message that is sent between nodes.
#[derive(Serialize, Deserialize, Debug)]
pub enum Message<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    Request(Request<D>),
    Response(Response<D>),
    Kill,
}

/// `Protocol` facilitates the underlying communication between nodes by sending messages to other
/// nodes, and by passing messages from other nodes to the current node.
#[derive(Debug, Clone)]
pub struct Protocol<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    socket: Arc<UdpSocket>,
    _phantom: std::marker::PhantomData<D>,
}

impl<D> Protocol<D>
where
    D: std::fmt::Debug + Clone + Eq + PartialEq,
{
    pub fn new(socket: UdpSocket, tx: Sender<Message<D>>) -> Protocol<D> {
        let protocol = Protocol {
            socket: Arc::new(socket),
            _phantom: std::marker::PhantomData,
        };

        let ret = protocol.clone();
        thread::spawn(move || {
            let mut buffer = [0u8; MESSAGE_LENGTH];
            loop {
                if let Ok((len, _src_addr)) = protocol.socket.recv_from(&mut buffer) {
                    if let Ok(message) = bincode::deserialize(&buffer[..len]) {
                        if let Err(err) = tx.send(message) {
                            error!("Protocol: Error sending message: {}", err);
                            break;
                        }
                    }
                }
            }
        });
        ret
    }

    pub fn send_message(&self, message: &Message<D>, node_data: &NodeData<D>) {
        let size_limit = bincode::Bounded(MESSAGE_LENGTH as u64);
        let buffer_string = match bincode::serialize(&message, size_limit) {
            Ok(buffer) => buffer,
            Err(err) => {
                error!("Protocol: Failed to serialize message: {}", err);
                return;
            }
        };
        let NodeData { ref addr, .. } = node_data;
        if let Err(err) = self.socket.send_to(&buffer_string, addr) {
            warn!("Protocol: Failed to send data: {}", err);
        }
    }
}
