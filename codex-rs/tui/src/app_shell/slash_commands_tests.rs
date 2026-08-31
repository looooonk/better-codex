use super::*;
use crate::app_shell::transcript_copy::ResponseOrdinal;
use pretty_assertions::assert_eq;

#[test]
fn copy_accepts_an_optional_response_ordinal() {
    assert_eq!(
        [
            LocalSlashCommand::parse("/copy"),
            LocalSlashCommand::parse(" /copy 1 "),
            LocalSlashCommand::parse("/copy 5"),
            LocalSlashCommand::parse("/copy 9"),
        ],
        [
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Response(
                ResponseOrdinal::LATEST,
            ))),
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Response(
                ResponseOrdinal::LATEST,
            ))),
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Response(
                ResponseOrdinal::from_ascii_digit('5').expect("5 should be a response ordinal"),
            ))),
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Response(
                ResponseOrdinal::from_ascii_digit('9').expect("9 should be a response ordinal"),
            ))),
        ]
    );
}

#[test]
fn invalid_copy_arguments_stay_local_for_usage_feedback() {
    assert_eq!(
        [
            LocalSlashCommand::parse("/copy 0"),
            LocalSlashCommand::parse("/copy 10"),
            LocalSlashCommand::parse("/copy second"),
            LocalSlashCommand::parse("/copy 2 extra"),
        ],
        [
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Invalid)),
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Invalid)),
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Invalid)),
            Some(LocalSlashCommand::Copy(CopyResponseRequest::Invalid)),
        ]
    );
}
