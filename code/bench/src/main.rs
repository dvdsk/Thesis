// use clap::{Parser, Subcommand};

use node::Partition;

enum Operation {
    Ls,
    Touch,
    Read,
    Write,
}

enum Command {

}

struct Bench {
    operation: Operation,
    clients: usize,
    partitions: Partition,

}

fn main() {
    println!("Hello, world!");
}
