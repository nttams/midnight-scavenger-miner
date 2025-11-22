use ashmaize::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Solution {
    #[serde(rename = "_id")]
    pub id: String, // challenge_id:address
    pub instance_id: String,
    pub challenge_id: String,
    pub address: String,
    pub nonce: String,
    pub hash: String,
    pub preimage: String,
    pub create_time: DateTime<Utc>,
    pub found_time: DateTime<Utc>,
    pub submitted_time: DateTime<Utc>,
    pub time_taken_sec: i32,
    pub total_hashes: i32,
    pub status: String, // "onit" | "found" | "submitted"
    pub submitter_id: String,
}

impl Solution {
    pub fn is_empty(&self) -> bool {
        self.nonce.is_empty() || self.hash.is_empty() || self.preimage.is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Challenge {
    #[serde(default, rename = "_id")]
    pub id: String,

    pub challenge: ChallengeData,
    pub total_challenges: i32,
    pub next_challenge_starts_at: String,

    #[serde(default)]
    pub latest_submission_epoch: i32,
}

// This is not used anymore, kept for reference
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MidnightScavengerChallenge {
    pub code: String,
    pub challenge: ChallengeData,
    pub mining_period_ends: String,
    pub max_day: i32,
    pub total_challenges: i32,
    pub current_day: i32,
    pub next_challenge_starts_at: String,
    pub latest_submission_epoch: i32,
}

impl Challenge {
    pub fn is_late(&self, minute: i64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.latest_submission_epoch as i64 - now <= minute * 60 // less than x minutes left
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChallengeData {
    pub challenge_id: String,
    pub challenge_number: i32,
    pub day: i32,
    pub issued_at: String,
    pub latest_submission: String,
    pub difficulty: String,
    pub no_pre_mine: String,
    pub no_pre_mine_hour: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Address {
    pub tag: String,
    pub address: String,
}

#[derive(Clone)]
pub struct Task {
    pub rom: Arc<Rom>,
    pub addr: String,
    pub challenge: Challenge,
    pub solution: Solution,
}

#[derive(Debug, Clone, Default)]
pub struct MongodbConfig {
    pub mongo_url: String,
    pub mongo_db: String,
    pub coll_config: String,
    pub coll_challenge: String,
    pub coll_address: String,
    pub coll_submit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubmitResponse {
    pub crypto_receipt: CryptoReceipt,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CryptoReceipt {
    pub preimage: String,
    pub timestamp: String,
    pub signature: String,
}
