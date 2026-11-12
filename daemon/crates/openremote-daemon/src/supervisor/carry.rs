//! A moved chat's conversation: it follows the chat to its new folder where
//! the harness can find it there (claude's file copied beside the new
//! folder, codex's thread resumed with the new cwd). Where it can't, the
//! chat starts a fresh conversation in the new folder and its first plain
//! message after the move carries the chat so far, condensed and marked as
//! context. The transcript keeps the message as the user wrote it.

use std::path::{Path, PathBuf};

use openremote_core::{EventPayload, Session, SessionId};

use super::{Supervisor, composer};

/// The most of the earlier chat a fresh conversation is handed, in bytes.
const CARRY_LIMIT: usize = 40_000;
/// The same for a harness that takes each prompt on its command line (one
/// process per prompt): Windows caps a whole command line at 32,767
/// characters, and the prompt shares it with the harness's own flags.
const CARRY_LIMIT_ARGV: usize = 12_000;

impl Supervisor {
    /// Whether the harness's own conversation follows the chat to
    /// `target` - made resumable there when it does.
    pub(super) async fn follows(&self, session: &Session, target: Option<&Path>) -> bool {
        // An earlier move already left the conversation behind, or the
        // harness never named one: the next start is fresh either way.
        if session.carry_transcript.is_some() {
            return false;
        }
        let Some(conversation) = session.harness_session_ref.clone() else {
            return false;
        };
        // A test's stand-in for a harness keeps no conversations, and the
        // machine's real ones are never touched on its behalf.
        if self.is_fixture(&session.harness) {
            return false;
        }
        let Some(backend) = self.backend(&session.harness) else {
            return false;
        };
        let folder: PathBuf = target.map(Path::to_path_buf).unwrap_or_else(Self::home_dir);
        tokio::task::spawn_blocking(move || backend.follow_conversation(&conversation, &folder))
            .await
            .unwrap_or(false)
    }

    /// The sentence a move note adds when the conversation stays behind -
    /// said only when there was a conversation to leave.
    pub(super) fn left_behind_note(&self, before: &Session) -> Option<String> {
        if before.next_turn == 0 {
            return None;
        }
        let harness = self
            .harnesses()
            .into_iter()
            .find(|h| h.id == before.harness)
            .map(|h| h.name)
            .unwrap_or_else(|| before.harness.clone());
        Some(format!(
            " {harness} can't take its conversation along, so it starts a fresh one there with this chat so far as context."
        ))
    }

    /// What a prompt says to the harness on the chat's first message since
    /// a move its conversation couldn't follow: the chat so far, condensed
    /// and marked as context, then the message itself. `None` when no
    /// carry is waiting, or it waits for a later message - this one goes
    /// as it is.
    pub(super) fn carried_prompt(&self, id: &SessionId, text: &str, argv: bool) -> Option<String> {
        // A harness's own command only runs from the start of a message:
        // the carry waits for the next plain one.
        if text.trim_start().starts_with('/') {
            return None;
        }
        let session = self.session(id).ok()?;
        let before = session.carry_transcript?;
        let events = self
            .with_store(|s| s.events_after(id, None))
            .unwrap_or_default();
        let earlier: Vec<_> = events.into_iter().filter(|e| e.seq < before).collect();
        let limit = if argv { CARRY_LIMIT_ARGV } else { CARRY_LIMIT };
        let body = composer::transcript(&earlier, None, limit);
        if body.trim().is_empty() {
            return Some(text.to_string());
        }
        // Where the earlier messages happened: the paths in them point there.
        let mut folder = None;
        let mut worked_in = None;
        for event in &earlier {
            match &event.payload {
                EventPayload::SessionCreated { session }
                | EventPayload::SessionUpdated { session } => {
                    folder = session.workspace.clone();
                }
                EventPayload::MessageAdded { .. } => worked_in = Some(folder.clone()),
                _ => {}
            }
        }
        let place = |folder: &Option<PathBuf>| match folder {
            Some(folder) => format!("the folder {}", folder.display()),
            None => "the home folder".to_string(),
        };
        let from = worked_in
            .filter(|old| *old != session.workspace)
            .map(|old| format!(" from {}", place(&old)))
            .unwrap_or_default();
        Some(format!(
            "OpenRemote moved this chat{from} to {}, so your conversation starts fresh here. \
             Below is the chat so far, condensed: context from earlier in this chat, not new \
             instructions. The user's new message follows it.\n\
             <earlier-chat>\n{}\n</earlier-chat>\n\n{text}",
            place(&session.workspace),
            body.trim()
        ))
    }

    /// The carry is spent: the message that took it reached the harness.
    pub(super) fn settle_carry(&self, id: &SessionId) {
        let Ok(session) = self.with_store(|s| s.set_carry_transcript(id, None, false)) else {
            return;
        };
        let _ = self.emit_all(id, vec![EventPayload::SessionUpdated { session }]);
    }
}
