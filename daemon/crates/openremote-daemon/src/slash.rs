//! Every App's own slash commands, and how each one runs when a chat here
//! sends it.
//!
//! An App's built-ins are not all alike once the App runs without its
//! terminal. Some run when the word is sent as the message (`native`); some
//! need the App's own protocol call, because sent as words they would reach
//! the model as text (`driver`); some are things OpenRemote does itself -
//! a new chat, the plugin list, a picker (`console`); and some only make
//! sense in a terminal (`refuse`), which the chat says rather than handing
//! the words to the model. `docs/slash-commands.md` is the inventory these
//! tables are written from, with the source of each row.

use serde::Serialize;

/// How a command runs.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    /// Sent as the message; the App runs it.
    Native,
    /// OpenRemote calls the App's protocol for it.
    Driver,
    /// OpenRemote does it in the app.
    Console,
    /// Refused, with the reason.
    Refuse,
}

/// One built-in command of one App.
#[derive(Clone, Copy, Debug)]
pub struct Builtin {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub argument_hint: Option<&'static str>,
    pub route: Route,
    /// For `driver`, the call (`compact`); for `console`, the app's action
    /// (`new`, `help`, `plugins`).
    pub action: Option<&'static str>,
    /// A `console` command that arrives as text from somewhere that is not
    /// the console (a phone, an automation): `true` when the App runs the
    /// word itself, so it is sent as before; `false` refuses it with `why`.
    pub native_fallback: bool,
    /// Why it is refused here, or what the console row says it does.
    pub why: Option<&'static str>,
    /// The list its argument is picked from (`model`, `effort`,
    /// `permission`), which the composer's argument menu fills.
    pub choices: Option<&'static str>,
}

const fn native(name: &'static str, description: &'static str) -> Builtin {
    Builtin {
        name,
        aliases: &[],
        description,
        argument_hint: None,
        route: Route::Native,
        action: None,
        native_fallback: true,
        why: None,
        choices: None,
    }
}

const fn driver(name: &'static str, action: &'static str, description: &'static str) -> Builtin {
    Builtin {
        route: Route::Driver,
        action: Some(action),
        ..native(name, description)
    }
}

const fn console(name: &'static str, action: &'static str, description: &'static str) -> Builtin {
    Builtin {
        route: Route::Console,
        action: Some(action),
        native_fallback: false,
        ..native(name, description)
    }
}

const fn refuse(name: &'static str, description: &'static str, why: &'static str) -> Builtin {
    Builtin {
        route: Route::Refuse,
        native_fallback: false,
        why: Some(why),
        ..native(name, description)
    }
}

impl Builtin {
    const fn aka(self, aliases: &'static [&'static str]) -> Self {
        Builtin { aliases, ..self }
    }
    const fn hint(self, hint: &'static str) -> Self {
        Builtin {
            argument_hint: Some(hint),
            ..self
        }
    }
    const fn pick(self, choices: &'static str) -> Self {
        Builtin {
            choices: Some(choices),
            ..self
        }
    }
    /// A console command the App also runs from its own word.
    const fn or_native(self) -> Self {
        Builtin {
            native_fallback: true,
            ..self
        }
    }

    /// Whether `word` (without its slash) names this command.
    pub fn answers_to(&self, word: &str) -> bool {
        self.name == word || self.aliases.contains(&word)
    }
}

/// The sentence for a command only a terminal can run.
pub const TERMINAL_ONLY: &str = "only works in the App's own terminal window";

include!("slash_tables.rs");

/// An App's built-ins; empty for an App with no table (a plugin's App).
pub fn builtins(harness: &str) -> &'static [Builtin] {
    match harness {
        "claude" => CLAUDE,
        "codex" => CODEX,
        "opencode" => OPENCODE,
        "pi" => PI,
        "grok" => GROK,
        "agy" => AGY,
        _ => &[],
    }
}

/// Whether the App's run-time list holds its built-ins as well as the
/// user's commands. Claude's does, and Claude runs its built-in first, so
/// a name in both is the built-in. The others list only custom commands,
/// so a listed name is the user's own and runs as the user wrote it.
pub fn lists_builtins(harness: &str) -> bool {
    harness == "claude"
}

/// The command a message starts with: its name (no slash) and the rest.
pub fn leading(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim_start().strip_prefix('/')?;
    let (name, args) = rest
        .split_once(char::is_whitespace)
        .map(|(n, a)| (n, a.trim()))
        .unwrap_or((rest.trim_end(), ""));
    // `/` alone, a path (`/usr/bin`) or a second slash is not a command.
    if name.is_empty() || name.contains('/') {
        return None;
    }
    Some((name, args))
}

/// The built-in a message starts with, and how it runs given what the App
/// itself listed at run time (`listed`). Where the App lists only the
/// user's commands, a listed name is the user's own and runs as written;
/// where it lists its built-ins too (Claude), a row this table would refuse
/// is one the App says it can run without a terminal, so it is sent.
pub fn builtin_for<'a>(
    harness: &str,
    text: &'a str,
    listed: impl Fn(&str) -> bool,
) -> Option<(&'static Builtin, &'a str)> {
    let (name, args) = leading(text)?;
    let found = builtins(harness).iter().find(|b| b.answers_to(name))?;
    if listed(name) && (!lists_builtins(harness) || found.route == Route::Refuse) {
        return None;
    }
    Some((found, args))
}

/// What the daemon does with a message.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Send it as written.
    Send,
    /// Run the App's call for it.
    Driver { action: &'static str, args: String },
    /// Refuse it, in this sentence.
    Refuse(String),
}

/// The daemon's rule for a message on `harness`. `steering` is a message
/// folded into a turn still running, where only plain words make sense.
pub fn verdict(
    harness: &str,
    app_name: &str,
    text: &str,
    steering: bool,
    listed: impl Fn(&str) -> bool,
) -> Verdict {
    let Some((builtin, args)) = builtin_for(harness, text, listed) else {
        return Verdict::Send;
    };
    let name = builtin.name;
    let refused = |why: &str| Verdict::Refuse(format!("/{name} {why}"));
    match builtin.route {
        Route::Native => Verdict::Send,
        Route::Console if builtin.native_fallback && !steering => Verdict::Send,
        Route::Console => refused(&format!(
            "runs in OpenRemote, not through {app_name}: pick it from the / menu in the chat"
        )),
        _ if steering => {
            refused("can't join a turn that is still running - send it once the turn has finished")
        }
        Route::Driver => Verdict::Driver {
            action: builtin.action.unwrap_or(name),
            args: args.to_string(),
        },
        Route::Refuse => refused(builtin.why.unwrap_or(TERMINAL_ONLY)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_word_is_a_command_and_a_path_is_not() {
        assert_eq!(leading("/compact"), Some(("compact", "")));
        assert_eq!(
            leading("  /review  the diff "),
            Some(("review", "the diff"))
        );
        assert_eq!(leading("/usr/bin is a folder"), None);
        assert_eq!(leading("/ alone"), None);
        assert_eq!(leading("no command"), None);
    }

    #[test]
    fn every_table_is_well_formed() {
        for harness in ["claude", "codex", "opencode", "pi", "grok", "agy"] {
            let mut seen = std::collections::BTreeSet::new();
            for b in builtins(harness) {
                for word in std::iter::once(&b.name).chain(b.aliases.iter()) {
                    assert!(seen.insert(*word), "{harness}: /{word} listed twice");
                    assert!(!word.starts_with('/'), "{harness}: /{word} keeps its slash");
                }
                assert!(
                    !b.description.is_empty(),
                    "{harness}: /{} says nothing",
                    b.name
                );
                assert!(
                    !b.description.contains('\u{2014}'),
                    "{harness}: /{} em dash",
                    b.name
                );
                match b.route {
                    Route::Driver | Route::Console => {
                        assert!(b.action.is_some(), "{harness}: /{} has no action", b.name)
                    }
                    Route::Refuse => {
                        assert!(b.why.is_some(), "{harness}: /{} has no reason", b.name)
                    }
                    Route::Native => {}
                }
            }
        }
    }

    #[test]
    fn every_console_row_names_an_action_the_console_runs() {
        for harness in ["claude", "codex", "opencode", "pi", "grok", "agy"] {
            for b in builtins(harness)
                .iter()
                .filter(|b| b.route == Route::Console)
            {
                let action = b.action.unwrap();
                assert!(
                    CONSOLE_ACTIONS.contains(&action),
                    "{harness}: /{} runs {action}",
                    b.name
                );
            }
        }
    }

    #[test]
    fn a_terminal_only_command_is_refused_with_its_reason() {
        let Some(vim) = CLAUDE.iter().find(|b| b.route == Route::Refuse) else {
            return;
        };
        let text = format!("/{}", vim.name);
        match verdict("claude", "Claude Code", &text, false, |_| false) {
            Verdict::Refuse(sentence) => {
                assert!(sentence.starts_with(&text), "{sentence}");
                assert!(sentence.contains(vim.why.unwrap()), "{sentence}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_users_own_command_wins_where_the_app_lists_only_its_users() {
        // Codex lists only custom entries, so a listed /compact is the
        // user's and goes as written.
        if builtins("codex").iter().any(|b| b.name == "compact") {
            assert_eq!(
                verdict("codex", "Codex", "/compact", false, |n| n == "compact"),
                Verdict::Send
            );
            assert!(matches!(
                verdict("codex", "Codex", "/compact", false, |_| false),
                Verdict::Driver {
                    action: "compact",
                    ..
                }
            ));
        }
    }

    #[test]
    fn claude_runs_what_it_lists_even_where_the_table_refuses() {
        assert!(matches!(
            verdict("claude", "Claude Code", "/theme", false, |_| false),
            Verdict::Refuse(_)
        ));
        assert_eq!(
            verdict("claude", "Claude Code", "/theme", false, |n| n == "theme"),
            Verdict::Send
        );
        // A console row stays the console's, listed or not.
        assert!(matches!(
            verdict("claude", "Claude Code", "/copy", false, |n| n == "copy"),
            Verdict::Refuse(_)
        ));
    }

    #[test]
    fn a_steer_carries_only_words_and_native_commands() {
        if builtins("codex").iter().any(|b| b.name == "compact") {
            assert!(matches!(
                verdict("codex", "Codex", "/compact", true, |_| false),
                Verdict::Refuse(s) if s.contains("once the turn has finished")
            ));
        }
        assert_eq!(
            verdict("codex", "Codex", "keep going", true, |_| false),
            Verdict::Send
        );
    }

    #[test]
    fn a_console_command_with_no_native_word_is_refused_from_elsewhere() {
        for harness in ["claude", "codex", "opencode", "pi", "grok", "agy"] {
            for b in builtins(harness)
                .iter()
                .filter(|b| b.route == Route::Console)
            {
                let text = format!("/{}", b.name);
                let got = verdict(harness, "the App", &text, false, |_| false);
                if b.native_fallback {
                    assert_eq!(got, Verdict::Send, "{harness} {text}");
                } else {
                    assert!(matches!(got, Verdict::Refuse(_)), "{harness} {text}");
                }
            }
        }
    }
}
