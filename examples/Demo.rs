use clap::Parser;
use kademlia_dht::{Key, Node, NodeData};
use sha3::{Digest, Sha3_256};
use std::alloc::System;
use std::convert::TryFrom;
use std::io;
use std::process::exit;
use std::thread;
use std::time::Duration;

//First Terminal Run
// cargo run  --example Demo -- --port 8080 --is-bootstrap
//Output
//Node Key is ED3B11D7B0EFF352ECEA93D1DA40E2B533BF13BD2F906B25E8675F06470A2225
//Please choose a command: (Get/Put/Print)

//Second Terminal Run
//cargo run  --example Demo  -- --port 8081 --bootstrap-key ED3B11D7B0EFF352ECEA93D1DA40E2B533BF13BD2F906B25E8675F06470A2225
//Please choose a command: (Get/Put/Print)

//Third Terminal Run
// cargo run  --example Demo -- --port 8081 --bootstrap-key ED3B11D7B0EFF352ECEA93D1DA40E2B533BF13BD2F906B25E8675F06470A2225
//Please choose a command: (Get/Put/Print)

/// This is a simple program
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, short, action)]
    is_bootstrap: bool,

    #[arg(short, long)]
    bootstrap_key: Option<String>,

    #[arg(short, long)]
    port: i16,
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

fn get_key(key: &str) -> Key {
    let mut hasher = Sha3_256::default();
    hasher.input(key.as_bytes());
    Key(clone_into_array(hasher.result().as_slice()))
}

fn main() {
    let args = Args::parse();
    let mut node = if args.is_bootstrap {
        let n = Node::new("127.0.0.1", args.port.to_string().as_str(), None);
        let k = n.node_data().id.0;
        println!("Key {:?}", hex::encode(k.to_vec()));
        println!("Node Key is {:?}", n.node_data().id);
        n
    } else {
        let data = hex::decode(&args.bootstrap_key.unwrap()).unwrap();
        println!("Key is {:?}", data);
        let key: kademlia_dht::Key = Key::try_from(data).unwrap();
        let node_data = NodeData::new(
            String::from("127.0.0.1"),
            args.port.to_string(),
            format!("{}:{}", "127.0.0.1", "8080".to_string()),
            key,
        );
        Node::new("127.0.0.1", args.port.to_string().as_str(), Some(node_data))
    };

    let c = thread::spawn(move || loop {
        println!("Please choose a command: (Get/Put/Print)");

        let mut command = String::new();
        io::stdin()
            .read_line(&mut command)
            .expect("Failed to read line");

        let command = command.trim();
        if command.starts_with("Get") {
            let data: Vec<String> = command
                .split_whitespace()
                .into_iter()
                .map(|x| x.to_string())
                .collect();
            println!("{:?}", data);
            let key = get_key(data.get(1).unwrap());
            let value = node.get(&key);
            println!("Value for GET {:?} : {:?}", key, value);
        } else if command.starts_with("Put") {
            println!("Performing Put Key operation");
            let data: Vec<String> = command
                .split_whitespace()
                .into_iter()
                .map(|x| x.to_string())
                .collect();
            let key = get_key(data.get(1).unwrap());
            node.insert(key, data.get(2).unwrap());
        } else if command == "Print" {
            println!("Performing Print operation");
            let c = node
                .routing_table
                .lock()
                .unwrap()
                .get_closest_nodes(&node.node_data().id, 3);
            println!("Neighbours of node {:?}", c);
        } else if command == "exit" {
            exit(1)
        } else {
            println!("Error: unknown command");
        }
    });
    c.join();
}
