mod engine;
mod prover;
mod rpc;

use engine::orderbook::{OrderBook, Order};
use tokio::sync::mpsc;
use rpc::websocket::start_server;

#[tokio::main]
async fn main() {
    println!("Initializing HEX Decentralized CLOB Sequencer...");

    // 1. Initialize the OrderBook Memory safely
    let mut book = OrderBook::new();
    
    // 2. Create the MPSC Channel for 10,000 parallel queued orders
    let (tx, mut rx) = mpsc::channel::<Order>(10_000);

    // 3. Spawn the internal Matching Engine Task
    // This runs completely independently in the background, systematically processing orders ONE AT A TIME 
    // to strictly preserve the Price-Time Priority and completely eliminate any multi-threading Data Races.
    tokio::spawn(async move {
        println!("Core Matching Engine Loop is actively listening for incoming RPC orders...");
        
        // Wait asynchronously for validated orders from the RPC WebSockets
        while let Some(order) = rx.recv().await {
            // Because we pass ownership of the order, and `book` is wholly owned by this MPSC consumer loop,
            // we get fearless, completely locked-down memory-safe sequence matching without a single Mutex!
            // println!("Received new Order: {:?}", order); // (silenced to keep high-frequency performance clean)
            book.place_order(order);
        }
    });

    // 4. Start the Axum WebSockets Server to blindly ingest the market data
    // It passes clones of the `tx` sender into the thousands of individual parallel user connections.
    start_server(tx).await;
}
