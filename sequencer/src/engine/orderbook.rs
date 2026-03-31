use std::collections::{BTreeMap, HashMap};
use tokio::sync::oneshot;
use crate::prover::zk_client::Trade;

// Defines the type of action the core engine loop should execute.
// Each variant carries its own data and, where needed, a oneshot channel
// to send the result back to the calling WebSocket task.
pub enum EngineMessage {
    PlaceOrder(Order),
    CancelOrder {
        id: u64,
        response_tx: oneshot::Sender<bool>,
    },
    GetOrder {
        id: u64,
        response_tx: oneshot::Sender<Option<Order>>,
    },
}

// 1. Define the Order Struct
#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub user_address: String, 
    pub price: u64,
    pub amount: u64,
    pub is_buy: bool,
    pub timestamp: u64,
}

// 2. Define the OrderBook Struct
pub struct OrderBook {
    // We store the actual order data here for O(1) lookups and cancellations
    pub orders: HashMap<u64, Order>,
    
    // BTreeMap keeps keys (prices) perfectly sorted.
    // Asks (Sellers): We use price as the key, and a queue of order IDs as the value.
    pub asks: BTreeMap<u64, Vec<u64>>, 
    
    // Bids (Buyers): Same, queue of order IDs.
    pub bids: BTreeMap<u64, Vec<u64>>, 
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            orders: HashMap::new(),
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
        }
    }
                  
    // O(1) Read operation
    pub fn get_order(&self, id: u64) -> Option<&Order> {
        self.orders.get(&id)
    }

    // O(1) Cancel operation
    // Note: The ID will lazily be removed from the BTreeMap queues during the next matching cycle
    pub fn cancel_order(&mut self, id: u64) -> Option<Order> {
        self.orders.remove(&id)
    }

    // 3. The Core Processing Loop
    // Returns a Vec<Trade> capturing every fill that occurred during matching.
    // These trades are forwarded to the ZK prover pipeline for batch proof generation.
    pub fn place_order(&mut self, mut order: Order) -> Vec<Trade> {
        let trades = if order.is_buy {
            let t = self.match_buy(&mut order);
            // If the order wasn't fully filled, add the remainder to the bids book
            if order.amount > 0 {
                self.add_bid(order);
            }
            t
        } else {
            let t = self.match_sell(&mut order);
            // If the order wasn't fully filled, add the remainder to the asks book
            if order.amount > 0 {
                self.add_ask(order);
            }
            t
        };
        trades
    }

    // 4. Matching a Buy Order Against Existing Asks
    // Returns the list of Trade fills that occurred during matching.
    fn match_buy(&mut self, order: &mut Order) -> Vec<Trade> {
        let mut empty_price_levels = Vec::new();
        let mut fully_filled_ids = Vec::new();
        let mut trades = Vec::new();

        for (&ask_price, asks_at_price) in self.asks.iter_mut() {
            if ask_price > order.price || order.amount == 0 {
                break; 
            }

            asks_at_price.retain(|&ask_id| {
                if order.amount == 0 {
                    return true;
                }

                if let Some(ask) = self.orders.get_mut(&ask_id) {
                    let fill_amount = std::cmp::min(order.amount, ask.amount);

                    // Capture the fill as a Trade before mutating balances.
                    // The resting order (ask) is the maker; the incoming order is the taker.
                    trades.push(Trade {
                        maker_pubkey: ask.user_address.as_bytes().to_vec(),
                        taker_pubkey: order.user_address.as_bytes().to_vec(),
                        amount: fill_amount,
                        price: ask_price,
                        maker_signature: Vec::new(), // Original signature not retained in Order struct
                    });

                    order.amount -= fill_amount;
                    ask.amount -= fill_amount;

                    println!("Matched: {} units at price {}", fill_amount, ask_price);

                    if ask.amount == 0 {
                        fully_filled_ids.push(ask_id);
                        false // drop from queue
                    } else {
                        true // keep in queue
                    }
                } else {
                    // Order was cancelled in O(1) earlier, drop it from queue now (lazy removal)
                    false 
                }
            });

            if asks_at_price.is_empty() {
                empty_price_levels.push(ask_price);
            }
        }

        for id in fully_filled_ids {
            self.orders.remove(&id);
        }
        for price in empty_price_levels {
            self.asks.remove(&price);
        }

        trades
    }

    // 5. Matching a Sell Order Against Existing Bids
    // Returns the list of Trade fills that occurred during matching.
    fn match_sell(&mut self, order: &mut Order) -> Vec<Trade> {
        let mut empty_price_levels = Vec::new();
        let mut fully_filled_ids = Vec::new();
        let mut trades = Vec::new();

        for (&bid_price, bids_at_price) in self.bids.iter_mut().rev() {
            if bid_price < order.price || order.amount == 0 {
                break;
            }

            bids_at_price.retain(|&bid_id| {
                if order.amount == 0 {
                    return true;
                }

                if let Some(bid) = self.orders.get_mut(&bid_id) {
                    let fill_amount = std::cmp::min(order.amount, bid.amount);

                    // Capture the fill as a Trade.
                    // The resting order (bid) is the maker; the incoming order is the taker.
                    trades.push(Trade {
                        maker_pubkey: bid.user_address.as_bytes().to_vec(),
                        taker_pubkey: order.user_address.as_bytes().to_vec(),
                        amount: fill_amount,
                        price: bid_price,
                        maker_signature: Vec::new(),
                    });

                    order.amount -= fill_amount;
                    bid.amount -= fill_amount;

                    println!("Matched: {} units at price {}", fill_amount, bid_price);

                    if bid.amount == 0 {
                        fully_filled_ids.push(bid_id);
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            });

            if bids_at_price.is_empty() {
                empty_price_levels.push(bid_price);
            }
        }

        for id in fully_filled_ids {
            self.orders.remove(&id);
        }
        for price in empty_price_levels {
            self.bids.remove(&price);
        }

        trades
    }

    fn add_bid(&mut self, order: Order) {
        let price = order.price;
        let id = order.id;
        self.orders.insert(id, order);
        self.bids.entry(price).or_insert_with(Vec::new).push(id);
    }

    fn add_ask(&mut self, order: Order) {
        let price = order.price;
        let id = order.id;
        self.orders.insert(id, order);
        self.asks.entry(price).or_insert_with(Vec::new).push(id);
    }
}