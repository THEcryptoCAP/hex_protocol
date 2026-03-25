// src/main.rs

// 1. Declare the engine module so the compiler looks at `src/engine/mod.rs`
mod engine;

// 2. Import the structs you wrote in `orderbook.rs`
use engine::orderbook::{OrderBook, Order};

fn main() {
    println!("Initializing HEX Matching Engine...");

    // 3. Initialize the OrderBook in memory
    // We make it `mut` (mutable) because processing orders changes its internal state.
    let mut book = OrderBook::new();
    println!("OrderBook successfully initialized.\n");

    // 4. Create a mock Buy Order
    let buy_order = Order {
        id: 1,
        user_address: "0xAlice...".to_string(),
        price: 2000, // Willing to buy at $2000
        amount: 10,  // Wants 10 units of the RWA token
        is_buy: true,
    };

    println!("Incoming {:?}", buy_order);

    // 5. Process the order
    // This passes ownership of the `buy_order` into the engine.
    book.process_order(buy_order);

    // 6. Check the state of the OrderBook
    // Since there were no sellers (asks) to match with, this order should 
    // now be resting peacefully in the `bids` B-Tree.
    println!("\nCurrent Bids in the OrderBook:");
    for (price, orders) in book.bids.iter() {
        println!("Price Level ${}: {} resting orders", price, orders.len());
    }
}
