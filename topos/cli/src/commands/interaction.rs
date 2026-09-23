//! Whether a command may ask the person at the terminal, and how it asks.
//!
//! A question has a default, and every run that cannot ask takes it: a
//! pipe, a CI job, `--no-input`. `--yes` answers yes instead. Only a run
//! with a terminal on all three standard streams is ever asked, so a
//! required check never waits on a prompt nobody can see.

use std::io::IsTerminal;

use console::Style;

use super::menu::{self, SelectOption, SelectStep, StepLayout};
use super::render::{paint, RenderOptions};

/// Which of the three standard streams are terminals. Resolved once so the
/// gates that read it are pure functions of it, and every combination is
/// unit-testable.
#[derive(Clone, Copy)]
pub(crate) struct Streams {
    pub(crate) stderr: bool,
    pub(crate) stdout: bool,
    pub(crate) stdin: bool,
}

impl Streams {
    pub(crate) fn detect() -> Self {
        Self {
            stderr: std::io::stderr().is_terminal(),
            stdout: std::io::stdout().is_terminal(),
            stdin: std::io::stdin().is_terminal(),
        }
    }
}

/// How this run answers its questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interaction {
    /// Ask on the terminal.
    Prompt,
    /// `--yes`: answer yes without asking.
    AssumeYes,
    /// Take each question's default without asking.
    Defaults,
}

/// The environment variables that turn prompts off, read once so tests
/// never have to set them on the process.
#[derive(Debug, Clone, Default)]
pub(crate) struct PromptEnv {
    pub(crate) ci: Option<String>,
    pub(crate) gh_prompt_disabled: Option<String>,
}

impl PromptEnv {
    pub(crate) fn from_env() -> Self {
        let read = |name| std::env::var_os(name).map(|value| value.to_string_lossy().into_owned());
        Self {
            ci: read("CI"),
            gh_prompt_disabled: read("GH_PROMPT_DISABLED"),
        }
    }

    /// `CI` set to anything but empty, `false` or `0`, or a non-empty
    /// `GH_PROMPT_DISABLED` (the same switch `gh` honors).
    fn disables_prompts(&self) -> bool {
        let ci = self.ci.as_deref().is_some_and(|value| {
            !(value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false"))
        });
        ci || self
            .gh_prompt_disabled
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    }
}

/// `--yes`, then `--no-input`, then the environment, then the streams.
///
/// stderr has to be a terminal too, not just stdin: the menus draw on and
/// read keys through stderr, and console's `read_key` returns
/// `Key::Unknown` at once on a non-terminal, which a menu loop ignores and
/// reads again, forever.
pub(crate) fn resolve(
    yes: bool,
    no_input: bool,
    env: &PromptEnv,
    streams: &Streams,
) -> Interaction {
    if yes {
        Interaction::AssumeYes
    } else if no_input || env.disables_prompts() {
        Interaction::Defaults
    } else if streams.stdin && streams.stdout && streams.stderr {
        Interaction::Prompt
    } else {
        Interaction::Defaults
    }
}

/// One yes/no question, with what answering it will do.
pub(crate) struct Question {
    pub(crate) title: String,
    /// One line per fact the answer depends on, shown as `·` bullets.
    pub(crate) plan: Vec<String>,
    pub(crate) yes: &'static str,
    pub(crate) no: &'static str,
    /// The answer a run that cannot ask takes. Listed first when asked.
    pub(crate) default: bool,
}

/// Something that can put a [`Question`] to a person.
pub(crate) trait Asker {
    /// `Some(answer)`, `None` when the question was skipped, or an error
    /// when the person interrupted (Ctrl-C) or the terminal failed.
    fn confirm(&mut self, question: &Question) -> Result<Option<bool>, String>;
}

/// Asks on stderr with the install/config single-select.
pub(crate) struct TermAsker;

impl Asker for TermAsker {
    fn confirm(&mut self, question: &Question) -> Result<Option<bool>, String> {
        let (step, yes_at) = question_step(question);
        let header = question_header(question, RenderOptions::stderr());
        Ok(menu::run_select(&header, &step)?.map(|chosen| chosen == yes_at))
    }
}

/// The answer to `question` under `interaction`. A skipped question takes
/// no: it asked whether to do something, and nothing was agreed to.
pub(crate) fn ask(
    interaction: Interaction,
    asker: &mut dyn Asker,
    question: &Question,
) -> Result<bool, String> {
    match interaction {
        Interaction::Prompt => Ok(asker.confirm(question)?.unwrap_or(false)),
        Interaction::AssumeYes => Ok(true),
        Interaction::Defaults => Ok(question.default),
    }
}

/// `┌  title` and one `│  · fact` line per plan entry.
fn question_header(question: &Question, options: RenderOptions) -> Vec<String> {
    let mut header = vec![paint(
        format!("┌  {}", question.title),
        Style::new().bold(),
        options,
    )];
    for item in &question.plan {
        header.push(format!(
            "│  {} {item}",
            paint("·", Style::new().dim(), options)
        ));
    }
    header
}

/// The two choices, the default first and under the cursor, and the index
/// of the yes choice.
fn question_step(question: &Question) -> (SelectStep, usize) {
    let choice = |label, key| SelectOption {
        label,
        hint: String::new(),
        current: false,
        key: Some(key),
    };
    let yes = choice(question.yes, 'y');
    let no = choice(question.no, 'n');
    let (options, yes_at) = if question.default {
        (vec![yes, no], 0)
    } else {
        (vec![no, yes], 1)
    };
    let step = SelectStep {
        title: "",
        keys: "↑↓ · enter · y/n · esc skips",
        options,
        initial: 0,
        layout: StepLayout::Question,
    };
    (step, yes_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streams(stdin: bool, stdout: bool, stderr: bool) -> Streams {
        Streams {
            stderr,
            stdout,
            stdin,
        }
    }

    fn env(ci: Option<&str>, gh: Option<&str>) -> PromptEnv {
        PromptEnv {
            ci: ci.map(str::to_string),
            gh_prompt_disabled: gh.map(str::to_string),
        }
    }

    #[test]
    fn resolve_follows_the_precedence_table() {
        use Interaction::*;
        let tty = streams(true, true, true);
        let quiet = env(None, None);
        let rows: [(bool, bool, PromptEnv, Streams, Interaction); 16] = [
            // --yes beats everything, even --no-input and CI.
            (true, false, quiet.clone(), tty, AssumeYes),
            (true, true, env(Some("true"), None), tty, AssumeYes),
            (
                true,
                false,
                quiet.clone(),
                streams(false, false, false),
                AssumeYes,
            ),
            // --no-input beats a terminal.
            (false, true, quiet.clone(), tty, Defaults),
            // CI, unless empty, false or 0.
            (false, false, env(Some("true"), None), tty, Defaults),
            (false, false, env(Some("1"), None), tty, Defaults),
            (false, false, env(Some("false"), None), tty, Prompt),
            (false, false, env(Some("FALSE"), None), tty, Prompt),
            (false, false, env(Some("0"), None), tty, Prompt),
            (false, false, env(Some(""), None), tty, Prompt),
            // GH_PROMPT_DISABLED, when non-empty.
            (false, false, env(None, Some("1")), tty, Defaults),
            (false, false, env(None, Some("")), tty, Prompt),
            // All three streams must be terminals.
            (false, false, quiet.clone(), tty, Prompt),
            (
                false,
                false,
                quiet.clone(),
                streams(false, true, true),
                Defaults,
            ),
            (
                false,
                false,
                quiet.clone(),
                streams(true, false, true),
                Defaults,
            ),
            (
                false,
                false,
                quiet.clone(),
                streams(true, true, false),
                Defaults,
            ),
        ];
        for (index, (yes, no_input, env, streams, expected)) in rows.iter().enumerate() {
            assert_eq!(
                resolve(*yes, *no_input, env, streams),
                *expected,
                "row {index}: yes={yes} no_input={no_input} {env:?}"
            );
        }
    }

    /// Answers with a fixed result and counts how often it was asked.
    struct Fake {
        answer: Result<Option<bool>, String>,
        asked: usize,
    }

    impl Asker for Fake {
        fn confirm(&mut self, _: &Question) -> Result<Option<bool>, String> {
            self.asked += 1;
            self.answer.clone()
        }
    }

    fn fake(answer: Result<Option<bool>, String>) -> Fake {
        Fake { answer, asked: 0 }
    }

    fn question(default: bool) -> Question {
        Question {
            title: "Build it?".to_string(),
            plan: vec!["it is slow".to_string()],
            yes: "Yes, build it",
            no: "No, skip it",
            default,
        }
    }

    #[test]
    fn a_prompt_takes_the_persons_answer() {
        let q = question(true);
        let mut yes = fake(Ok(Some(true)));
        assert_eq!(ask(Interaction::Prompt, &mut yes, &q), Ok(true));
        let mut no = fake(Ok(Some(false)));
        assert_eq!(ask(Interaction::Prompt, &mut no, &q), Ok(false));
        let mut skip = fake(Ok(None));
        assert_eq!(
            ask(Interaction::Prompt, &mut skip, &q),
            Ok(false),
            "a skip builds nothing, whatever the default"
        );
        let mut abort = fake(Err("interrupted at the prompt".to_string()));
        assert_eq!(
            ask(Interaction::Prompt, &mut abort, &q),
            Err("interrupted at the prompt".to_string())
        );
        assert_eq!(yes.asked + no.asked + skip.asked + abort.asked, 4);
    }

    #[test]
    fn yes_and_defaults_never_ask() {
        let mut asker = fake(Err("must not be asked".to_string()));
        assert_eq!(
            ask(Interaction::AssumeYes, &mut asker, &question(false)),
            Ok(true)
        );
        assert_eq!(
            ask(Interaction::Defaults, &mut asker, &question(true)),
            Ok(true)
        );
        assert_eq!(
            ask(Interaction::Defaults, &mut asker, &question(false)),
            Ok(false)
        );
        assert_eq!(asker.asked, 0);
    }

    #[test]
    fn the_default_is_listed_first_under_the_cursor() {
        let plain = RenderOptions {
            styled: false,
            width: 80,
        };
        let (step, yes_at) = question_step(&question(true));
        assert_eq!(yes_at, 0);
        let lines = menu::render_select(&question_header(&question(true), plain), &step, 0, plain);
        assert_eq!(
            lines,
            [
                "┌  Build it?",
                "│  · it is slow",
                "│ ❯ ● Yes, build it",
                "│   ○ No, skip it",
                "└  ↑↓ · enter · y/n · esc skips",
            ]
        );
        let (step, yes_at) = question_step(&question(false));
        assert_eq!(yes_at, 1);
        assert_eq!(step.options[0].label, "No, skip it");
        assert_eq!(step.options[0].key, Some('n'));
    }
}
