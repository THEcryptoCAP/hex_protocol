HEX(Hash-DEX) is a decentralised CLOB on Hash key chain(optimistic rollup)
# Walkthrough: Action-Based WebSocket Routing

## Overview

We integrated `cancel_order` and `get_order` into the live WebSocket pipeline, eliminating all OrderBook dead-code warnings and establishing a structured message-routing architecture across 3 files.

## Files Changed

- [orderbook.rs](file:///Users/avinash/Desktop/projects/hex_protocol/sequencer/src/engine/orderbook.rs) — Added `EngineMessage` enum, removed `#[allow(dead_code)]`
- [main.rs](file:///Users/avinash/Desktop/projects/hex_protocol/sequencer/src/main.rs) — Switched channel type, added match-based routing
- [websocket.rs](file:///Users/avinash/Desktop/projects/hex_protocol/sequencer/src/rpc/websocket.rs) — Full rewrite with action-based JSON routing

---

## How It Works

### The New Pipeline

```mermaid
sequenceDiagram
    participant Client as Frontend/Bot
    participant WS as WebSocket Task
    participant MPSC as MPSC Channel
    participant Engine as Matching Engine Loop
    
    Client->>WS: {"action": "PlaceOrder", "payload": {...}}
    WS->>WS: verify_order_signature()
    WS->>MPSC: EngineMessage::PlaceOrder(order)
    MPSC->>Engine: book.place_order(order)
    Engine-->>WS: (fire-and-forget)
    WS-->>Client: {"status": "OrderPlaced", "order_id": 42}

    Client->>WS: {"action": "CancelOrder", "payload": {...}}
    WS->>WS: verify_cancel_signature()
    WS->>MPSC: EngineMessage::CancelOrder { id, resp_tx }
    MPSC->>Engine: book.cancel_order(id)
    Engine-->>WS: oneshot → true/false
    WS-->>Client: {"status": "Cancelled", "order_id": 5}

    Client->>WS: {"action": "GetOrder", "payload": {...}}
    WS->>MPSC: EngineMessage::GetOrder { id, resp_tx }
    MPSC->>Engine: book.get_order(id)
    Engine-->>WS: oneshot → Some(Order) / None
    WS-->>Client: {"status": "Found", "order": {...}}
```

### PlaceOrder (unchanged logic, new envelope)
1. Client sends `{"action": "PlaceOrder", "payload": { user_address, price, amount, is_buy, signature }}`.
2. The WebSocket task verifies the EIP-712 signature (unchanged crypto pipeline).
3. The atomic counter generates a unique ID.
4. An `EngineMessage::PlaceOrder(order)` is pushed into the MPSC channel.
5. The engine loop calls `book.place_order()` — **fire-and-forget**, no response channel needed.
6. Client gets `{"status": "OrderPlaced", "order_id": N}`.

### CancelOrder (new)
1. Client sends `{"action": "CancelOrder", "payload": { user_address, order_id, signature }}`.
2. The WebSocket task verifies a **new** `Eip712CancelPayload` signature (hashes the `order_id` instead of trade data).
3. A `tokio::oneshot::channel` is created — this is the return line.
4. `EngineMessage::CancelOrder { id, response_tx }` is pushed into the MPSC channel.
5. The engine loop calls `book.cancel_order(id)`, checks if it returned `Some`, and sends `true`/`false` back over the oneshot.
6. The WebSocket task `await`s the oneshot result and replies to the client with `{"status": "Cancelled"}` or `{"error": "Order not found"}`.

### GetOrder (new)
1. Client sends `{"action": "GetOrder", "payload": { order_id }}`.
2. **No signature required** — this is a public read-only query.
3. A `oneshot::channel` is created.
4. `EngineMessage::GetOrder { id, response_tx }` is pushed into the MPSC.
5. The engine loop calls `book.get_order(id).cloned()` and sends the result back.
6. The WebSocket task formats the full order as JSON and replies.

> [!IMPORTANT]
> The core design principle is preserved: the OrderBook is **never** touched from multiple threads. Every operation still flows through the single-threaded MPSC consumer loop. The `oneshot` channels only carry *results* back — they don't give external code access to the OrderBook.

---

## New Client JSON Format

All WebSocket messages must now be wrapped in an `action` envelope:

```json
// Place Order
{"action": "PlaceOrder", "payload": {"user_address": "0x...", "price": 100, "amount": 5, "is_buy": true, "signature": "0x..."}}

// Cancel Order
{"action": "CancelOrder", "payload": {"user_address": "0x...", "order_id": 42, "signature": "0x..."}}

// Get Order (no signature)
{"action": "GetOrder", "payload": {"order_id": 42}}
```

## Validation

- `cargo check` passes with **zero errors** and **zero warnings from `orderbook.rs`**.
- The only remaining warnings are from `zk_client.rs` (the ZK prover module — a separate integration task).
