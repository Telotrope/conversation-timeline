//! What a later file adds to a conversation already stored (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4d, §4e, §10b;
//! screen-flow plan §8b-2 and Q18).
//!
//! Usually a later export holds the stored conversation plus newer
//! messages. Only messages timed outside the stored conversation's message
//! range are added (the user's rule, Q18); stored messages are never
//! compared with the file's. A message of unknown time is never added this
//! way, since it can't be outside the range; such messages are counted as
//! skipped.
//!
//! **A revived branch** (C12). If the file's newest messages continue a
//! branch an earlier upload pruned, that branch now holds the latest
//! message, so it becomes the conversation's path. It is found by the first
//! new message's parent: a parent that is stored but isn't the stored
//! path's last message marks a branch point, provided something the stored
//! path held after it is no longer on the file's path (otherwise the file
//! only adds to the stored path). What the stored path held after
//! that point becomes the replaced branch, a note or (if important) a
//! conversation of its own, and the new path's messages from the branch
//! point on are added from the file, which holds every branch.

use std::collections::HashSet;

use crate::branches::{words_not_repeated, IMPORTANT_WORDS};
use crate::conversation_metadata::ConversationSpan;
use crate::dedup::extract_text;
use crate::keep::{branch_conversation_id, Kept, KeptBranch, KeptFile};
use crate::message_time::MessageTime;
use crate::model::{ChatMessage, ConversationId, ConversationName, MessageId, ParentLink, Sender};
use crate::stored_message::{BranchNote, Entry, EntryKey, StoredMessage};

/// Stored messages moved into a conversation of their own because a revived
/// branch replaced them and they are important.
#[derive(Debug, Clone, PartialEq)]
pub struct MovedBranch {
    pub conversation_id: ConversationId,
    pub name: ConversationName,
    /// The moved rows, keyed in the new conversation.
    pub entries: Vec<Entry>,
}

/// What to change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MergePlan {
    /// Messages and notes to write into the conversation.
    pub add: Vec<Entry>,
    /// The files of the added messages.
    pub files: Vec<KeptFile>,
    /// The file's own important branches among what is added, as
    /// conversations of their own.
    pub branches: Vec<KeptBranch>,
    /// Stored rows a revived branch replaced.
    pub remove: Vec<EntryKey>,
    pub moved: Option<MovedBranch>,
    /// Messages of unknown time the file holds that weren't added.
    pub untimed_skipped: usize,
}

impl MergePlan {
    pub fn changes_nothing(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }
}

/// Works out what `file`, the kept form of a conversation in a later file,
/// adds to `stored`, the conversation's rows, whose known message times run
/// over `stored_span`.
pub fn plan_merge(
    stored: &[Entry],
    stored_span: Option<&ConversationSpan>,
    name: &ConversationName,
    file: &Kept,
) -> MergePlan {
    let stored_messages: Vec<&StoredMessage> =
        stored.iter().filter_map(Entry::as_message).collect();
    let stored_ids: HashSet<MessageId> = stored_messages.iter().map(|m| m.key.id).collect();
    let path = &file.main.kept_path;
    let Some(first_new) = path.iter().position(|m| !stored_ids.contains(&m.uuid)) else {
        return MergePlan::default();
    };
    if let Some(plan) = plan_revival(&stored_messages, &stored_ids, name, file, first_new) {
        return plan;
    }
    let outside = |at: MessageTime| match (at, stored_span) {
        (MessageTime::Unknown, _) => false,
        (MessageTime::Known(_), None) => true,
        (MessageTime::Known(at), Some(span)) => {
            let at = at.fixed_offset();
            at < span.start() || at > span.end()
        }
    };
    // By time alone: stored messages are never compared with the file's.
    let new_ids: HashSet<MessageId> = path
        .iter()
        .filter(|m| outside(m.time()))
        .map(|m| m.uuid)
        .collect();
    let untimed_skipped = path
        .iter()
        .filter(|m| !stored_ids.contains(&m.uuid) && m.time() == MessageTime::Unknown)
        .count();
    let mut plan = take_from_file(file, |entry| match entry {
        Entry::Message(m) => new_ids.contains(&m.key.id),
        Entry::Note(n) => outside(n.key.time()),
    });
    plan.untimed_skipped = untimed_skipped;
    plan
}

/// The file's entries `wanted` picks, with their files, and the important
/// branches whose notes are picked.
fn take_from_file(file: &Kept, wanted: impl Fn(&Entry) -> bool) -> MergePlan {
    let add: Vec<Entry> = file
        .main
        .entries
        .iter()
        .filter(|e| wanted(e))
        .cloned()
        .collect();
    let added_messages: HashSet<MessageId> = add
        .iter()
        .filter_map(Entry::as_message)
        .map(|m| m.key.id)
        .collect();
    let kept_as: HashSet<ConversationId> = add
        .iter()
        .filter_map(|e| match e {
            Entry::Note(n) => n.kept_as,
            Entry::Message(_) => None,
        })
        .collect();
    MergePlan {
        files: file
            .main
            .files
            .iter()
            .filter(|f| added_messages.contains(&f.message_id))
            .cloned()
            .collect(),
        branches: file
            .branches
            .iter()
            .filter(|b| kept_as.contains(&b.conversation.conversation_id))
            .cloned()
            .collect(),
        add,
        ..MergePlan::default()
    }
}

/// The revived-branch case, when `path[first_new]`'s parent is a stored
/// message other than the stored path's last, and the file's newest message
/// is newer than every stored one.
fn plan_revival(
    stored: &[&StoredMessage],
    stored_ids: &HashSet<MessageId>,
    name: &ConversationName,
    file: &Kept,
    first_new: usize,
) -> Option<MergePlan> {
    let path = &file.main.kept_path;
    let parent = match path[first_new].parent() {
        ParentLink::Message(id) => id,
        ParentLink::Root | ParentLink::Unstated => return None,
    };
    // A parent not stored, or the stored path's last message, is no branch
    // point.
    let branch_point = stored.iter().find(|m| m.key.id == parent)?;
    if stored.iter().map(|m| m.key).max() == Some(branch_point.key) {
        return None;
    }
    let newest =
        |times: &mut dyn Iterator<Item = MessageTime>| times.filter_map(MessageTime::known).max();
    let file_newest = newest(&mut path.iter().map(ChatMessage::time));
    let stored_newest = newest(&mut stored.iter().map(|m| m.key.time()));
    if file_newest <= stored_newest {
        return None;
    }
    let path_ids: HashSet<MessageId> = path.iter().map(|m| m.uuid).collect();
    let replaced: Vec<&StoredMessage> = stored
        .iter()
        .filter(|m| m.key > branch_point.key && !path_ids.contains(&m.key.id))
        .copied()
        .collect();
    let new_ids: HashSet<MessageId> = path[first_new..]
        .iter()
        .filter(|m| !stored_ids.contains(&m.uuid) && m.time() != MessageTime::Unknown)
        .map(|m| m.uuid)
        .collect();
    let untimed_skipped = path[first_new..]
        .iter()
        .filter(|m| m.time() == MessageTime::Unknown)
        .count();
    // A revival replaces something: if everything the stored path held after
    // the branch point is still on the file's path, the file only adds to
    // it, and the time rule decides what.
    let first = *replaced.first()?;
    let mut plan = take_from_file(file, |entry| match entry {
        Entry::Message(m) => new_ids.contains(&m.key.id),
        Entry::Note(n) => n.key > branch_point.key && !stored_ids.contains(&n.key.id),
    });
    plan.untimed_skipped = untimed_skipped;
    plan.remove = replaced.iter().map(|m| m.key).collect();
    let replaced_texts: Vec<(Sender, String)> = replaced
        .iter()
        .map(|m| (m.sender.clone(), m.text()))
        .collect();
    let kept_texts: Vec<(Sender, String)> = path
        .iter()
        .map(|m| (m.sender.clone(), extract_text(m)))
        .collect();
    let words = words_not_repeated(&replaced_texts, &kept_texts);
    let kept_as = (words >= IMPORTANT_WORDS).then(|| {
        let id = branch_conversation_id(first.key.conversation_id, first.key.id);
        let when = match first.key.time() {
            MessageTime::Known(at) => at.format("%Y-%m-%d %H:%M UTC").to_string(),
            MessageTime::Unknown => "an unknown time".to_string(),
        };
        plan.moved = Some(MovedBranch {
            conversation_id: id,
            name: ConversationName(format!("{}: earlier branch from {when}", name.0)),
            entries: replaced
                .iter()
                .map(|m| {
                    let mut moved = (*m).clone();
                    moved.key.conversation_id = id;
                    Entry::Message(moved)
                })
                .collect(),
        });
        id
    });
    plan.add.push(Entry::Note(BranchNote {
        key: first.key,
        last_at: replaced
            .iter()
            .map(|m| m.key.at)
            .fold(first.key.at, std::cmp::max),
        messages: replaced.len(),
        words_not_repeated: words,
        replaced_by: Some(path[first_new].uuid),
        kept_as,
    }));
    plan.add.sort_by_key(Entry::key);
    Some(plan)
}
