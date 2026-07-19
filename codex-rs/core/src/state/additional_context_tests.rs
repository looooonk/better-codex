use super::*;
use pretty_assertions::assert_eq;

#[test]
fn merge_limits_the_number_of_stored_fragments() {
    let mut store = AdditionalContextStore::default();
    let values = (0..MAX_ADDITIONAL_CONTEXT_ITEMS + 10)
        .map(|index| {
            (
                format!("context_{index:02}"),
                AdditionalContextEntry {
                    value: format!("value {index}"),
                    kind: AdditionalContextKind::Untrusted,
                },
            )
        })
        .collect();

    assert_eq!(store.merge(values).len(), MAX_ADDITIONAL_CONTEXT_ITEMS);
    assert_eq!(store.values.len(), MAX_ADDITIONAL_CONTEXT_ITEMS);
}
