use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeSeq;

use super::serialized_size_exceeds;

const MAX_FIELD_BYTES: usize = 4 * 1024;

struct OversizedThenFlag<'a> {
    later_serialized: &'a AtomicBool,
}

impl Serialize for OversizedThenFlag<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(/*len*/ Some(2))?;
        sequence.serialize_element(&"x".repeat(MAX_FIELD_BYTES * 2))?;
        self.later_serialized.store(true, Ordering::Release);
        sequence.serialize_element("later")?;
        sequence.end()
    }
}

#[test]
fn detects_exact_serialized_size_boundary() {
    assert!(!serialized_size_exceeds(&"abc", /*max_bytes*/ 5).expect("serialize value at limit"));
    assert!(serialized_size_exceeds(&"abc", /*max_bytes*/ 4).expect("serialize value over limit"));
}

#[test]
fn stops_serializing_after_the_cap() {
    let later_serialized = AtomicBool::new(false);

    assert!(
        serialized_size_exceeds(
            &OversizedThenFlag {
                later_serialized: &later_serialized,
            },
            MAX_FIELD_BYTES,
        )
        .expect("measure oversized value")
    );
    assert!(!later_serialized.load(Ordering::Acquire));
}
