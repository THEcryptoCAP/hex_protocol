use std::collections::{BTreeMap, HashMap};

// 1. Define the Order Struct
#[allow(dead_code)]
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
    // Notice `mut order`: We take ownership of the order and mutate its amount if partially filled.
    pub fn place_order(&mut self, mut order: Order) {
        if order.is_buy {
            self.match_buy(&mut order);
            // If the order wasn't fully filled, add the remainder to the bids book
            if order.amount > 0 {
                self.add_bid(order);
            }
        } else {
            self.match_sell(&mut order);
            // If the order wasn't fully filled, add the remainder to the asks book
            if order.amount > 0 {
                self.add_ask(order);
            }
        }
    }

    // 4. Matching a Buy Order Against Existing Asks
    fn match_buy(&mut self, order: &mut Order) {
        let mut empty_price_levels = Vec::new();
        let mut fully_filled_ids = Vec::new();

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
    }

    fn match_sell(&mut self, order: &mut Order) {
        let mut empty_price_levels = Vec::new();
        let mut fully_filled_ids = Vec::new();

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