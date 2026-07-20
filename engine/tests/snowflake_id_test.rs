use std::collections::HashSet;
use vertex_engine::snowflake_id::{SnowFlakeGenerator, MAX_DATACENTER_ID, MAX_MACHINE_ID};

#[test]
fn new_valid_min_ids() {
    let mut generator = SnowFlakeGenerator::new(0, 0);
    let id = generator.next_id();
    assert!(id > 0);
}

#[test]
fn new_valid_max_ids() {
    let mut generator = SnowFlakeGenerator::new(MAX_MACHINE_ID, MAX_DATACENTER_ID);
    let id = generator.next_id();
    assert!(id > 0);
}

#[test]
#[should_panic(expected = "machine_id must be between 0")]
fn new_panics_machine_id_out_of_range() {
    let _gen = SnowFlakeGenerator::new(MAX_MACHINE_ID + 1, 0);
}

#[test]
#[should_panic(expected = "datacenter_id must be between 0")]
fn new_panics_datacenter_id_out_of_range() {
    let _gen = SnowFlakeGenerator::new(0, MAX_DATACENTER_ID + 1);
}

#[test]
fn next_id_monotonically_increasing() {
    let mut generator = SnowFlakeGenerator::new(1, 1);
    let mut prev = generator.next_id();
    for _ in 0..100 {
        let current = generator.next_id();
        assert!(current > prev, "IDs must be monotonically increasing");
        prev = current;
    }
}

#[test]
fn ids_encode_machine_and_datacenter_bits() {
    let mut generator = SnowFlakeGenerator::new(5, 10);
    let id = generator.next_id();
    let machine_id = (id >> 12) & 0x1F;
    let datacenter_id = (id >> 17) & 0x1F;
    assert_eq!(machine_id, 5, "machine_id should be encoded in bits 12-16");
    assert_eq!(
        datacenter_id, 10,
        "datacenter_id should be encoded in bits 17-21"
    );
}

#[test]
fn different_generators_produce_different_ids() {
    let mut generator1 = SnowFlakeGenerator::new(1, 1);
    let mut generator2 = SnowFlakeGenerator::new(2, 2);
    let id1 = generator1.next_id();
    let id2 = generator2.next_id();
    assert_ne!(id1, id2);
}

#[test]
fn multiple_ids_no_collisions() {
    let mut generator = SnowFlakeGenerator::new(1, 1);
    let mut seen = HashSet::new();
    for _ in 0..1000 {
        let id = generator.next_id();
        assert!(seen.insert(id), "Duplicate ID generated: {}", id);
    }
}
