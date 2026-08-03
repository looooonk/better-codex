use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

pub(super) const VIM_ACTION_ENV: &str = "BETTER_CODEX_VIM_ACTION";
pub(super) const VIM_INPUT_ENV: &str = "BETTER_CODEX_VIM_INPUT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VimEditorCommand {
    pub(super) program: PathBuf,
    pub(super) args: Vec<String>,
}

#[derive(Debug)]
pub(super) struct VimEditorEnvironment {
    pub(super) visual: Option<String>,
    pub(super) editor: Option<String>,
    pub(super) path: Option<OsString>,
    pub(super) current_dir: PathBuf,
}

impl VimEditorEnvironment {
    pub(super) fn current() -> Result<Self> {
        Ok(Self {
            visual: env::var_os("VISUAL").map(|value| value.to_string_lossy().into_owned()),
            editor: env::var_os("EDITOR").map(|value| value.to_string_lossy().into_owned()),
            path: env::var_os("PATH"),
            current_dir: env::current_dir().wrap_err("failed to resolve the current directory")?,
        })
    }
}

pub(super) fn resolve_vim_editor(environment: &VimEditorEnvironment) -> Result<VimEditorCommand> {
    for (variable, configured) in [
        ("VISUAL", environment.visual.as_deref()),
        ("EDITOR", environment.editor.as_deref()),
    ] {
        let Some(configured) = configured else {
            continue;
        };
        if configured.trim().is_empty() {
            continue;
        }
        let parts = parse_editor_command(configured)
            .ok_or_else(|| eyre!("failed to parse {variable} as an editor command"))?;
        let Some((program, args)) = parts.split_first() else {
            continue;
        };
        let Some(program_path) = resolve_program(program, environment) else {
            if is_vim_program_name(program) {
                return Err(eyre!(
                    "{variable} selects `{program}`, but that Vim or Neovim executable was not found"
                ));
            }
            if is_env_program_name(program) {
                return Err(eyre!(
                    "{variable} selects `{program}` as an editor wrapper, but that executable was not found"
                ));
            }
            continue;
        };
        if !configured_editor_is_vim(program, args, &program_path, environment, variable)? {
            continue;
        }
        return Ok(VimEditorCommand {
            program: program_path,
            args: args.to_vec(),
        });
    }

    for program in ["vim", "nvim"] {
        if let Some(program) = resolve_program(program, environment) {
            return Ok(VimEditorCommand {
                program,
                args: Vec::new(),
            });
        }
    }
    Err(eyre!(
        "neither Vim nor Neovim was found; set VISUAL or EDITOR to one of them, or install `nvim` or `vim` on PATH"
    ))
}

#[cfg(not(windows))]
fn parse_editor_command(command: &str) -> Option<Vec<String>> {
    shlex::split(command)
}

#[cfg(windows)]
fn parse_editor_command(command: &str) -> Option<Vec<String>> {
    Some(winsplit::split(command))
}

fn is_vim_program_name(program: &str) -> bool {
    is_vim_program_path(Path::new(program))
}

fn is_env_program_name(program: &str) -> bool {
    Path::new(program)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("env"))
}

fn is_vim_program_path(program: &Path) -> bool {
    program
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            matches!(name.as_str(), "vi" | "vim" | "nvim")
                || name.starts_with("vim-")
                || name.starts_with("nvim-")
                || name.starts_with("vim.")
                || name.starts_with("nvim.")
        })
}

fn resolved_program_is_vim(program: &Path) -> bool {
    is_vim_program_path(program)
        || program
            .canonicalize()
            .is_ok_and(|program| is_vim_program_path(&program))
}

#[cfg(not(windows))]
fn is_path_environment_variable(name: &str) -> bool {
    name == "PATH"
}

#[cfg(windows)]
fn is_path_environment_variable(name: &str) -> bool {
    name.eq_ignore_ascii_case("PATH")
}

fn configured_editor_is_vim(
    program: &str,
    args: &[String],
    program_path: &Path,
    environment: &VimEditorEnvironment,
    variable: &str,
) -> Result<bool> {
    if is_vim_program_name(program) || resolved_program_is_vim(program_path) {
        return Ok(true);
    }
    if !is_env_program_name(program) {
        return Ok(false);
    }
    let mut nested_program_index = 0;
    let mut path_was_unset = false;
    while let Some(argument) = args.get(nested_program_index) {
        match argument.as_str() {
            "--" => {
                nested_program_index += 1;
                break;
            }
            "-u" | "--unset" => {
                let Some(name) = args.get(nested_program_index + 1) else {
                    return Err(eyre!(
                        "{variable} has an incomplete `{argument}` option for `env`"
                    ));
                };
                path_was_unset |= is_path_environment_variable(name);
                nested_program_index += 2;
            }
            argument if argument.starts_with("--unset=") => {
                let name = argument.trim_start_matches("--unset=");
                path_was_unset |= is_path_environment_variable(name);
                nested_program_index += 1;
            }
            argument if argument.starts_with("-u") && argument.len() > 2 => {
                path_was_unset |= is_path_environment_variable(&argument[2..]);
                nested_program_index += 1;
            }
            argument if argument.starts_with('-') => {
                return Err(eyre!(
                    "{variable} uses unsupported `env` option `{argument}` for Vim input"
                ));
            }
            _ => break,
        }
    }
    let assignment_start = nested_program_index;
    while args
        .get(nested_program_index)
        .is_some_and(|argument| argument.contains('='))
    {
        nested_program_index += 1;
    }
    let Some(nested_program) = args.get(nested_program_index) else {
        return Ok(false);
    };
    let wrapper_path = args[assignment_start..nested_program_index]
        .iter()
        .filter_map(|argument| argument.split_once('='))
        .filter(|(name, _)| is_path_environment_variable(name))
        .map(|(_, value)| OsString::from(value))
        .next_back();
    let nested_environment = VimEditorEnvironment {
        visual: None,
        editor: None,
        path: wrapper_path.or_else(|| {
            if path_was_unset {
                None
            } else {
                environment.path.clone()
            }
        }),
        current_dir: environment.current_dir.clone(),
    };
    let Some(nested_program_path) = resolve_program(nested_program, &nested_environment) else {
        if is_vim_program_name(nested_program) {
            return Err(eyre!(
                "{variable} selects `{nested_program}` through `env`, but that Vim or Neovim executable was not found"
            ));
        }
        return Ok(false);
    };
    Ok(is_vim_program_name(nested_program) || resolved_program_is_vim(&nested_program_path))
}

#[cfg(unix)]
pub(super) fn resolve_program(
    program: &str,
    environment: &VimEditorEnvironment,
) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let is_executable = |path: &Path| {
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        let resolved = if program_path.is_absolute() {
            program_path.to_path_buf()
        } else {
            environment.current_dir.join(program_path)
        };
        return is_executable(&resolved).then_some(resolved);
    }

    environment.path.as_deref().and_then(|path| {
        env::split_paths(path)
            .map(|directory| {
                if directory.as_os_str().is_empty() {
                    environment.current_dir.join(program)
                } else if directory.is_absolute() {
                    directory.join(program)
                } else {
                    environment.current_dir.join(directory).join(program)
                }
            })
            .find(|candidate| is_executable(candidate))
    })
}

#[cfg(windows)]
pub(super) fn resolve_program(
    program: &str,
    environment: &VimEditorEnvironment,
) -> Option<PathBuf> {
    which::which_in(
        program,
        environment.path.as_deref(),
        &environment.current_dir,
    )
    .ok()
}

pub(super) fn build_editor_command(
    editor: &VimEditorCommand,
    bridge_path: &Path,
    input_path: &Path,
    submit_path: &Path,
) -> Command {
    let mut command = Command::new(&editor.program);
    command
        .args(&editor.args)
        .arg("-n")
        .arg("-i")
        .arg("NONE")
        .arg("-S")
        .arg(bridge_path)
        .env(VIM_ACTION_ENV, submit_path)
        .env(VIM_INPUT_ENV, input_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    command
}
