//! A minimal JSON-RPC client for talking to a local validator.
//!
//! Deliberately hand-rolled rather than pulling `solana-client`: that crate's
//! 4.1 releases do not currently build, their transaction-status types
//! disagreeing with themselves about which `wincode` they were compiled
//! against. Nothing here needs the other ninety percent of it -- send a
//! transaction, read an account, ask the time -- and a hundred lines of HTTP
//! cannot break in that particular way.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

pub struct Rpc {
    url: String,
    agent: ureq::Agent,
}

/// Why a transaction did not land. The distinction matters: a test that expects
/// a refusal must not be satisfied by a network hiccup.
#[derive(Debug)]
pub enum RpcError {
    /// The cluster rejected the transaction, and this is what it said.
    Rejected(String),
    /// Something went wrong reaching or parsing the node.
    Transport(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Rejected(message) => write!(formatter, "rejected: {message}"),
            RpcError::Transport(message) => write!(formatter, "transport: {message}"),
        }
    }
}

impl Rpc {
    pub fn new(url: &str) -> Rpc {
        Rpc {
            url: url.to_string(),
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(30))
                .build(),
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let response: Value = self
            .agent
            .post(&self.url)
            .send_json(body)
            .map_err(|error| RpcError::Transport(error.to_string()))?
            .into_json()
            .map_err(|error| RpcError::Transport(error.to_string()))?;

        if let Some(error) = response.get("error") {
            return Err(RpcError::Rejected(error.to_string()));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| RpcError::Transport("no result field".into()))
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self.call("getHealth", json!([])), Ok(value) if value == "ok")
    }

    pub fn slot(&self) -> Result<u64, RpcError> {
        self.call("getSlot", json!([]))?
            .as_u64()
            .ok_or_else(|| RpcError::Transport("slot is not a number".into()))
    }

    /// The chain's own clock, which is what every deadline in the protocol
    /// reads. It drifts from the host's, so tests must never use the host's.
    pub fn block_time(&self) -> Result<i64, RpcError> {
        let slot = self.slot()?;
        self.call("getBlockTime", json!([slot]))?
            .as_i64()
            .ok_or_else(|| RpcError::Transport("no block time yet".into()))
    }

    pub fn latest_blockhash(&self) -> Result<Hash, RpcError> {
        let result = self.call(
            "getLatestBlockhash",
            json!([{"commitment": "confirmed"}]),
        )?;
        let encoded = result["value"]["blockhash"]
            .as_str()
            .ok_or_else(|| RpcError::Transport("no blockhash".into()))?;
        encoded
            .parse()
            .map_err(|_| RpcError::Transport("unparseable blockhash".into()))
    }

    pub fn rent_exempt_minimum(&self, space: usize) -> Result<u64, RpcError> {
        self.call("getMinimumBalanceForRentExemption", json!([space]))?
            .as_u64()
            .ok_or_else(|| RpcError::Transport("rent is not a number".into()))
    }

    pub fn account_data(&self, address: &Pubkey) -> Result<Vec<u8>, RpcError> {
        use base64::Engine;
        let result = self.call(
            "getAccountInfo",
            json!([address.to_string(), {"encoding": "base64", "commitment": "confirmed"}]),
        )?;
        let encoded = result["value"]["data"][0]
            .as_str()
            .ok_or_else(|| RpcError::Transport(format!("{address} does not exist")))?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| RpcError::Transport(error.to_string()))
    }

    pub fn account_exists(&self, address: &Pubkey) -> bool {
        self.account_data(address).is_ok()
    }

    pub fn lamports(&self, address: &Pubkey) -> u64 {
        self.call(
            "getBalance",
            json!([address.to_string(), {"commitment": "confirmed"}]),
        )
        .ok()
        .and_then(|result| result["value"].as_u64())
        .unwrap_or_default()
    }

    pub fn airdrop(&self, address: &Pubkey, lamports: u64) -> Result<(), RpcError> {
        let signature = self
            .call("requestAirdrop", json!([address.to_string(), lamports]))?
            .as_str()
            .ok_or_else(|| RpcError::Transport("no signature".into()))?
            .to_string();
        self.await_signature(&signature)
    }

    /// Sends and waits, with preflight on so a bad instruction is reported as a
    /// rejection rather than a silent drop.
    pub fn send(&self, transaction: &Transaction) -> Result<(), RpcError> {
        use base64::Engine;
        let wire = bincode::serialize(transaction)
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(wire);
        let signature = self
            .call(
                "sendTransaction",
                json!([encoded, {"encoding": "base64", "preflightCommitment": "confirmed"}]),
            )?
            .as_str()
            .ok_or_else(|| RpcError::Transport("no signature".into()))?
            .to_string();
        self.await_signature(&signature)
    }

    fn await_signature(&self, signature: &str) -> Result<(), RpcError> {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            let result = self.call(
                "getSignatureStatuses",
                json!([[signature], {"searchTransactionHistory": true}]),
            )?;
            let status = &result["value"][0];
            if !status.is_null() {
                if let Some(error) = status.get("err") {
                    if !error.is_null() {
                        return Err(RpcError::Rejected(error.to_string()));
                    }
                }
                if status["confirmationStatus"] == "confirmed"
                    || status["confirmationStatus"] == "finalized"
                {
                    return Ok(());
                }
            }
            if Instant::now() > deadline {
                return Err(RpcError::Transport("never confirmed".into()));
            }
            sleep(Duration::from_millis(250));
        }
    }

    /// Signs and sends in one step.
    pub fn submit(
        &self,
        instructions: &[solana_sdk::instruction::Instruction],
        payer: &Keypair,
        signers: &[&Keypair],
    ) -> Result<(), RpcError> {
        let blockhash = self.latest_blockhash()?;
        let mut all: Vec<&Keypair> = vec![payer];
        for signer in signers {
            if signer.pubkey() != payer.pubkey() {
                all.push(signer);
            }
        }
        let transaction =
            Transaction::new_signed_with_payer(instructions, Some(&payer.pubkey()), &all, blockhash);
        self.send(&transaction)
    }
}
