use kademlia_dht::{Key, Node, NodeData};
use sha3::{Digest, Sha3_256};
use std::process::exit;
use std::thread;
use std::time::Duration;


fn main() {
    let mut node = Node::new("127.0.0.1", "8080", None);
    let key = Node::get_key("Hello");
    let value = "World";
    println!("Node Data {:?}", node.node_data());
    let mut node1 = Node::new("127.0.0.1", "8081", Some(node.node_data()));
    node.insert(key, value);
    let mut node2 = Node::new("127.0.0.1", "8082", Some(node.node_data()));

    let mut node3 = Node::new("127.0.0.1", "8083", Some(node.node_data()));

    let mut node4 = Node::new("127.0.0.1", "8084", Some(node.node_data()));

    let mut node5 = Node::new("127.0.0.1", "8085", Some(node.node_data()));

    // inserting is asynchronous, so sleep for a second
    thread::sleep(Duration::from_millis(1000));

    assert_eq!(node.get(&key).unwrap(), value);

    println!("Fetch Value {:?}", node.get(&key));

    println!("Fetch Value {:?}", node1.get(&key));

    let key = Node::get_key("Sikandar");
    let value = "Sikka";
    node.insert(key, value);
    thread::sleep(Duration::from_millis(7000));
    println!("Fetch Value {:?}", node2.get(&key));

    let c = node2
        .routing_table
        .lock()
        .unwrap()
        .get_closest_nodes(&node1.node_data().id, 3);
    println!("NeighbourHood for 2 :{:?}", c);

    let c = node1
        .routing_table
        .lock()
        .unwrap()
        .get_closest_nodes(&node1.node_data().id, 3);
    println!("NeighbourHood  for 1 :{:?}", c);

    let c = node5
        .routing_table
        .lock()
        .unwrap()
        .get_closest_nodes(&node5.node_data().id, 3);
    println!("NeighbourHood  for 5 :{:?}", c);

    let c = node4
        .routing_table
        .lock()
        .unwrap()
        .get_closest_nodes(&node4.node_data().id, 3);
    println!("NeighbourHood  for 4 :{:?}", c);

    let c = node3
        .routing_table
        .lock()
        .unwrap()
        .get_closest_nodes(&node3.node_data().id, 3);

    println!("NeighbourHood  for 3 :{:?}", c);
    println!("Ne {:?}", node.routing_table.lock().unwrap().total_peers());
    
    
}
