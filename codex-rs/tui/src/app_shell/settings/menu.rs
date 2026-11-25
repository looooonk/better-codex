use super::{ShellState, TuiAppTheme, app_theme_label};
use crate::app_shell::selector::{SelectorState, SelectorValue, SelectorOption};

impl ShellState {
    pub(super) fn open_app_theme_menu(&mut self) {
        use TuiAppTheme::*;
        let options = [TokyoNight, GruvboxDark, CatppuccinMocha, Monochrome].into_iter().map(|theme|
            SelectorOption::new(SelectorValue::AppTheme(theme), app_theme_label(theme), "").current(theme == self.app_theme)
        ).collect();
        self.open_selector(SelectorState::new("Theme", options).settings_menu());
    }

    pub(super) fn open_syntax_theme_menu(&mut self) {
        let mut options = vec![SelectorOption::new(SelectorValue::SyntaxTheme(String::new()), "Default", "").current(self.tui_theme.is_none())];
        for theme in crate::render::highlight::list_available_themes(Some(&self.codex_home)) {
            let current = self.tui_theme.as_ref() == Some(&theme.name);
            options.push(SelectorOption::new(SelectorValue::SyntaxTheme(theme.name.clone()), theme.name, "").current(current));
        }
        options.push(SelectorOption::new(SelectorValue::EditSyntaxTheme, "Enter theme name…", ""));
        self.open_selector(SelectorState::new("Syntax theme", options).settings_menu());
    }
}
