//! Prunes the branches of a conversation that a later message replaced
//! (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4d).
//!
//! When you edit a message or send it again, claude.ai keeps the earlier
//! version too: every message names the one it answers, and the
//! conversation becomes a tree. The kept narrative is the path from the
//! start to the conversation's most recent message; every branch off that
//! path was replaced. (Not "the newer reply wins", which the user's data
//! showed would sometimes keep a dead end and drop a long conversation.)
//!
//! Each replaced branch is returned with how many of its words are not
//! repeated on the kept path, which decides whether it is kept as a
//! conversation of its own ([`ReplacedBranch::is_important`]).
//!
//! A conversation is left whole when its links can't be trusted: when any
//! message has no parent field (an older export, or a file made by hand),
//! or names a parent that isn't in the conversation (a cut-down copy). It is
//! left whole too when any message's time is unknown (plan §4e): the most
//! recent message can't be found then, and a message of unknown time would
//! otherwise look like the oldest, never on the path.

use std::collections::{HashMap, HashSet};

use crate::dedup::extract_text;
use crate::message_time::MessageTime;
use crate::model::{ChatMessage, Conversation, MessageId, ParentLink, Sender};

/// A branch worth keeping as its own conversation holds at least this many
/// words not repeated on the kept path (the user, 2026-10-06, Q5).
pub const IMPORTANT_WORDS: usize = 100;

/// One branch that a later message replaced.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplacedBranch {
    /// The branch's messages, oldest first.
    pub messages: Vec<ChatMessage>,
    /// The message on the kept path that took the branch's place: the kept
    /// child of the message the branch answered. `None` when the kept path
    /// has no message there (the branch answered the kept path's last
    /// message).
    pub replaced_by: Option<MessageId>,
    /// Words in the branch's messages whose text is not included in a
    /// message from the same sender on the kept path. Text is compared after
    /// collapsing runs of spaces and line breaks, so a message cut off and
    /// sent again in full, or sent and then extended, counts as repeated
    /// (the user, 2026-10-06).
    pub words_not_repeated: usize,
}

impl ReplacedBranch {
    pub fn is_important(&self) -> bool {
        self.words_not_repeated >= IMPORTANT_WORDS
    }
}

/// A conversation with its replaced branches taken out.
#[derive(Debug, Clone, PartialEq)]
pub struct PrunedConversation {
    /// The path from the start to the most recent message, in order.
    pub kept: Vec<ChatMessage>,
    /// Every branch off that path, in the order they began.
    pub branches: Vec<ReplacedBranch>,
}

/// Splits `conversation` into its kept path and its replaced branches.
pub fn prune_replaced_branches(conversation: &Conversation) -> PrunedConversation {
    let messages = &conversation.chat_messages;
    let whole = || PrunedConversation {
        kept: messages.clone(),
        branches: Vec::new(),
    };
    if messages.iter().any(|m| m.time() == MessageTime::Unknown) {
        return whole();
    }
    let ids: HashSet<MessageId> = messages.iter().map(|m| m.uuid).collect();
    let mut parent_of: HashMap<MessageId, Option<MessageId>> = HashMap::new();
    for m in messages {
        let parent = match m.parent() {
            ParentLink::Unstated => return whole(),
            ParentLink::Root => None,
            ParentLink::Message(p) if ids.contains(&p) => Some(p),
            ParentLink::Message(_) => return whole(),
        };
        parent_of.insert(m.uuid, parent);
    }
    // The most recent message; of several at the same time, the last listed.
    let Some(latest) = messages
        .iter()
        .enumerate()
        .max_by_key(|(i, m)| (m.created_at, *i))
        .map(|(_, m)| m.uuid)
    else {
        return whole();
    };

    let mut path = Vec::new();
    let mut on_path = HashSet::new();
    let mut cursor = Some(latest);
    while let Some(id) = cursor {
        // A cycle in the links would loop forever; stop where it repeats.
        if !on_path.insert(id) {
            break;
        }
        path.push(id);
        cursor = parent_of[&id];
    }
    path.reverse();

    let by_id: HashMap<MessageId, &ChatMessage> = messages.iter().map(|m| (m.uuid, m)).collect();
    let kept: Vec<ChatMessage> = path.iter().map(|id| by_id[id].clone()).collect();

    // The kept child of each kept message, and the kept root.
    let mut kept_child: HashMap<Option<MessageId>, MessageId> = HashMap::new();
    for id in &path {
        kept_child.insert(parent_of[id], *id);
    }

    let mut children: HashMap<Option<MessageId>, Vec<&ChatMessage>> = HashMap::new();
    for m in messages {
        children.entry(parent_of[&m.uuid]).or_default().push(m);
    }

    let kept_texts: Vec<(Sender, String)> = kept
        .iter()
        .map(|m| (m.sender.clone(), extract_text(m)))
        .collect();

    let mut branches = Vec::new();
    for m in messages {
        if on_path.contains(&m.uuid) {
            continue;
        }
        let parent = parent_of[&m.uuid];
        let branch_starts_here = match parent {
            None => true,
            Some(p) => on_path.contains(&p),
        };
        if !branch_starts_here {
            continue;
        }
        let mut branch = Vec::new();
        let mut stack = vec![m];
        while let Some(x) = stack.pop() {
            branch.push(x.clone());
            if let Some(kids) = children.get(&Some(x.uuid)) {
                stack.extend(kids.iter().copied());
            }
        }
        branch.sort_by_key(|x| x.created_at);
        let branch_texts: Vec<(Sender, String)> = branch
            .iter()
            .map(|x| (x.sender.clone(), extract_text(x)))
            .collect();
        let words_not_repeated = words_not_repeated(&branch_texts, &kept_texts);
        branches.push(ReplacedBranch {
            messages: branch,
            replaced_by: kept_child.get(&parent).copied(),
            words_not_repeated,
        });
    }
    branches.sort_by_key(|b| b.messages[0].created_at);
    PrunedConversation { kept, branches }
}

fn collapse_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Words in `branch`'s messages (each a sender and its text) whose text is
/// not included in a message from the same sender in `kept`. Text is
/// compared after collapsing runs of spaces and line breaks; empty text is
/// included in any message, so it adds no words.
pub fn words_not_repeated(branch: &[(Sender, String)], kept: &[(Sender, String)]) -> usize {
    let kept: Vec<(&Sender, String)> = kept
        .iter()
        .map(|(sender, text)| (sender, collapse_spaces(text)))
        .collect();
    branch
        .iter()
        .filter(|(sender, text)| {
            let text = collapse_spaces(text);
            !kept
                .iter()
                .any(|(kept_sender, kept_text)| *kept_sender == sender && kept_text.contains(&text))
        })
        .map(|(_, text)| text.split_whitespace().count())
        .sum()
}
