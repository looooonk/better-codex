use super::*;
use pretty_assertions::assert_eq;

#[test]
fn channel_selection_parses_one_based_lists_and_rejects_invalid_input() {
    assert_eq!(
        parse_channels(Some("2, 4")).unwrap(),
        Some(vec![
            NonZeroU16::new(2).unwrap(),
            NonZeroU16::new(4).unwrap()
        ])
    );
    assert_eq!(parse_channels(None).unwrap(), None);
    for input in ["", "0", "-1", "one", "1,", "65536"] {
        assert!(parse_channels(Some(input)).is_err(), "{input}");
    }
    assert!(parse_channels(Some(&vec!["1"; 33].join(","))).is_err());
}
