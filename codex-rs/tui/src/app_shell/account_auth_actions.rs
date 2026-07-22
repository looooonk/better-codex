use super::AccountAuthMode;
use super::AccountAuthState;
use super::ShellState;

pub(super) fn open_url(shell: &mut ShellState) {
    let url = shell
        .pending_account_auth
        .as_ref()
        .and_then(AccountAuthState::active_url)
        .map(str::to_string);
    let Some(url) = url else {
        return;
    };
    match webbrowser::open(&url) {
        Ok(()) => {
            if let Some(state) = &mut shell.pending_account_auth {
                state.notice = Some("Opened the sign-in link in your browser.".to_string());
                state.error = None;
            }
        }
        Err(error) => {
            tracing::warn!("failed to open browser for login URL: {error}");
            if let Some(state) = &mut shell.pending_account_auth {
                state.notice = None;
                state.error = Some(format!("Could not open the sign-in link: {error}"));
            }
        }
    }
}

pub(super) fn copy_code(shell: &mut ShellState) {
    let code = shell.pending_account_auth.as_ref().and_then(|state| {
        if let AccountAuthMode::DeviceCode { user_code, .. } = &state.mode {
            Some(user_code.clone())
        } else {
            None
        }
    });
    let Some(code) = code else {
        return;
    };
    match crate::clipboard_copy::copy_to_clipboard(&code) {
        Ok(lease) => {
            shell.clipboard_lease = lease;
            if let Some(state) = &mut shell.pending_account_auth {
                state.notice = Some("Copied the one-time code.".to_string());
                state.error = None;
            }
        }
        Err(error) => {
            if let Some(state) = &mut shell.pending_account_auth {
                state.notice = None;
                state.error = Some(format!("Copy failed: {error}"));
            }
        }
    }
}
