const ARGUMENT_SLASH_COMMANDS: [&str; 2] = ["/copy", "/goal"];
const NO_ARGUMENT_SLASH_COMMANDS: [&str; 5] = ["/clear", "/exit", "/login", "/logout", "/vim"];

pub(super) fn script() -> String {
    let argument_commands = ARGUMENT_SLASH_COMMANDS
        .iter()
        .map(|command| format!("'{command}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let no_argument_commands = NO_ARGUMENT_SLASH_COMMANDS
        .iter()
        .map(|command| format!("'{command}'"))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"set nomodeline
silent execute 'edit ' . fnameescape($BETTER_CODEX_VIM_INPUT)

function! s:IsBetterCodexInputBuffer() abort
  return !empty($BETTER_CODEX_VIM_INPUT) && expand('%:p') ==# fnamemodify($BETTER_CODEX_VIM_INPUT, ':p')
endfunction

let s:RustWhitespaceAtom = '\%(\%u0009\|\%u000a\|\%u000b\|\%u000c\|\%u000d\|\%u0020\|\%u0085\|\%u00a0\|\%u1680\|\%u2000\|\%u2001\|\%u2002\|\%u2003\|\%u2004\|\%u2005\|\%u2006\|\%u2007\|\%u2008\|\%u2009\|\%u200a\|\%u2028\|\%u2029\|\%u202f\|\%u205f\|\%u3000\)'

function! s:CodexSubmit() abort
  try
    silent write
    call writefile(['submit'], $BETTER_CODEX_VIM_ACTION)
    qall!
  catch
    call delete($BETTER_CODEX_VIM_ACTION)
    echoerr v:exception
  endtry
endfunction

function! s:RefreshSlashCommandHighlight() abort
  if !s:IsBetterCodexInputBuffer()
    return
  endif

  silent! syntax clear BetterCodexSlashCommand
  let l:text = join(getline(1, '$'), "\n")
  let l:start = matchend(l:text, '^' . s:RustWhitespaceAtom . '*')
  if l:start < 0 || l:start == strlen(l:text)
    return
  endif

  let l:remaining = strpart(l:text, l:start)
  if strcharpart(l:remaining, 0, 1) !=# '/'
    return
  endif
  let l:command_prefix = strcharpart(l:remaining, 0, 8)
  let l:boundary = match(l:command_prefix, s:RustWhitespaceAtom)
  let l:command = l:boundary < 0 ? l:command_prefix : strpart(l:command_prefix, 0, l:boundary)
  let l:valid = index([{argument_commands}], l:command) >= 0
  if !l:valid && index([{no_argument_commands}], l:command) >= 0
    let l:tail = strpart(l:remaining, strlen(l:command))
    let l:valid = matchend(l:tail, '^' . s:RustWhitespaceAtom . '*') == strlen(l:tail)
  endif
  if !l:valid
    return
  endif

  let l:prefix = strpart(l:text, 0, l:start)
  let l:line = count(l:prefix, "\n") + 1
  let l:column = l:start - strridx(l:prefix, "\n")

  execute printf('syntax match BetterCodexSlashCommand /\%%%dl\%%%dc%s/ containedin=ALL', l:line, l:column, escape(l:command, '/\'))
endfunction

function! s:ConfigureBetterCodexInputBuffer() abort
  if !s:IsBetterCodexInputBuffer()
    return
  endif

  setlocal noswapfile nobackup nowritebackup noundofile nomodeline fileformat=unix
  command! -buffer Codex call <SID>CodexSubmit()
  nnoremap <silent><buffer> <C-J> :Codex<CR>
  highlight default link BetterCodexSlashCommand Statement
  call s:RefreshSlashCommandHighlight()
endfunction

augroup BetterCodexVimInputBridge
  autocmd!
  autocmd VimEnter * call <SID>ConfigureBetterCodexInputBuffer()
  autocmd BufWinEnter * call <SID>ConfigureBetterCodexInputBuffer()
  autocmd TextChanged,TextChangedI * call <SID>RefreshSlashCommandHighlight()
augroup END

call s:ConfigureBetterCodexInputBuffer()
echo 'Ctrl-J or :Codex sends this buffer to Better Codex'
"#
    )
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
