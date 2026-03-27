// 1. Disable the standard Rust execution model because this runs in a VM
#![no_main]

// Tell the sp1-zkvm macro where the program actually starts
sp1_zkvm::entrypoint!(main);

use serde::{Deserialize, Serialize};

// 2. Define the exact same struct used in your host (zk_client.rs)
// The memory layout must perfectly match for bincode to unpack it.
#[derive(Serialize, Deserialize, Debug)]
pub struct BatchPayload {
    pub previous_state_root: [u8; 32],
    pub new_state_root: [u8; 32],
    // For the hackathon MVP, we represent trade data as raw bytes.
    // In production, this would be a structured list of executed trades.
    pub trade_data: Vec<u8>, 
}

pub fn main() {
    // 3. Read the raw memory buffer provided by the host
    // This is the only way the guest program gets data.
    let serialized_data = sp1_zkvm::io::read_vec();

    // 4. Deserialize the data safely
    // If the sequencer sends corrupted data, the guest panics and the proof fails.
    let payload: BatchPayload = bincode::deserialize(&serialized_data)
        .expect("Critical Failure: Could not deserialize batch payload.");

    // 5. Run the strict verification logic
    let is_valid = verify_state_transition(
        &payload.previous_state_root, 
        &payload.trade_data, 
        &payload.new_state_root
    );
    
    // If the off-chain sequencer tries to steal funds or alter balances incorrectly, 
    // `is_valid` evaluates to false, the VM panics, and no proof is generated.
    if !is_valid {
        panic!("Fraud detected: Invalid state transition!");
    }

    // 6. Commit Public Values to the ZK Proof
    // These specific bytes will be exposed to the `HexSettlement.sol` contract on HashKey Chain.
    sp1_zkvm::io::commit_slice(&payload.previous_state_root);
    sp1_zkvm::io::commit_slice(&payload.new_state_root);
}

/// A deterministic function to verify the trades. 
/// For the hackathon, we create a skeleton structure. 
fn verify_state_transition(_old_root: &[u8; 32], _trades: &[u8], _new_root: &[u8; 32]) -> bool {
    // Hackathon implementation: 
    // Here you would normally reconstruct the Merkle tree from the `old_root`,
    // apply the `trades` to verify conservation of mass (buys = sells), 
    // calculate the resulting root, and compare it to `new_root`.
    
    // We assume true for the MVP scaffolding.
    true 
}