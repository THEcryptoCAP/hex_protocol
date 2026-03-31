#[path = "../prover/mod.rs"]
pub mod prover;
use prover::zk_client::{BatchPayload, HexProver, AccountState, Trade, MerkleProof};
use k256::ecdsa::{SigningKey, Signature, signature::Signer};
use sha2::{Digest, Sha256};
use rand_core::OsRng;

fn hash_account(state: &AccountState) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(state.nonce.to_le_bytes());
    hasher.update(state.base_balance.to_le_bytes());
    hasher.update(state.quote_balance.to_le_bytes());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&hasher.finalize());
    hash
}

fn main() {
    println!("Generating mock BatchPayload for ZK Verification...");

    // 1. Maker Setup
    let signing_key = SigningKey::random(&mut OsRng);
    let maker_pubkey = signing_key.verifying_key().to_sec1_bytes().to_vec();
    
    // Taker Setup (Signature not verified for taker in current circuit logic)
    let taker_pubkey = vec![0u8; 33];

    let maker_state = AccountState { nonce: 0, base_balance: 100, quote_balance: 0 };
    let taker_state = AccountState { nonce: 0, base_balance: 0, quote_balance: 1000 };

    let mut trade = Trade {
        maker_pubkey: maker_pubkey.clone(),
        taker_pubkey: taker_pubkey.clone(),
        amount: 10,
        price: 50,
        maker_signature: vec![],
    };

    // 2. Sign the Order
    let mut hasher = Sha256::new();
    hasher.update(&trade.maker_pubkey);
    hasher.update(&trade.taker_pubkey);
    hasher.update(trade.amount.to_le_bytes());
    hasher.update(trade.price.to_le_bytes());
    let mut order_hash = [0u8; 32];
    order_hash.copy_from_slice(&hasher.finalize());

    let signature: Signature = signing_key.sign(&order_hash);
    trade.maker_signature = signature.to_vec();

    // 3. Merkle Leaf Hashes
    let maker_leaf = hash_account(&maker_state);
    let taker_leaf = hash_account(&taker_state);

    let mut root_hasher = Sha256::new();
    root_hasher.update(maker_leaf);
    root_hasher.update(taker_leaf);
    let mut previous_state_root = [0u8; 32];
    previous_state_root.copy_from_slice(&root_hasher.finalize());

    // Proofs connecting leaves to previous_state_root
    let maker_proof = MerkleProof { sibling_hashes: vec![taker_leaf], is_left: vec![false] };
    let taker_proof = MerkleProof { sibling_hashes: vec![maker_leaf], is_left: vec![true] };

    // 4. Compute Expected Output State
    let mut new_taker_state = taker_state.clone();
    new_taker_state.base_balance += 10;
    new_taker_state.quote_balance -= 500;
    new_taker_state.nonce += 1;
    let new_taker_leaf = hash_account(&new_taker_state);

    let mut new_root_hasher = Sha256::new();
    new_root_hasher.update(maker_leaf); // Taker proof static sibling
    new_root_hasher.update(new_taker_leaf);
    let mut new_state_root = [0u8; 32];
    new_state_root.copy_from_slice(&new_root_hasher.finalize());

    let payload = BatchPayload {
        previous_state_root,
        new_state_root,
        trades: vec![trade],
        maker_states: vec![maker_state],
        maker_proofs: vec![maker_proof],
        taker_states: vec![taker_state],
        taker_proofs: vec![taker_proof],
    };

    println!("Payload cryptographically ready. Booting SP1 HexProver...");

    // The SP1 build artifacts directory
    let elf_path = include_bytes!("../../program/target/riscv32im-succinct-zkvm-elf/release/hex-program");
    let prover = HexProver::new(elf_path);
    
    // We expect this to not panic and successfully return the proof bundle
    match prover.generate_evm_proof(&payload) {
        Ok(_) => println!("Successfully generated and verified the state transition in the ZK-VM!"),
        Err(e) => println!("Error during proof generation: {}", e),
    }
}
