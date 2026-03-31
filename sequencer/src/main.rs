mod engine;
mod prover;
mod rpc;

use engine::orderbook::{OrderBook, EngineMessage};
use prover::zk_client::{HexProver, Trade, BatchPayload, AccountState, MerkleProof};
use tokio::sync::mpsc;
use rpc::websocket::start_server;

// Number of matched trades to accumulate before dispatching a ZK proof batch.
// In production this would be tuned based on gas costs and proving latency.
const BATCH_SIZE: usize = 10;

// The compiled SP1 guest program ELF binary.
// Replace with: include_bytes!("../program/target/...") once the guest is compiled via `cargo prove build`.
// While empty, the prover is gracefully bypassed at runtime.
static GUEST_ELF: &[u8] = &[];

#[tokio::main]
async fn main() {
    println!("Initializing HEX Decentralized CLOB Sequencer...");

    // 1. Initialize the OrderBook Memory safely
    let mut book = OrderBook::new();
    
    // 2. Create the MPSC Channel for 10,000 parallel queued orders
    let (tx, mut rx) = mpsc::channel::<EngineMessage>(10_000);

    // 3. Spawn the internal Matching Engine Task
    // This runs completely independently in the background, systematically processing orders ONE AT A TIME 
    // to strictly preserve the Price-Time Priority and completely eliminate any multi-threading Data Races.
    tokio::spawn(async move {
        println!("Core Matching Engine Loop is actively listening for incoming RPC orders...");

        // Trade buffer accumulates fills for ZK batch proving
        let mut trade_buffer: Vec<Trade> = Vec::new();
        
        // Wait asynchronously for validated messages from the RPC WebSockets
        while let Some(msg) = rx.recv().await {
            match msg {
                EngineMessage::PlaceOrder(order) => {
                    // place_order now returns all fills that occurred during matching
                    let fills = book.place_order(order);

                    if !fills.is_empty() {
                        println!("Engine captured {} fill(s) for ZK batch.", fills.len());
                        trade_buffer.extend(fills);
                    }

                    // When the buffer reaches the batch threshold, dispatch for proving
                    if trade_buffer.len() >= BATCH_SIZE {
                        let batch_trades = std::mem::take(&mut trade_buffer);
                        let batch_len = batch_trades.len();

                        // Construct placeholder account states for each trade participant.
                        // In production, these would be read from a persistent state database
                        // (account balances, nonces) backed by a Merkle tree.
                        let placeholder_state = AccountState {
                            nonce: 0,
                            base_balance: 0,
                            quote_balance: 0,
                        };
                        let placeholder_proof = MerkleProof {
                            sibling_hashes: Vec::new(),
                            is_left: Vec::new(),
                        };

                        let payload = BatchPayload {
                            previous_state_root: [0u8; 32],
                            new_state_root: [0u8; 32],
                            trades: batch_trades,
                            maker_states: vec![placeholder_state.clone(); batch_len],
                            maker_proofs: vec![placeholder_proof.clone(); batch_len],
                            taker_states: vec![placeholder_state.clone(); batch_len],
                            taker_proofs: vec![placeholder_proof.clone(); batch_len],
                        };

                        // Spawn proof generation on a blocking thread so it doesn't stall
                        // the critical matching engine loop.
                        if !GUEST_ELF.is_empty() {
                            tokio::task::spawn_blocking(move || {
                                let prover = HexProver::new(GUEST_ELF);
                                match prover.generate_evm_proof(&payload) {
                                    Ok(_proof) => {
                                        println!("ZK Proof generated and ready for on-chain verification!");
                                    }
                                    Err(e) => {
                                        println!("ZK Proof generation failed: {}", e);
                                    }
                                }
                            });
                        } else {
                            println!(
                                "Batch of {} trades ready. ZK proving bypassed (guest ELF not compiled).",
                                payload.trades.len()
                            );
                        }
                    }
                }
                EngineMessage::CancelOrder { id, response_tx } => {
                    let was_found = book.cancel_order(id).is_some();
                    let _ = response_tx.send(was_found);
                }
                EngineMessage::GetOrder { id, response_tx } => {
                    let order = book.get_order(id).cloned();
                    let _ = response_tx.send(order);
                }
            }
        }
    });

    // 4. Start the Axum WebSockets Server to blindly ingest the market data
    // It passes clones of the `tx` sender into the thousands of individual parallel user connections.
    start_server(tx).await;
}
