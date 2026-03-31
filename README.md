HEX(Hash-DEX) is a decentralised CLOB on Hash key chain(optimistic rollup)



## The Full Pipeline: Life of an Order

Here is the exact path an order takes through the entire system, step by step.

```mermaid
flowchart TD
    A[Client Browser or Bot] -->|WS JSON| B

    subgraph websocket.rs
        B[handle_connection] --> C{action?}
        C -->|PlaceOrder| D[Parse RawOrderPayload]
        C -->|CancelOrder| E[Parse RawCancelPayload]
        C -->|GetOrder| F[Parse RawGetOrderPayload]
        D --> G[verify_order_signature - EIP712]
        E --> H[verify_cancel_signature - EIP712]
        G --> I[AtomicU64 fetch_add - unique ID]
        I --> J[Construct Order struct]
        J --> K[mpsc send PlaceOrder]
        H --> L[Create oneshot channel]
        L --> M[mpsc send CancelOrder]
        F --> N[Create oneshot channel]
        N --> O[mpsc send GetOrder]
    end

    K --> P
    M --> P
    O --> P

    subgraph main.rs
        P[Engine Loop - single threaded] -->|PlaceOrder| Q[book.place_order]
        P -->|CancelOrder| R[book.cancel_order]
        P -->|GetOrder| S[book.get_order]
        Q --> V[trade_buffer.extend fills]
        V -->|buffer >= BATCH_SIZE| W[Construct BatchPayload]
    end

    subgraph orderbook.rs
        Q -.-> Q2[match_buy or match_sell]
        Q2 -.-> Q3[Constructs Trade on each fill]
        R -.-> R2[HashMap remove - O1]
        S -.-> S2[HashMap get - O1]
    end

    R2 -->|oneshot reply| T[WS: Cancelled or Not Found]
    S2 -->|oneshot reply| U[WS: Order JSON or Not Found]

    subgraph zk_client.rs
        W --> X[HexProver generate_evm_proof]
    end

    subgraph program/src/main.rs
        X --> Y[SP1 ZK-VM verify_state_transition]
    end

    Y --> Z[On-chain HashKey EVM]
```

### Step-by-Step Walkthrough

#### Step 1: Client sends a WebSocket message
The client (browser, trading bot, etc.) connects to `ws://0.0.0.0:3000/ws` and sends a JSON message:
```json
{
  "action": "PlaceOrder",
  "payload": {
    "user_address": "0xAbC123...",
    "price": 100,
    "amount": 5,
    "is_buy": true,
    "signature": "0x1a2b3c..."
  }
}
```

