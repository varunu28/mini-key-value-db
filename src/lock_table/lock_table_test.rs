#[cfg(test)]
mod test {
    use core::time;

    use uuid::Uuid;

    use crate::{
        execute::request::{GetRequest, PutRequest, Request, RequestMetadata, RequestUnion},
        hlc::timestamp::Timestamp,
        lock_table::lock_table::{
            LockStateLink, LockTable, LockTableGuard, LockTableGuardLink, WaitingState,
        },
        storage::{serialized_to_value, str_to_key, txn::Txn, Key},
    };

    pub fn assert_holder_txn_id(lock_state_link: LockStateLink, txn_id: Uuid) {
        let lock_state = lock_state_link.as_ref();
        let holder = lock_state.lock_holder.read().unwrap();
        assert!(holder.is_some());
        assert_eq!(lock_state.get_holder_txn_id(), Some(txn_id));
    }

    pub fn get_guard_id(guard_link: LockTableGuardLink) -> Uuid {
        guard_link.as_ref().guard_id
    }

    // Test struture for LockState to assert what is being held by a LockState
    pub struct TestLockState {
        pub queued_writers: Vec<Uuid>,  // guard ids
        pub waiting_readers: Vec<Uuid>, // guard ids
        pub lock_holder: Option<Uuid>,  // txn_id
        pub reservation: Option<Uuid>,  // guard_id
    }

    pub fn assert_lock_table_guard_wait_state(lg: LockTableGuardLink, waiting_state: WaitingState) {
        let state = lg.as_ref().wait_state.read().unwrap();
        assert_eq!(*state, waiting_state);
    }

    #[cfg(test)]
    pub fn assert_lock_state(lock_table: &LockTable, key: Key, test_lock_state: TestLockState) {
        let lock_state_arc = lock_table.get_lock_state(&key).unwrap();
        let lock_state = lock_state_arc.as_ref();
        assert_eq!(
            lock_state.get_queued_writer_ids(),
            test_lock_state.queued_writers
        );
        assert_eq!(
            lock_state.get_waiting_readers_ids(),
            test_lock_state.waiting_readers
        );
        match test_lock_state.lock_holder {
            Some(txn_id) => {
                let holder = &lock_state.lock_holder.read().unwrap().unwrap();
                assert_eq!(txn_id, holder.txn_id)
            }
            None => {
                assert!(lock_state.lock_holder.read().unwrap().is_none())
            }
        }
        match test_lock_state.reservation {
            Some(guard_id) => {
                let reservation = lock_state.reservation.read().unwrap().clone();
                assert!(reservation.is_some());
                let reservation_arc = reservation.unwrap();
                let reservation = reservation_arc.as_ref();
                assert_eq!(guard_id, reservation.guard_id)
            }
            None => {
                assert!(lock_state.reservation.read().unwrap().is_none())
            }
        }
    }

    pub fn create_test_txn() -> Txn {
        Txn::new(Uuid::new_v4(), Timestamp::new(1, 1), Timestamp::new(1, 1))
    }

    pub fn create_test_txn_with_timestamp(timestamp: Timestamp) -> Txn {
        Txn::new(Uuid::new_v4(), timestamp, timestamp)
    }

    pub fn create_test_lock_table_guard(is_read_only: bool) -> (Uuid, Txn, LockTableGuardLink) {
        let txn_id = Uuid::new_v4();
        let txn = Txn::new(txn_id, Timestamp::new(1, 1), Timestamp::new(1, 1));
        let lg = LockTableGuard::new_lock_table_guard_link(txn.clone(), is_read_only);
        (txn_id, txn, lg)
    }

    pub fn create_test_lock_table_guard_with_timestamp(
        timestamp: Timestamp,
        is_read_only: bool,
    ) -> (Uuid, Txn, LockTableGuardLink) {
        let txn_id = Uuid::new_v4();
        let txn = Txn::new(txn_id, timestamp, timestamp);
        let lg = LockTableGuard::new_lock_table_guard_link(txn.clone(), is_read_only);
        (txn_id, txn, lg)
    }

    pub fn create_test_put_request(key: &str) -> (Request, Txn) {
        let request_union = RequestUnion::Put(PutRequest {
            key: str_to_key(key),
            value: serialized_to_value(2),
        });
        let txn_id = Uuid::new_v4();
        let timestamp = Timestamp::new(1, 2);
        let txn = Txn::new(txn_id, timestamp, timestamp);
        (
            Request {
                metadata: RequestMetadata {
                    timestamp: timestamp,
                    txn: txn.clone(),
                },
                request_union,
            },
            txn,
        )
    }

    pub fn create_test_read_request(key: &str, timestamp: Timestamp) -> (Request, Txn) {
        let request_union = RequestUnion::Get(GetRequest {
            key: str_to_key(key),
        });
        let txn_id = Uuid::new_v4();
        let txn = Txn::new(txn_id, timestamp, timestamp);
        (
            Request {
                metadata: RequestMetadata {
                    timestamp: timestamp,
                    txn: txn.clone(),
                },
                request_union,
            },
            txn,
        )
    }

    mod lock_table {
        mod add_discovered_lock {
            use uuid::Uuid;

            use crate::{
                hlc::timestamp::Timestamp,
                lock_table::{
                    lock_table::{LockTable, LockTableGuard, WaitingState},
                    lock_table_test::test::{
                        assert_lock_state, assert_lock_table_guard_wait_state,
                        create_test_lock_table_guard, create_test_txn_with_timestamp, get_guard_id,
                        TestLockState,
                    },
                },
                storage::{
                    str_to_key,
                    txn::{Txn, TxnIntent, TxnMetadata},
                },
            };

            #[test]
            fn empty_lock_table() {
                let lock_table = LockTable::new();
                let key = str_to_key("foo");

                let lock_holder_txn = create_test_txn_with_timestamp(Timestamp::new(1, 1));
                let (_, _, lg) = create_test_lock_table_guard(false);

                lock_table.add_discovered_lock(lg.clone(), lock_holder_txn.to_intent(key.clone()));
                let test_lock_state = TestLockState {
                    queued_writers: Vec::from([get_guard_id(lg.clone())]),
                    waiting_readers: Vec::from([]),
                    lock_holder: Some(lock_holder_txn.txn_id),
                    reservation: None,
                };
                assert_lock_table_guard_wait_state(lg.clone(), WaitingState::Waiting);
                assert_lock_state(&lock_table, key, test_lock_state);
            }

            #[test]
            fn two_guards_add_same_key() {
                let lock_table = LockTable::new();

                let (_, _, lg_1) = create_test_lock_table_guard(true);
                let lock_holder_txn = create_test_txn_with_timestamp(Timestamp::new(1, 1));

                let key = str_to_key("foo");
                lock_table
                    .add_discovered_lock(lg_1.clone(), lock_holder_txn.to_intent(key.clone()));
                assert_lock_table_guard_wait_state(lg_1.clone(), WaitingState::Waiting);

                let test_lock_state = TestLockState {
                    queued_writers: Vec::from([]),
                    waiting_readers: Vec::from([get_guard_id(lg_1.clone())]),
                    lock_holder: Some(lock_holder_txn.txn_id),
                    reservation: None,
                };
                assert_lock_state(&lock_table, key, test_lock_state);
            }
        }

        mod scan_and_enqueue {
            mod write_request {
                use uuid::Uuid;

                use crate::{
                    execute::request::{PutRequest, Request, RequestMetadata, RequestUnion},
                    hlc::timestamp::Timestamp,
                    lock_table::{
                        lock_table::{LockTable, WaitingState},
                        lock_table_test::test::{
                            assert_lock_state, assert_lock_table_guard_wait_state,
                            create_test_lock_table_guard, create_test_put_request,
                            create_test_txn_with_timestamp, get_guard_id, TestLockState,
                        },
                    },
                    storage::{
                        serialized_to_value, str_to_key,
                        txn::{Txn, TxnIntent},
                    },
                };

                #[test]
                fn no_lock_state_for_key() {
                    let key_str = "foo";
                    let key = str_to_key(key_str);
                    let lock_table = LockTable::new();

                    let (request, _) = create_test_put_request(key_str);
                    let (should_wait, lg) = lock_table.scan_and_enqueue(&request);
                    assert_lock_table_guard_wait_state(lg.clone(), WaitingState::DoneWaiting);

                    assert!(!should_wait);
                    let lock_state_option = lock_table.get_lock_state(&key);
                    assert!(lock_state_option.is_none());
                }

                #[test]
                fn queue_write_request_to_held_lock() {
                    let key_str = "foo";
                    let lock_table = LockTable::new();

                    // add discovered lock
                    let (_, _, lg) = create_test_lock_table_guard(false);
                    let lock_holder_txn = create_test_txn_with_timestamp(Timestamp::new(1, 1));
                    lock_table.add_discovered_lock(
                        lg.clone(),
                        lock_holder_txn.to_intent(str_to_key(key_str)),
                    );

                    // enqueue a WRITE request onto the discovered lock
                    let (request, _) = create_test_put_request(key_str);
                    let (should_wait, guard) = lock_table.scan_and_enqueue(&request);
                    assert!(should_wait);
                    assert_lock_table_guard_wait_state(guard.clone(), WaitingState::Waiting);

                    let test_lock_state = TestLockState {
                        queued_writers: Vec::from([
                            get_guard_id(lg.clone()),
                            get_guard_id(guard.clone()),
                        ]),
                        waiting_readers: Vec::from([]),
                        lock_holder: Some(lock_holder_txn.txn_id),
                        reservation: None,
                    };
                    assert_lock_state(&lock_table, str_to_key(key_str), test_lock_state);

                    // enqueue another WRITE request to the locked state
                    let (request, _) = create_test_put_request(key_str);
                    let (should_wait_2, guard_2) = lock_table.scan_and_enqueue(&request);
                    assert!(should_wait_2);
                    assert_lock_table_guard_wait_state(guard_2.clone(), WaitingState::Waiting);

                    let test_lock_state_2 = TestLockState {
                        queued_writers: Vec::from([
                            get_guard_id(lg.clone()),
                            get_guard_id(guard.clone()),
                            get_guard_id(guard_2.clone()),
                        ]),
                        waiting_readers: Vec::from([]),
                        lock_holder: Some(lock_holder_txn.txn_id),
                        reservation: None,
                    };
                    assert_lock_state(&lock_table, str_to_key(key_str), test_lock_state_2);
                }
            }
            mod read_request {
                use crate::{
                    hlc::timestamp::Timestamp,
                    lock_table::{
                        lock_table::{LockTable, WaitingState},
                        lock_table_test::test::{
                            assert_lock_state, assert_lock_table_guard_wait_state,
                            create_test_lock_table_guard,
                            create_test_lock_table_guard_with_timestamp, create_test_read_request,
                            create_test_txn_with_timestamp, get_guard_id, TestLockState,
                        },
                    },
                    storage::{str_to_key, txn::TxnIntent},
                };

                #[test]
                fn queue_read_request_to_held_lock() {
                    let key_str = "foo";
                    let lock_table = LockTable::new();

                    // add discovered lock
                    let lock_timestamp = Timestamp::new(2, 2);
                    let lock_holder_txn = create_test_txn_with_timestamp(Timestamp::new(1, 1));
                    let (txn_id, _, lg) =
                        create_test_lock_table_guard_with_timestamp(lock_timestamp, true);
                    lock_table.add_discovered_lock(
                        lg.clone(),
                        lock_holder_txn.to_intent(str_to_key(key_str)),
                    );

                    // enqueue a READ request onto the discovered lock
                    let (read_request, _) =
                        create_test_read_request(key_str, lock_timestamp.advance_by(1));
                    let (should_wait, read_lg) = lock_table.scan_and_enqueue(&read_request);
                    assert!(should_wait);
                    assert_lock_table_guard_wait_state(read_lg.clone(), WaitingState::Waiting);

                    let test_lock_state = TestLockState {
                        queued_writers: Vec::from([]),
                        waiting_readers: Vec::from([get_guard_id(lg), get_guard_id(read_lg)]),
                        lock_holder: Some(lock_holder_txn.txn_id),
                        reservation: None,
                    };
                    assert_lock_state(&lock_table, str_to_key(key_str), test_lock_state);
                }

                #[test]
                fn read_request_with_smaller_timestamp_than_lock_holder() {
                    let key_str = "foo";
                    let lock_table = LockTable::new();

                    // add discovered lock
                    let lock_timestamp = Timestamp::new(2, 2);
                    let (txn_id, _, lg) = create_test_lock_table_guard(false);
                    let lock_holder_txn = create_test_txn_with_timestamp(lock_timestamp);

                    lock_table.add_discovered_lock(
                        lg.clone(),
                        lock_holder_txn.to_intent(str_to_key(key_str)),
                    );

                    let (read_request, _) =
                        create_test_read_request(key_str, lock_timestamp.decrement_by(1));
                    let (should_wait, lg_1) = lock_table.scan_and_enqueue(&read_request);
                    assert!(!should_wait);
                    assert_lock_table_guard_wait_state(lg_1.clone(), WaitingState::DoneWaiting);

                    let test_lock_state = TestLockState {
                        queued_writers: Vec::from([get_guard_id(lg)]),
                        waiting_readers: Vec::from([]),
                        lock_holder: Some(lock_holder_txn.txn_id),
                        reservation: None,
                    };
                    assert_lock_state(&lock_table, str_to_key(key_str), test_lock_state);
                }
            }
        }

        mod wait_for {
            use std::sync::Arc;

            use tokio::time::{self, sleep, Duration};

            use crate::hlc::timestamp::Timestamp;
            use crate::lock_table;
            use crate::lock_table::lock_table::LockTable;
            use crate::lock_table::lock_table_test::test::{
                create_test_lock_table_guard, create_test_txn_with_timestamp,
            };
            use crate::storage::str_to_key;

            #[tokio::test]
            async fn test() {
                let lock_table = Arc::new(LockTable::new());
                // add discovered lock
                let lock_timestamp = Timestamp::new(2, 2);
                let (txn_id, _, lg) = create_test_lock_table_guard(false);
                let key_str = "foo";

                let lock_holder_txn = create_test_txn_with_timestamp(lock_timestamp);
                lock_table.add_discovered_lock(
                    lg.clone(),
                    lock_holder_txn.to_intent(str_to_key(key_str)),
                );

                let task_1 = tokio::spawn(async move {
                    println!("sleeping!");
                    sleep(Duration::from_millis(100)).await;
                    println!("releasing!");
                });

                let lock_table_2 = lock_table.clone();
                // let task_2 = tokio::spawn(async move {
                //     println!("thread 2 starting sleep!");
                //     sleep(Duration::from_millis(100)).await;
                //     println!("updating lock!");
                //     lock_table_2
                //         .update_locks(str_to_key(key_str), lock_holder_txn)
                //         .await;
                // });
                // tokio::try_join!(task_1, task_2).unwrap();
            }
        }

        mod dequeue {}

        mod update_locks {
            use crate::{
                hlc::timestamp::Timestamp,
                lock_table::{
                    lock_table::{LockTable, WaitingState},
                    lock_table_test::test::{
                        assert_lock_state, assert_lock_table_guard_wait_state,
                        create_test_lock_table_guard, create_test_put_request,
                        create_test_read_request, create_test_txn_with_timestamp, get_guard_id,
                        TestLockState,
                    },
                },
                storage::str_to_key,
            };

            #[tokio::test]
            async fn one_queued_writer() {
                let key_str = "foo";
                let lock_table = LockTable::new();
                let (_, _, lg) = create_test_lock_table_guard(false);
                let lock_holder_txn = create_test_txn_with_timestamp(Timestamp::new(1, 1));

                lock_table.add_discovered_lock(
                    lg.clone(),
                    lock_holder_txn.to_intent(str_to_key(key_str)),
                );
                assert_lock_table_guard_wait_state(lg.clone(), WaitingState::Waiting);

                let can_gc_lock = lock_table
                    .update_locks(str_to_key(key_str), lock_holder_txn)
                    .await;
                assert!(!can_gc_lock);
                assert_lock_table_guard_wait_state(lg.clone(), WaitingState::DoneWaiting);
            }

            #[tokio::test]
            async fn multiple_queued_readers() {
                let key_str = "foo";
                let lock_table = LockTable::new();
                let write_timestamp = Timestamp::new(12, 12);

                let lock_holder_txn = create_test_txn_with_timestamp(write_timestamp);

                let (_, _, lg_1) = create_test_lock_table_guard(true);
                lock_table.add_discovered_lock(
                    lg_1.clone(),
                    lock_holder_txn.to_intent(str_to_key(key_str)),
                );
                assert_lock_table_guard_wait_state(lg_1.clone(), WaitingState::Waiting);

                let (read_req, _) =
                    create_test_read_request(key_str, write_timestamp.advance_by(3));
                let (should_wait, read_lg) = lock_table.scan_and_enqueue(&read_req);
                assert!(should_wait);
                assert_lock_table_guard_wait_state(read_lg.clone(), WaitingState::Waiting);

                let can_gc_lock = lock_table
                    .update_locks(str_to_key(key_str), lock_holder_txn)
                    .await;
                assert!(can_gc_lock);
                assert_lock_table_guard_wait_state(lg_1.clone(), WaitingState::DoneWaiting);
                assert_lock_table_guard_wait_state(read_lg.clone(), WaitingState::DoneWaiting);
            }

            #[tokio::test]
            async fn multiple_queued_writers() {
                let key_str = "foo";
                let lock_table = LockTable::new();
                let write_timestamp = Timestamp::new(12, 12);

                let lock_holder_txn = create_test_txn_with_timestamp(write_timestamp);

                let (_, _, lg_1) = create_test_lock_table_guard(false);
                lock_table.add_discovered_lock(
                    lg_1.clone(),
                    lock_holder_txn.to_intent(str_to_key(key_str)),
                );
                assert_lock_table_guard_wait_state(lg_1.clone(), WaitingState::Waiting);

                let (read_req, _) = create_test_put_request(key_str);
                let (should_wait, lg_2) = lock_table.scan_and_enqueue(&read_req);
                assert!(should_wait);
                assert_lock_table_guard_wait_state(lg_2.clone(), WaitingState::Waiting);

                let can_gc_lock = lock_table
                    .update_locks(str_to_key(key_str), lock_holder_txn)
                    .await;
                assert!(!can_gc_lock);
                assert_lock_table_guard_wait_state(lg_1.clone(), WaitingState::DoneWaiting);
                assert_lock_table_guard_wait_state(lg_2.clone(), WaitingState::Waiting);

                let test_lock_state = TestLockState {
                    queued_writers: Vec::from([get_guard_id(lg_2)]),
                    waiting_readers: Vec::from([]),
                    lock_holder: None,
                    reservation: Some(get_guard_id(lg_1)),
                };
                assert_lock_state(&lock_table, str_to_key(key_str), test_lock_state);
            }
        }
    }
}
