use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hlc::timestamp::Timestamp;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct TransactionMetadata {
    pub transaction_id: Uuid,
    pub write_timestamp: Timestamp,
}

#[derive(Debug, Clone, Copy)]
pub struct Transaction {
    pub transaction_id: Uuid,
    pub metadata: TransactionMetadata,
    // All reads are performed on this read_timestamp
    // Writes are performed on metadata.write_timestamp.
    // If the write runs into timestamp oracle, then the write timestamp will be bumped.
    pub read_timestamp: Timestamp,
    // TODO: locks, etc
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
// What's stored in the database
pub struct TransactionRecord {
    pub status: TransactionStatus,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TransactionStatus {
    PENDING,
    COMMITTED,
    ABORTED,
}