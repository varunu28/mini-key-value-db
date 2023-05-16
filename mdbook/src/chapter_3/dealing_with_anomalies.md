# Dealing with Anomalies

CockroachDB outlined its strategy to deal with transaction conflicts in section 3.3 of [its research paper](https://www.cockroachlabs.com/guides/thank-you/?pdf=/pdf/cockroachdb-the-resilient-geo-distributed-sql-database-sigmod-2020.pdf). My database uses the same concurrency control techniques outlined in that section.

### Commit Timestamp

Each transaction performs its reads and writes at its commit timestamp. This is what guarantees the serializability of transactions. This section covers how a transaction determines its commit timestamp.

A transaction has a read timestamp and a write timestamp. The read/write timestamps are initialized to the timestamp when the transaction is created, which is guaranteed to be unique. The transaction stores the most recent write timestamp as part of the write intent. When the transaction commits, the final write timestamp is used as the commit timestamp.

Usually, the write timestamp for a transaction won’t change. But in some situations, it is required to be bumped. Let’s look at these scenarios.

### Dealing with conflicts

#### Read-write conflict

If a write detects that a read has been performed with a greater timestamp, the write will need to advance its timestamp past the read’s timestamp.

The most recent read timestamp for each key is tracked by the database with the Timestamp Oracle (CockroachDB calls it the TimestampCache). This will be covered in another section.

#### Write-write conflict

Write-write conflict happens when a write runs into another write, which could be either committed or uncommitted.

There are two scenarios to look at

- the write runs into an uncommitted write intent: the write will need to wait for the other transaction to finalize
- the write runs into a committed write intent: the transaction performing the write needs to advance its timestamp past the committed write intent’s timestamp.

#### Write-read conflict

Write-read happens when a read runs into an uncommitted write. Two scenarios could occur:

- the uncommitted write intent has a **bigger** timestamp: the read ignores the intent and returns the key with the biggest timestamp less than the read timestamp.
- the uncommitted write intent has a **smaller** timestamp: the read needs to wait for the transaction associated with the write intent to be finalized (aborted or committed)