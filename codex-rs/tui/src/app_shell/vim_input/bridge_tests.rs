use super::ARGUMENT_SLASH_COMMANDS;
use super::NO_ARGUMENT_SLASH_COMMANDS;
use super::script;
use crate::app_shell::slash_commands::SLASH_COMMANDS;
use pretty_assertions::assert_eq;

#[test]
fn generated_vim_bridge() {
    insta::assert_snapshot!("vim_bridge", script());
}

#[test]
fn bridge_commands_match_the_local_command_registry() {
    let mut bridge_with_arguments = ARGUMENT_SLASH_COMMANDS.to_vec();
    let mut bridge_without_arguments = NO_ARGUMENT_SLASH_COMMANDS.to_vec();
    let mut registry_with_arguments = SLASH_COMMANDS
        .iter()
        .filter(|definition| definition.accepts_arguments())
        .map(|definition| definition.name())
        .collect::<Vec<_>>();
    let mut registry_without_arguments = SLASH_COMMANDS
        .iter()
        .filter(|definition| !definition.accepts_arguments())
        .map(|definition| definition.name())
        .collect::<Vec<_>>();
    bridge_with_arguments.sort_unstable();
    bridge_without_arguments.sort_unstable();
    registry_with_arguments.sort_unstable();
    registry_without_arguments.sort_unstable();

    assert_eq!(
        (bridge_with_arguments, bridge_without_arguments),
        (registry_with_arguments, registry_without_arguments)
    );
}
