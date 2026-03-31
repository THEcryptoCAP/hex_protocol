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
use tokio::sync::{mpsc, oneshot};
use std::str::FromStr;

use ethers::types::{Signature, H160, U256};
use ethers::contract::{Eip712, EthAbiType};

use crate::engine::orderbook::{Order, EngineMessage};

// ═══════════════════════════════════════════════════════════════════
// 1. RAW JSON PAYLOADS — What the frontend sends over the WebSocket
// ═══════════════════════════════════════════════════════════════════

/// The top-level envelope for every WebSocket message.
/// The `action` field determines which handler branch executes.
#[derive(Debug, Deserialize)]
pub struct ClientRequest {
    pub action: String,
    pub payload: serde_json::Value,
}

/// Payload for placing a new order (requires EIP-712 signature).
#[derive(Debug, Deserialize, Serialize)]
pub struct RawOrderPayload {
    pub user_address: String,
    pub price: u64,
    pub amount: u64,
    pub is_buy: bool,
    pub signature: String,
}

/// Payload for cancelling an existing order (requires EIP-712 signature).
#[derive(Debug, Deserialize, Serialize)]
pub struct RawCancelPayload {
    pub user_address: String,
    pub order_id: u64,
    pub signature: String,
}

/// Payload for querying an order (public read — no signature needed).
#[derive(Debug, Deserialize)]
pub struct RawGetOrderPayload {
    pub order_id: u64,
}

// ═══════════════════════════════════════════════════════════════
// 2. EIP-712 TYPED DATA DEFINITIONS — What the user's wallet signs
// ═══════════════════════════════════════════════════════════════

/// Typed data for Place Order signatures.
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

/// Typed data for Cancel Order signatures.
#[derive(Eip712, EthAbiType, Clone, Debug)]
#[eip712(
    name = "HexProtocol",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
struct Eip712CancelPayload {
    user_address: H160,
    order_id: U256,
}

// ═══════════════════════════════════════════════════════════════════
// 3. SIGNATURE VERIFICATION — Cryptographic proof-of-origin checks
// ═══════════════════════════════════════════════════════════════════

/// Verifies an EIP-712 Place Order signature.
/// Steps: Parse → Construct typed data → Recover signer → Compare addresses.
fn verify_order_signature(payload: &RawOrderPayload) -> bool {
    let user_address = match H160::from_str(&payload.user_address) {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    let signature = match Signature::from_str(&payload.signature) {
        Ok(sig) => sig,
        Err(_) => return false,
    };

    let typed_payload = Eip712OrderPayload {
        user_address,
        price: U256::from(payload.price),
        amount: U256::from(payload.amount),
        is_buy: payload.is_buy,
    };

    match signature.recover_typed_data(&typed_payload) {
        Ok(recovered_address) => recovered_address == user_address,
        Err(_) => false,
    }
}

/// Verifies an EIP-712 Cancel Order signature.
/// Identical cryptographic pipeline, but hashes the order_id instead of trade data.
fn verify_cancel_signature(payload: &RawCancelPayload) -> bool {
    let user_address = match H160::from_str(&payload.user_address) {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    let signature = match Signature::from_str(&payload.signature) {
        Ok(sig) => sig,
        Err(_) => return false,
    };

    let typed_payload = Eip712CancelPayload {
        user_address,
        order_id: U256::from(payload.order_id),
    };

    match signature.recover_typed_data(&typed_payload) {
        Ok(recovered_address) => recovered_address == user_address,
        Err(_) => false,
    }
}

// ═══════════════════════════════════════════════════════════════
// 4. APP STATE — Shared across every WebSocket connection
// ═══════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct AppState {
    tx: mpsc::Sender<EngineMessage>,
    order_counter: Arc<AtomicU64>,
}

// ═══════════════════════════════════════════════════════════════
// 5. AXUM SERVER INITIALIZATION
// ═══════════════════════════════════════════════════════════════

pub async fn start_server(tx: mpsc::Sender<EngineMessage>) {
    let state = AppState {
        tx,
        order_counter: Arc::new(AtomicU64::new(1)),
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("WebSocket RPC fully initialized and listening on ws://0.0.0.0:3000/ws");

    axum::serve(listener, app).await.unwrap();
}

// ═══════════════════════════════════════════════════════════════
// 6. WEBSOCKET UPGRADE HANDLER
// ═══════════════════════════════════════════════════════════════

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_connection(socket, state))
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. CORE ASYNC CONNECTION LOOP — Action-based message routing per connection
// ═══════════════════════════════════════════════════════════════════════════════

async fn handle_connection(stream: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = stream.split();

    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(text) = message {
            // Parse the top-level envelope to determine the action
            let request: ClientRequest = match serde_json::from_str(&text) {
                Ok(req) => req,
                Err(e) => {
                    let err = format!("{{\"error\": \"JSON Parsing Error: {}\"}}", e);
                    let _ = sender.send(Message::Text(err)).await;
                    continue;
                }
            };

            match request.action.as_str() {
                // ─────────────────────────────────────────
                // ACTION: PlaceOrder
                // ─────────────────────────────────────────
                "PlaceOrder" => {
                    let payload: RawOrderPayload = match serde_json::from_value(request.payload) {
                        Ok(p) => p,
                        Err(e) => {
                            let err = format!("{{\"error\": \"Invalid PlaceOrder payload: {}\"}}", e);
                            let _ = sender.send(Message::Text(err)).await;
                            continue;
                        }
                    };

                    if !verify_order_signature(&payload) {
                        let _ = sender.send(Message::Text("{\"error\": \"Invalid EIP-712 Signature\"}".to_string())).await;
                        continue;
                    }

                    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    let unique_id = state.order_counter.fetch_add(1, Ordering::SeqCst);

                    let order = Order {
                        id: unique_id,
                        user_address: payload.user_address,
                        price: payload.price,
                        amount: payload.amount,
                        is_buy: payload.is_buy,
                        timestamp,
                    };

                    if state.tx.send(EngineMessage::PlaceOrder(order)).await.is_err() {
                        println!("Engine channel closed. Shutting down connection.");
                        break;
                    }

                    let resp = format!("{{\"status\": \"OrderPlaced\", \"order_id\": {}}}", unique_id);
                    let _ = sender.send(Message::Text(resp)).await;
                }

                // ─────────────────────────────────────────
                // ACTION: CancelOrder
                // ─────────────────────────────────────────
                "CancelOrder" => {
                    let payload: RawCancelPayload = match serde_json::from_value(request.payload) {
                        Ok(p) => p,
                        Err(e) => {
                            let err = format!("{{\"error\": \"Invalid CancelOrder payload: {}\"}}", e);
                            let _ = sender.send(Message::Text(err)).await;
                            continue;
                        }
                    };

                    if !verify_cancel_signature(&payload) {
                        let _ = sender.send(Message::Text("{\"error\": \"Invalid EIP-712 Signature\"}".to_string())).await;
                        continue;
                    }

                    // Create a oneshot channel so the engine loop can reply with the result
                    let (resp_tx, resp_rx) = oneshot::channel();

                    if state.tx.send(EngineMessage::CancelOrder {
                        id: payload.order_id,
                        response_tx: resp_tx,
                    }).await.is_err() {
                        println!("Engine channel closed. Shutting down connection.");
                        break;
                    }

                    // Await the response from the single-threaded engine loop
                    match resp_rx.await {
                        Ok(true) => {
                            let resp = format!("{{\"status\": \"Cancelled\", \"order_id\": {}}}", payload.order_id);
                            let _ = sender.send(Message::Text(resp)).await;
                        }
                        Ok(false) => {
                            let resp = format!("{{\"error\": \"Order not found\", \"order_id\": {}}}", payload.order_id);
                            let _ = sender.send(Message::Text(resp)).await;
                        }
                        Err(_) => {
                            let _ = sender.send(Message::Text("{\"error\": \"Engine did not respond\"}".to_string())).await;
                        }
                    }
                }

                // ─────────────────────────────────────────
                // ACTION: GetOrder (public read, no sig)
                // ─────────────────────────────────────────
                "GetOrder" => {
                    let payload: RawGetOrderPayload = match serde_json::from_value(request.payload) {
                        Ok(p) => p,
                        Err(e) => {
                            let err = format!("{{\"error\": \"Invalid GetOrder payload: {}\"}}", e);
                            let _ = sender.send(Message::Text(err)).await;
                            continue;
                        }
                    };

                    let (resp_tx, resp_rx) = oneshot::channel();

                    if state.tx.send(EngineMessage::GetOrder {
                        id: payload.order_id,
                        response_tx: resp_tx,
                    }).await.is_err() {
                        println!("Engine channel closed. Shutting down connection.");
                        break;
                    }

                    match resp_rx.await {
                        Ok(Some(order)) => {
                            let resp = format!(
                                "{{\"status\": \"Found\", \"order\": {{\"id\": {}, \"user_address\": \"{}\", \"price\": {}, \"amount\": {}, \"is_buy\": {}, \"timestamp\": {}}}}}",
                                order.id, order.user_address, order.price, order.amount, order.is_buy, order.timestamp
                            );
                            let _ = sender.send(Message::Text(resp)).await;
                        }
                        Ok(None) => {
                            let resp = format!("{{\"error\": \"Order not found\", \"order_id\": {}}}", payload.order_id);
                            let _ = sender.send(Message::Text(resp)).await;
                        }
                        Err(_) => {
                            let _ = sender.send(Message::Text("{\"error\": \"Engine did not respond\"}".to_string())).await;
                        }
                    }
                }

                // ─────────────────────────────────────────
                // UNKNOWN ACTION
                // ─────────────────────────────────────────
                unknown => {
                    let err = format!("{{\"error\": \"Unknown action: {}\"}}", unknown);
                    let _ = sender.send(Message::Text(err)).await;
                }
            }
        }
    }
}
