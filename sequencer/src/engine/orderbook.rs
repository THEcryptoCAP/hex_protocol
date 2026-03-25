use std::collections::BTreeMap;

// 1. Define the Order Struct
#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub user_address: String, 
    pub price: u64,
    pub amount: u64,
    pub is_buy: bool,
}

// 2. Define the OrderBook Struct
pub struct OrderBook {
    // BTreeMap keeps keys (prices) perfectly sorted.
    // Asks (Sellers): We want the lowest price first. Standard BTreeMap sorts ascending.
    pub asks: BTreeMap<u64, Vec<Order>>, 
    
    // Bids (Buyers): We want the highest price first. We will handle this by iterating in reverse.
    pub bids: BTreeMap<u64, Vec<Order>>, 
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
        }
    }

    // 3. The Core Processing Loop
    // Notice `mut order`: We take ownership of the order and mutate its amount if partially filled.
    pub fn process_order(&mut self, mut order: Order) {
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
        // We need to keep track of price levels that get completely emptied to clean up memory
        let mut empty_price_levels = Vec::new();

        // Iterate through asks starting from the lowest price
        for (&ask_price, asks_at_price) in self.asks.iter_mut() {
            // Stop if the ask is too expensive, or our buy order is fully filled
            if ask_price > order.price || order.amount == 0 {
                break; 
            }

            // Iterate through the queue of orders at this specific price level (Time Priority)
            // We use retain() to keep orders that aren't fully filled and drop the rest safely.
            asks_at_price.retain_mut(|ask| {
                if order.amount == 0 {
                    return true; // Keep remaining asks
                }

                let fill_amount = std::cmp::min(order.amount, ask.amount);
                
                // Mutate both orders
                order.amount -= fill_amount;
                ask.amount -= fill_amount;

                println!("Matched: {} units at price {}", fill_amount, ask_price);

                // If ask.amount is > 0, return true to keep it in the Vec.
                // If 0, return false, which automatically drops it from memory.
                ask.amount > 0 
            });

            // If we ate through all asks at this price level, mark it for removal
            if asks_at_price.is_empty() {
                empty_price_levels.push(ask_price);
            }
        }

        // 5. Memory Cleanup
        // Remove empty Vectors from the BTreeMap so we don't waste memory on dead price levels
        for price in empty_price_levels {
            self.asks.remove(&price);
        }
    }

    fn match_sell(&mut self, order: &mut Order) {
        let mut empty_price_levels = Vec::new();

        // Iterate through bids starting from the highest price (reverse order)
        for (&bid_price, bids_at_price) in self.bids.iter_mut().rev() {
            if bid_price < order.price || order.amount == 0 {
                break;
            }

            bids_at_price.retain_mut(|bid| {
                if order.amount == 0 {
                    return true;
                }

                let fill_amount = std::cmp::min(order.amount, bid.amount);
                
                order.amount -= fill_amount;
                bid.amount -= fill_amount;

                println!("Matched: {} units at price {}", fill_amount, bid_price);

                bid.amount > 0
            });

            if bids_at_price.is_empty() {
                empty_price_levels.push(bid_price);
            }
        }

        for price in empty_price_levels {
            self.bids.remove(&price);
        }
    }

    fn add_bid(&mut self, order: Order) {
        // .entry() is highly efficient. If the price level doesn't exist, it allocates a new Vec.
        // If it does, it pushes to the end of the existing Vec (maintaining Time Priority).
        self.bids.entry(order.price).or_insert_with(Vec::new).push(order);
    }

    fn add_ask(&mut self, order: Order) {
        self.asks.entry(order.price).or_insert_with(Vec::new).push(order);
    }
}
