use std::fmt;
use std::fmt::Write;

use colored::Colorize;
use dialoguer::{console::Term, theme::ColorfulTheme, Input, MultiSelect, Password, Select};
use error_stack::{IntoReport, Result, ResultExt};

#[derive(Debug)]
pub struct DialoguerError;

impl fmt::Display for DialoguerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Dialoguer error")
    }
}

impl std::error::Error for DialoguerError {}

pub type DialoguerResult<T> = error_stack::Result<T, DialoguerError>;

#[derive(Debug, Clone)]
pub struct Dialoguer;

impl Dialoguer {
    pub fn multiselect<T>(
        prompt_text: String,
        items: Vec<T>,
        default: Option<&Vec<bool>>,
        force_select: bool,
    ) -> DialoguerResult<Vec<usize>>
    where
        T: ToString + Clone,
    {
        let waiting_response = true;
        while waiting_response {
            let colorful_theme = &ColorfulTheme::default();
            let mut multi_select = MultiSelect::with_theme(colorful_theme);
            let mut dialog = multi_select.with_prompt(&prompt_text).items(&items);

            if let Some(def) = default {
                dialog = dialog.defaults(def);
            }
            let result = dialog
                .interact_on_opt(&Term::stderr())
                .into_report()
                .change_context(DialoguerError)?
                .ok_or(DialoguerError)
                .into_report()?;
            if force_select && result.is_empty() {
                println!(
                    "{}",
                    "No option selected, you should pick at least 1 by hitting space bar".red()
                );
            } else {
                return Ok(result);
            }
        }
        Ok(vec![])
    }

    pub fn select<T>(
        prompt_text: String,
        items: Vec<T>,
        default: Option<usize>,
    ) -> Result<usize, DialoguerError>
    where
        T: ToString + Clone,
    {
        let colorful_theme = &ColorfulTheme::default();
        let mut select = Select::with_theme(colorful_theme);
        let mut dialog = select.with_prompt(&prompt_text).items(&items);

        if let Some(def) = default {
            dialog = dialog.default(def);
        } else {
            dialog = dialog.default(0);
        }

        Ok(dialog
            .interact_on_opt(&Term::stderr())
            .into_report()
            .change_context(DialoguerError)?
            .ok_or(DialoguerError)
            .into_report()?)
    }

    pub fn select_yes_or_no(prompt_text: String) -> Result<bool, DialoguerError> {
        let colorful_theme = &ColorfulTheme::default();
        let mut select = Select::with_theme(colorful_theme);
        let dialog = select
            .with_prompt(&prompt_text)
            .item("yes")
            .item("no")
            .default(0);
        let opt = dialog
            .interact_on_opt(&Term::stderr())
            .into_report()
            .change_context(DialoguerError)?
            .ok_or(DialoguerError)
            .into_report()?;

        Ok(opt == 0)
    }

    pub fn input(prompt_text: String) -> Result<String, DialoguerError> {
        let colorful_theme = &ColorfulTheme::default();
        let mut input = Input::with_theme(colorful_theme);
        let dialog: String = input
            .with_prompt(&prompt_text)
            .interact_text()
            .into_report()
            .change_context(DialoguerError)?;

        Ok(dialog)
    }

    pub fn password(prompt_text: String) -> Result<String, DialoguerError> {
        let colorful_theme = &ColorfulTheme::default();
        let mut input = Password::with_theme(colorful_theme);
        let dialog: String = input
            .with_prompt(&prompt_text)
            .with_confirmation("Confirm password", "Password mismatch")
            .interact()
            .into_report()
            .change_context(DialoguerError)?;

        Ok(dialog)
    }
}
