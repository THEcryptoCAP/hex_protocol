// This code acts as the "Host". It takes the batched trades from your engine, serializes them, and feeds them into the ZK Virtual Machine.
// To ensure it is highly auditable and memory-safe, we avoid passing massive amounts of raw data by value (which duplicates memory).
// Instead, we use references and strict Result types for error handling so the sequencer never silently panics during a trade execution.
use sp1_sdk::{ProverClient, SP1Stdin, SP1ProofWithPublicValues};
use serde::{Deserialize, Serialize};

// This represents the state transition data we send to the ZK-VM
#[derive(Serialize, Deserialize, Debug)]
pub struct BatchPayload {
    pub previous_state_root: [u8; 32],
    pub new_state_root: [u8; 32],
    // In production, this contains the compressed list of matched trades
    pub trade_data: Vec<u8>, 
}

pub struct HexProver {
    client: ProverClient,
    // The path to the compiled RISC-V binary of your ZK guest program
    elf_path: &'static [u8], 
}

impl HexProver {
    /// Initializes the SP1 Prover Client
    pub fn new(elf_path: &'static [u8]) -> Self {
        println!("Initializing SP1 ZK Prover Client...");
        Self {
            client: ProverClient::new(),
            elf_path,
        }
    }
