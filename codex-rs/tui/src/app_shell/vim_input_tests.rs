use super::*;
use pretty_assertions::assert_eq;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(unix)]
fn fake_vim(body: &str) -> (tempfile::TempDir, PathBuf) {
    let temp_dir = tempfile::tempdir().expect("fake Vim directory");
    let program = temp_dir.path().join("fake-vim");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nset -eu\ninput=$BETTER_CODEX_VIM_INPUT\ntest \"$1\" = --from-user\nshift\nbridge=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = -S ]; then shift; bridge=$1; fi\n  shift\ndone\ntest -f \"$bridge\"\n{body}\n"
        ),
    )
    .expect("fake Vim script");
    let mut permissions = fs::metadata(&program)
        .expect("fake Vim metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&program, permissions).expect("executable fake Vim");
    (temp_dir, program)
}

#[cfg(unix)]
fn fake_editor_binary(directory: &Path, name: &str) -> PathBuf {
    let program = directory.join(name);
    fs::write(&program, "#!/bin/sh\nexit 0\n").expect("fake editor script");
    let mut permissions = fs::metadata(&program)
        .expect("fake editor metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&program, permissions).expect("executable fake editor");
    program
}

#[cfg(unix)]
fn editor_environment(directory: &Path) -> VimEditorEnvironment {
    VimEditorEnvironment {
        visual: None,
        editor: None,
        path: Some(OsString::from(directory)),
        current_dir: directory.to_path_buf(),
    }
}

#[cfg(unix)]
#[test]
fn editor_resolution_honors_configuration_and_preserves_arguments() {
    let temp_dir = tempfile::tempdir().expect("editor directory");
    let custom_bin = temp_dir.path().join("custom-bin");
    fs::create_dir(&custom_bin).expect("custom editor directory");
    let nvim = fake_editor_binary(temp_dir.path(), "nvim");
    let vim = fake_editor_binary(temp_dir.path(), "vim");
    let vim_basic = fake_editor_binary(temp_dir.path(), "vim.basic");
    let nvim_work = fake_editor_binary(temp_dir.path(), "nvim-work");
    let _custom_nvim = fake_editor_binary(&custom_bin, "nvim");
    let env = fake_editor_binary(temp_dir.path(), "env");

    let mut configured_environment = editor_environment(temp_dir.path());
    configured_environment.visual =
        Some(format!("'{}' --cmd 'set number'", nvim.to_string_lossy()));
    configured_environment.editor = Some("vim -f".to_string());
    let configured =
        resolve_vim_editor(&configured_environment).expect("VISUAL Neovim should resolve");
    let mut editor_fallback_environment = editor_environment(temp_dir.path());
    editor_fallback_environment.visual = Some("code --wait".to_string());
    editor_fallback_environment.editor = Some("vim -f".to_string());
    let editor_fallback = resolve_vim_editor(&editor_fallback_environment)
        .expect("unsupported VISUAL should fall through to EDITOR");
    let mut empty_visual_environment = editor_environment(temp_dir.path());
    empty_visual_environment.visual = Some("   ".to_string());
    empty_visual_environment.editor = Some("nvim --clean".to_string());
    let empty_visual_fallback = resolve_vim_editor(&empty_visual_environment)
        .expect("empty VISUAL should fall through to EDITOR");
    let vi = temp_dir.path().join("vi");
    symlink(&vim_basic, &vi).expect("Vim alias symlink");
    let mut alias_environment = editor_environment(temp_dir.path());
    alias_environment.visual = Some("vi -f".to_string());
    let alias = resolve_vim_editor(&alias_environment).expect("Vim alias should resolve");
    let custom_editor = temp_dir.path().join("my-editor");
    symlink(&vim_basic, &custom_editor).expect("custom Vim alias symlink");
    let mut custom_alias_environment = editor_environment(temp_dir.path());
    custom_alias_environment.visual = Some("my-editor -f".to_string());
    let custom_alias = resolve_vim_editor(&custom_alias_environment)
        .expect("a custom alias to a dot-suffixed Vim binary should resolve");
    let mut named_wrapper_environment = editor_environment(temp_dir.path());
    named_wrapper_environment.visual = Some("nvim-work --clean".to_string());
    let named_wrapper = resolve_vim_editor(&named_wrapper_environment)
        .expect("named Neovim wrapper should resolve");
    let mut env_wrapper_environment = editor_environment(temp_dir.path());
    env_wrapper_environment.visual = Some("env NVIM_APPNAME=work nvim --clean".to_string());
    let env_wrapper =
        resolve_vim_editor(&env_wrapper_environment).expect("env-wrapped Neovim should resolve");
    let mut env_path_environment = editor_environment(temp_dir.path());
    env_path_environment.visual = Some(format!(
        "env PATH={} nvim --clean",
        custom_bin.to_string_lossy()
    ));
    let env_path_wrapper = resolve_vim_editor(&env_path_environment)
        .expect("env-wrapped Neovim should resolve against its assigned PATH");
    let mut env_unset_environment = editor_environment(temp_dir.path());
    env_unset_environment.visual = Some("env -u VIMINIT -- nvim --clean".to_string());
    let env_unset_wrapper = resolve_vim_editor(&env_unset_environment)
        .expect("env options should preserve Neovim resolution");
    let mut env_unset_path_environment = editor_environment(temp_dir.path());
    env_unset_path_environment.visual =
        Some(format!("env -u PATH '{}' --clean", nvim.to_string_lossy()));
    let env_unset_path_wrapper = resolve_vim_editor(&env_unset_path_environment)
        .expect("unsetting PATH should permit an absolute Neovim path");
    let mut env_reassign_path_environment = editor_environment(temp_dir.path());
    env_reassign_path_environment.visual = Some(format!(
        "env -uPATH PATH={} nvim --clean",
        custom_bin.to_string_lossy()
    ));
    let env_reassign_path_wrapper = resolve_vim_editor(&env_reassign_path_environment)
        .expect("a PATH assignment should replace an earlier unset");
    let mut lowercase_path_environment = editor_environment(temp_dir.path());
    lowercase_path_environment.visual = Some("env path=/missing nvim".to_string());
    let lowercase_path_wrapper = resolve_vim_editor(&lowercase_path_environment)
        .expect("lowercase path should not replace PATH on Unix");
    let path_fallback = resolve_vim_editor(&editor_environment(temp_dir.path()))
        .expect("PATH fallback should resolve");

    assert_eq!(
        vec![
            configured,
            editor_fallback,
            empty_visual_fallback,
            alias,
            custom_alias,
            named_wrapper,
            env_wrapper,
            env_path_wrapper,
            env_unset_wrapper,
            env_unset_path_wrapper,
            env_reassign_path_wrapper,
            lowercase_path_wrapper,
            path_fallback,
        ],
        vec![
            VimEditorCommand {
                program: nvim.clone(),
                args: vec!["--cmd".to_string(), "set number".to_string()],
            },
            VimEditorCommand {
                program: vim.clone(),
                args: vec!["-f".to_string()],
            },
            VimEditorCommand {
                program: nvim.clone(),
                args: vec!["--clean".to_string()],
            },
            VimEditorCommand {
                program: vi,
                args: vec!["-f".to_string()],
            },
            VimEditorCommand {
                program: custom_editor,
                args: vec!["-f".to_string()],
            },
            VimEditorCommand {
                program: nvim_work,
                args: vec!["--clean".to_string()],
            },
            VimEditorCommand {
                program: env.clone(),
                args: vec![
                    "NVIM_APPNAME=work".to_string(),
                    "nvim".to_string(),
                    "--clean".to_string(),
                ],
            },
            VimEditorCommand {
                program: env.clone(),
                args: vec![
                    format!("PATH={}", custom_bin.to_string_lossy()),
                    "nvim".to_string(),
                    "--clean".to_string(),
                ],
            },
            VimEditorCommand {
                program: env.clone(),
                args: vec![
                    "-u".to_string(),
                    "VIMINIT".to_string(),
                    "--".to_string(),
                    "nvim".to_string(),
                    "--clean".to_string(),
                ],
            },
            VimEditorCommand {
                program: env.clone(),
                args: vec![
                    "-u".to_string(),
                    "PATH".to_string(),
                    nvim.to_string_lossy().into_owned(),
                    "--clean".to_string(),
                ],
            },
            VimEditorCommand {
                program: env.clone(),
                args: vec![
                    "-uPATH".to_string(),
                    format!("PATH={}", custom_bin.to_string_lossy()),
                    "nvim".to_string(),
                    "--clean".to_string(),
                ],
            },
            VimEditorCommand {
                program: env,
                args: vec!["path=/missing".to_string(), "nvim".to_string(),],
            },
            VimEditorCommand {
                program: vim,
                args: Vec::new(),
            },
        ]
    );
}

#[cfg(unix)]
#[test]
fn editor_resolution_reports_invalid_explicit_configuration() {
    let temp_dir = tempfile::tempdir().expect("editor directory");
    fake_editor_binary(temp_dir.path(), "vim");
    fake_editor_binary(temp_dir.path(), "env");

    let mut malformed_environment = editor_environment(temp_dir.path());
    malformed_environment.visual = Some("nvim 'unterminated".to_string());
    let malformed = resolve_vim_editor(&malformed_environment)
        .expect_err("malformed VISUAL should fail")
        .to_string();
    let mut missing_environment = editor_environment(temp_dir.path());
    missing_environment.visual = Some("nvim --clean".to_string());
    let missing = resolve_vim_editor(&missing_environment)
        .expect_err("missing configured Neovim should fail")
        .to_string();
    let mut missing_wrapped_environment = editor_environment(temp_dir.path());
    missing_wrapped_environment.visual = Some("env NVIM_APPNAME=work nvim".to_string());
    let missing_wrapped = resolve_vim_editor(&missing_wrapped_environment)
        .expect_err("missing env-wrapped Neovim should fail")
        .to_string();
    fake_editor_binary(temp_dir.path(), "nvim");
    let mut overridden_path_environment = editor_environment(temp_dir.path());
    overridden_path_environment.visual = Some("env PATH=/missing nvim".to_string());
    let overridden_path = resolve_vim_editor(&overridden_path_environment)
        .expect_err("env-wrapped Neovim should honor its missing PATH")
        .to_string();
    let mut unset_path_environment = editor_environment(temp_dir.path());
    unset_path_environment.visual = Some("env -u PATH nvim".to_string());
    let unset_path = resolve_vim_editor(&unset_path_environment)
        .expect_err("unsetting PATH should require an absolute editor path")
        .to_string();
    let missing_wrapper_dir = temp_dir.path().join("missing-wrapper-bin");
    fs::create_dir(&missing_wrapper_dir).expect("missing wrapper directory");
    fake_editor_binary(&missing_wrapper_dir, "nvim");
    let mut missing_outer_wrapper_environment = editor_environment(&missing_wrapper_dir);
    missing_outer_wrapper_environment.visual = Some("env nvim".to_string());
    let missing_outer_wrapper = resolve_vim_editor(&missing_outer_wrapper_environment)
        .expect_err("a missing configured env wrapper should not silently fall back")
        .to_string();

    assert_eq!(
        (
            malformed,
            missing,
            missing_wrapped,
            overridden_path,
            unset_path,
            missing_outer_wrapper,
        ),
        (
            "failed to parse VISUAL as an editor command".to_string(),
            "VISUAL selects `nvim`, but that Vim or Neovim executable was not found".to_string(),
            "VISUAL selects `nvim` through `env`, but that Vim or Neovim executable was not found"
                .to_string(),
            "VISUAL selects `nvim` through `env`, but that Vim or Neovim executable was not found"
                .to_string(),
            "VISUAL selects `nvim` through `env`, but that Vim or Neovim executable was not found"
                .to_string(),
            "VISUAL selects `env` as an editor wrapper, but that executable was not found"
                .to_string(),
        )
    );
}

#[test]
fn editor_command_preserves_user_arguments_without_disabling_config() {
    let editor = VimEditorCommand {
        program: PathBuf::from("nvim"),
        args: vec!["--cmd".to_string(), "set number".to_string()],
    };
    let bridge = PathBuf::from("bridge.vim");
    let input = PathBuf::from("input.md");
    let marker = PathBuf::from("submit");
    let command = build_editor_command(&editor, &bridge, &input, &marker);
    let command = command.as_std();
    let args = command.get_args().map(OsString::from).collect::<Vec<_>>();
    let explicit_env = command
        .get_envs()
        .filter_map(|(key, value)| value.map(|value| (key.to_os_string(), value.to_os_string())))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(
        (command.get_program().to_os_string(), args, explicit_env,),
        (
            OsString::from("nvim"),
            vec![
                OsString::from("--cmd"),
                OsString::from("set number"),
                OsString::from("-n"),
                OsString::from("-i"),
                OsString::from("NONE"),
                OsString::from("-S"),
                bridge.into_os_string(),
            ],
            std::collections::BTreeMap::from([
                (OsString::from(VIM_ACTION_ENV), marker.into_os_string()),
                (OsString::from(VIM_INPUT_ENV), input.into_os_string()),
            ]),
        )
    );
}

#[cfg(unix)]
#[tokio::test]
async fn process_exit_and_marker_contract_controls_the_outcome() {
    let scripts = [
        "printf 'first\n界  \n\n' > \"$input\"\nprintf 'submit\n' > \"$BETTER_CODEX_VIM_ACTION\"",
        "printf 'edited draft\n' > \"$input\"",
        ":",
        "printf 'must not send\n' > \"$input\"\nprintf 'submit\n' > \"$BETTER_CODEX_VIM_ACTION\"\nexit 7",
    ];
    let mut outcomes = Vec::new();
    for script in scripts {
        let (_temp_dir, program) = fake_vim(script);
        let editor = VimEditorCommand {
            program,
            args: vec!["--from-user".to_string()],
        };
        outcomes.push(
            run_with_editor(VimInputRequest::empty(ThreadId::new()), &editor)
                .await
                .expect("fake Vim should run"),
        );
    }
    assert_eq!(
        outcomes,
        vec![
            VimInputOutcome::Submit("first\n界  \n".to_string()),
            VimInputOutcome::ReturnDraft("edited draft".to_string()),
            VimInputOutcome::ReturnDraft(String::new()),
            VimInputOutcome::Cancelled,
        ]
    );
}

#[cfg(unix)]
async fn assert_real_editor_bridge(editor_name: &str) {
    let environment = VimEditorEnvironment::current().expect("current editor environment");
    let Some(program) = resolve_program(editor_name, &environment) else {
        return;
    };
    let temp_dir = tempfile::tempdir().expect("Vim bridge directory");
    let home = temp_dir.path().join("home");
    let xdg_config_home = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&home).expect("Vim home directory");
    fs::create_dir_all(xdg_config_home.join("nvim")).expect("Neovim config directory");
    let input = temp_dir.path().join("input.md");
    let marker = temp_dir.path().join("submit");
    let probe_output = temp_dir.path().join("probe-output");
    let bridge = temp_dir.path().join("bridge.vim");
    let probe = temp_dir.path().join("probe.vim");
    fs::write(&input, "before\n").expect("Vim input fixture");
    fs::write(&bridge, bridge::script()).expect("Vim bridge fixture");
    let (config_path, config) = if editor_name == "nvim" {
        (
            xdg_config_home.join("nvim/init.lua"),
            r#"vim.g.better_codex_user_config = "nvim"
vim.cmd("highlight BetterCodexSlashCommand ctermfg=123")
vim.cmd([[
  augroup BetterCodexUserConfigProbe
    autocmd!
    autocmd VimEnter * nnoremap <buffer> <C-J> :echo 'user mapping'<CR>
  augroup END
]])
"#,
        )
    } else {
        (
            home.join(".vimrc"),
            r#"let g:better_codex_user_config = 'vim'
highlight BetterCodexSlashCommand ctermfg=123
augroup BetterCodexUserConfigProbe
  autocmd!
  autocmd VimEnter * nnoremap <buffer> <C-J> :echo 'user mapping'<CR>
augroup END
"#,
        )
    };
    fs::write(config_path, config).expect("user editor config");
    fs::write(
        &probe,
        r#"function! s:IsBetterCodexSlashCommand(lines) abort
  silent %delete _
  call setline(1, a:lines)
  doautocmd <nomodeline> TextChanged
  let l:lnum = nextnonblank(1)
  if l:lnum == 0
    return 0
  endif
  let l:column = match(getline(l:lnum), '/') + 1
  if l:column <= 0
    return 0
  endif
  return synIDattr(synID(l:lnum, l:column, 1), 'name') ==# 'BetterCodexSlashCommand'
endfunction

doautocmd <nomodeline> VimEnter
let exact_groups = []
for command in ['/clear', '/exit', '/login', '/logout', '/vim']
  call add(exact_groups, s:IsBetterCodexSlashCommand([command]))
endfor
let invalid_groups = []
for command in ['/vim extra', '/goalpost', '/unknown', '/Clear']
  call add(invalid_groups, s:IsBetterCodexSlashCommand([command]))
endfor
let results = []
call add(results, 'config=' . get(g:, 'better_codex_user_config', 'missing'))
call add(results, 'mapping=' . maparg('<C-J>', 'n'))
call add(results, 'style=' . synIDattr(hlID('BetterCodexSlashCommand'), 'fg', 'cterm'))
call add(results, 'exact=' . join(exact_groups, ','))
call add(results, 'goal=' . s:IsBetterCodexSlashCommand(['/goal objective']))
call add(results, 'leading=' . s:IsBetterCodexSlashCommand(['', '  /goal objective']))
let nbsp = nr2char(0xa0)
let unicode_groups = [
      \ s:IsBetterCodexSlashCommand([nbsp . '/clear' . nbsp]),
      \ s:IsBetterCodexSlashCommand([nbsp . '/goal' . nbsp . 'objective']),
      \ s:IsBetterCodexSlashCommand([nbsp . '/vim' . nbsp . 'extra']),
      \ ]
call add(results, 'unicode=' . join(unicode_groups, ','))
call add(results, 'invalid=' . join(invalid_groups, ','))
call add(results, 'large=' . s:IsBetterCodexSlashCommand([repeat('x', 100000)]))
call writefile(results, $BETTER_CODEX_VIM_PROBE_OUTPUT)
silent %delete _
call setline(1, ['from bridge', 'second line'])
Codex
"#,
    )
    .expect("Vim bridge probe");

    let editor = VimEditorCommand {
        program,
        args: vec![if editor_name == "nvim" {
            "--headless".to_string()
        } else {
            "--not-a-term".to_string()
        }],
    };
    let mut command = build_editor_command(&editor, &bridge, &input, &marker);
    command
        .args([
            "-c",
            "execute 'source ' . fnameescape($BETTER_CODEX_VIM_PROBE)",
        ])
        .env("BETTER_CODEX_VIM_PROBE", &probe)
        .env("BETTER_CODEX_VIM_PROBE_OUTPUT", &probe_output)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("EXINIT")
        .env_remove("VIMINIT")
        .env_remove("NVIM_APPNAME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await
        .unwrap_or_else(|_| panic!("real {editor_name} bridge exceeded ten seconds"))
        .unwrap_or_else(|err| panic!("real {editor_name} bridge should launch: {err}"));
    let probe = fs::read_to_string(probe_output).unwrap_or_else(|err| {
        panic!(
            "{editor_name} bridge probe should be recorded: {err}; status={}; stdout={}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    });
    assert_eq!(
        (
            output.status.success(),
            probe,
            read_vim_input(&input).expect("bridge should save UTF-8 input"),
            is_submit_marker(&marker).expect("bridge marker should be readable"),
        ),
        (
            true,
            format!(
                "config={editor_name}\nmapping=:Codex<CR>\nstyle=123\nexact=1,1,1,1,1\ngoal=1\nleading=1\nunicode=1,1,0\ninvalid=0,0,0,0\nlarge=0\n"
            ),
            "from bridge\nsecond line".to_string(),
            true,
        )
    );
}

#[cfg(unix)]
#[tokio::test]
async fn real_vim_bridge_loads_user_config_lints_commands_and_submits() {
    assert_real_editor_bridge("vim").await;
}

#[cfg(unix)]
#[tokio::test]
async fn real_neovim_bridge_loads_user_config_lints_commands_and_submits() {
    assert_real_editor_bridge("nvim").await;
}

#[test]
fn input_reader_preserves_text_and_rejects_invalid_content() {
    let temp_dir = tempfile::tempdir().expect("Vim input directory");
    let input = temp_dir.path().join("input.md");
    let exact_limit = temp_dir.path().join("exact-limit.md");
    let oversized = temp_dir.path().join("oversized.md");
    let invalid = temp_dir.path().join("invalid.md");
    fs::write(&input, b"first  \r\nsecond \r\n\r\n").expect("Vim input fixture");
    let mut exact_limit_contents = vec![b'x'; MAX_COMPOSER_BYTES];
    exact_limit_contents.extend_from_slice(b"\r\n");
    fs::write(&exact_limit, exact_limit_contents).expect("exact-limit Vim input fixture");
    fs::write(&oversized, vec![b'x'; MAX_COMPOSER_BYTES.saturating_add(1)])
        .expect("oversized Vim input fixture");
    fs::write(&invalid, [0xff]).expect("invalid Vim input fixture");

    assert_eq!(
        (
            read_vim_input(&input).expect("valid Vim input"),
            read_vim_input(&exact_limit)
                .expect("editor newline should not exceed the input limit")
                .len(),
            read_vim_input(&oversized)
                .expect_err("oversized input should fail")
                .to_string(),
            read_vim_input(&invalid)
                .expect_err("invalid input should fail")
                .to_string()
                .contains("not valid UTF-8"),
        ),
        (
            "first  \nsecond \n".to_string(),
            MAX_COMPOSER_BYTES,
            input_too_large_message(MAX_COMPOSER_BYTES.saturating_add(1)),
            true,
        )
    );
}
