use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use tokio::sync::mpsc;
use std::str::FromStr;

use ethers::types::{Signature, H160, U256};
use ethers::contract::{Eip712, EthAbiType};

use crate::engine::orderbook::Order;

// 1. Define the Raw Payload we receive from the frontend
#[derive(Debug, Deserialize, Serialize)]
pub struct RawOrderPayload {
    pub user_address: String,
    pub price: u64,
    pub amount: u64,
    pub is_buy: bool,
    pub signature: String, // e.g. EIP-712 hex signature
}

// 2. Define the Typed Data for EIP-712 verification
#[derive(Eip712, EthAbiType, Clone, Debug)]
#[eip712(
    name = "HexProtocol",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
struct Eip712OrderPayload {
    user_address: H160,
    price: U256,
    amount: U256,
    is_buy: bool,
}

// 3. EIP-712 Signature Verification using ethers
fn verify_signature(payload: &RawOrderPayload) -> bool {
    // 1. Parse raw strings to EVM types
    let user_address = match H160::from_str(&payload.user_address) {
        Ok(addr) => addr,
        Err(_) => return false,
    };
    
    let signature = match Signature::from_str(&payload.signature) {
        Ok(sig) => sig,
        Err(_) => return false,
    };

    // 2. Construct the typed data object
    let typed_payload = Eip712OrderPayload {
        user_address,
        price: U256::from(payload.price),
        amount: U256::from(payload.amount),
        is_buy: payload.is_buy,
    };

    // 3. Recover the address from EIP-712 hash and perform final checks
    match signature.recover_typed_data(&typed_payload) {
        Ok(recovered_address) => recovered_address == user_address,
        Err(_) => false,
    }
}

// App configuration state passed to our Axum handlers
#[derive(Clone)]
pub struct AppState {
    tx: mpsc::Sender<Order>,
    order_counter: Arc<AtomicU64>,
}

// 4. Axum Server Initialization
pub async fn start_server(tx: mpsc::Sender<Order>) {
    let state = AppState { 
        tx,
        order_counter: Arc::new(AtomicU64::new(1)),
    };

    // Set up the WebSocket route at /ws
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("WebSocket RPC fully initialized and listening on ws://0.0.0.0:3000/ws");
    
    // Start serving requests infinitely
    axum::serve(listener, app).await.unwrap();
}

// 5. WebSocket Upgrade Handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // This upgrades the HTTP connection to a WebSocket connection
    ws.on_upgrade(|socket| handle_connection(socket, state))
}

// 6. The Core Async Connection Loop (Runs independently for EVERY single user without blocking!)
async fn handle_connection(stream: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = stream.split();

    // Loop asynchronously processing messages as they stream in from this specific user
    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(text) = message {
            // Attempt to parse the incoming JSON
            match serde_json::from_str::<RawOrderPayload>(&text) {
                Ok(payload) => {
                    // Instantly and independently verify the CPU-heavy cryptography here 
                    // BEFORE it ever touches the OrderBook lock
                    if !verify_signature(&payload) {
                        let _ = sender.send(Message::Text("Error: Invalid EIP-712 Signature".to_string())).await;
                        continue;
                    }
                    
                    // Map the standard JSON payload into the matching engine's native Order struct
                    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    
                    // Hardware-level atomic counter guarantees absolutely unique sequentially ordered IDs
                    // with zero locks perfectly suited for high-throughput single-node sequencing
                    let unique_id = state.order_counter.fetch_add(1, Ordering::SeqCst);

                    let order = Order {
                        id: unique_id, 
                        user_address: payload.user_address,
                        price: payload.price,
                        amount: payload.amount,
                        is_buy: payload.is_buy,
                        timestamp,
                    };

                    // Send the validated and constructed order into the single-threaded memory-safe Matching Engine!
                    if state.tx.send(order).await.is_err() {
                        println!("Engine channel closed. Shutting down connection.");
                        break;
                    }

                    // Notify user of success
                    let _ = sender.send(Message::Text("Order Submitted to Sequencer".to_string())).await;
                }
                Err(e) => {
                    let err_msg = format!("JSON Parsing Error: {}", e);
                    let _ = sender.send(Message::Text(err_msg)).await;
                }
            }
        }
    }
}
