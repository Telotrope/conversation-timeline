//! What is kept of a parsed conversation (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §2–§4d): rows
//! with text pieces and citations, file marks where files were presented,
//! attachments, your review, notes for replaced branches and important
//! branches as conversations of their own.

use serde_json::{json, Value};
use timeline_core::flag_values::FlagOverrides;
use timeline_core::keep::{
    branch_conversation_id, keep_conversation, latest_time, KeepError, Kept,
};
use timeline_core::kept_files::{kind_of, MAY_HAVE_CHANGED_MARK};
use timeline_core::model::{Conversation, ConversationId, MessageId, Sender};
use timeline_core::stored_message::{
    CitedAddress, Entry, FileContents, FileKind, FileRef, Piece, StoredMessage,
};
use timeline_core::{MessageTime, UNKNOWN_TIME};

const ROOT: &str = "00000000-0000-4000-8000-000000000000";
const CONV: &str = "cccccccc-0000-4000-8000-000000000001";

fn id(n: u32) -> String {
    format!("00000000-0000-0000-0000-{n:012}")
}

fn at(minute: i64) -> String {
    chrono::DateTime::from_timestamp(1_700_000_000 + minute * 60, 0)
        .unwrap()
        .to_rfc3339()
}

fn tool(name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "name": name, "input": input, "id": "toolu_1"})
}

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

/// A message: (id, parent or 0 for the start, sender, minute, content).
fn message(n: u32, parent: u32, sender: &str, minute: i64, content: Vec<Value>) -> Value {
    json!({
        "uuid": id(n),
        "parent_message_uuid": if parent == 0 { ROOT.to_string() } else { id(parent) },
        "sender": sender,
        "created_at": at(minute),
        "content": content,
    })
}

fn conversation(messages: Vec<Value>) -> Conversation {
    serde_json::from_value(json!({"uuid": CONV, "name": "Plans", "chat_messages": messages}))
        .unwrap()
}

fn kept(messages: Vec<Value>) -> Kept {
    keep_conversation(&conversation(messages), false).unwrap()
}

fn messages_of(kept: &Kept) -> Vec<&StoredMessage> {
    kept.main
        .entries
        .iter()
        .filter_map(Entry::as_message)
        .collect()
}

fn files_of(message: &StoredMessage) -> Vec<FileRef> {
    message.files().cloned().collect()
}

#[test]
fn text_pieces_keep_their_citations_and_everything_else_is_dropped() {
    let k = kept(vec![message(
        1,
        0,
        "assistant",
        0,
        vec![
            json!({"type": "thinking", "thinking": "hmm"}),
            json!({"type": "text", "text": "Fact one. Fact two.", "citations": [
                {"start_index": 0, "end_index": 9, "details": {"type": "web_search_citation", "url": "https://a.example/x"}},
                {"start_index": 10, "end_index": 19, "details": {"url": "javascript:alert(1)"}},
                {"start_index": 10, "end_index": 19, "details": {"type": "no address"}},
                {"end_index": 3, "details": {"url": "https://b.example"}},
            ]}),
            tool("web_search", json!({"query": "x"})),
            json!({"type": "tool_result", "content": [{"type": "text", "text": "big result"}]}),
        ],
    )]);
    let m = messages_of(&k)[0];
    assert_eq!(m.text(), "Fact one. Fact two.");
    let Piece::Text { citations, .. } = &m.pieces[0] else {
        panic!("{:?}", m.pieces);
    };
    assert_eq!(m.pieces.len(), 1, "only the text piece is kept");
    assert_eq!(
        citations.len(),
        2,
        "citations without an address or a position are left out"
    );
    assert_eq!((citations[0].start, citations[0].end), (0, 9));
    assert_eq!(
        citations[0].address,
        CitedAddress::Web("https://a.example/x".to_string())
    );
    assert_eq!(
        citations[1].address,
        CitedAddress::Other("javascript:alert(1)".to_string())
    );
    assert_eq!(m.flags, None, "Claude's messages carry no flags");
}

/// The file tool's edits are replayed to each file's final text, and the
/// file is marked where the reply presented it, between its paragraphs.
#[test]
fn a_presented_file_is_its_replayed_text_where_it_was_presented() {
    let k = kept(vec![
        message(1, 0, "human", 0, vec![text("write it")]),
        message(
            2,
            1,
            "assistant",
            1,
            vec![
                tool(
                    "create_file",
                    json!({"path": "/mnt/out/plan.md", "file_text": "# Plan\nstep one\n"}),
                ),
                tool(
                    "str_replace",
                    json!({"path": "/mnt/out/plan.md", "old_str": "one", "new_str": "two"}),
                ),
                tool(
                    "str_replace",
                    json!({"path": "/mnt/other.md", "old_str": "x", "new_str": "y"}),
                ),
                text("Here it is:"),
                tool(
                    "present_files",
                    json!({"filepaths": ["/mnt/out/plan.md", "/mnt/out/chart.png"]}),
                ),
                text("Done."),
            ],
        ),
    ]);
    let reply = messages_of(&k)[1];
    assert!(matches!(&reply.pieces[0], Piece::Text { text, .. } if text == "Here it is:"));
    let Piece::File { file: plan } = &reply.pieces[1] else {
        panic!("{:?}", reply.pieces)
    };
    assert_eq!(plan.name.as_str(), "plan.md");
    assert_eq!(plan.kind, FileKind::Markdown);
    assert_eq!(plan.contents, FileContents::Stored);
    assert!(!plan.may_have_changed_later);
    let Piece::File { file: chart } = &reply.pieces[2] else {
        panic!("{:?}", reply.pieces)
    };
    assert_eq!(chart.name.as_str(), "chart.png");
    assert_eq!(chart.contents, FileContents::NotInExport);
    assert_eq!(chart.kind, FileKind::Other);
    assert!(matches!(&reply.pieces[3], Piece::Text { text, .. } if text == "Done."));
    assert_eq!(k.main.files.len(), 1);
    assert_eq!(k.main.files[0].text, "# Plan\nstep two\n");
    assert_eq!(
        k.main.files[0].message_id,
        MessageId(id(2).parse().unwrap())
    );
    assert_eq!((plan.number, chart.number), (0, 1));
}

/// A file Claude changed afterwards by running a command can't be
/// replayed; when a later command names it (by path or by name), or the
/// page marked it before slimming the upload, or an edit can't be applied,
/// the copy is marked "may have been changed later" (plan C9).
#[test]
fn a_file_a_later_command_names_or_an_edit_misses_is_marked() {
    let presented = |files: &[&str]| tool("present_files", json!({"filepaths": files}));
    let k = kept(vec![message(
        1,
        0,
        "assistant",
        0,
        vec![
            tool("bash_tool", json!({"command": "ls /mnt/out/early.py"})),
            tool(
                "create_file",
                json!({"path": "/mnt/out/early.py", "file_text": "a"}),
            ),
            tool(
                "create_file",
                json!({"path": "/mnt/out/by_path.py", "file_text": "a"}),
            ),
            tool(
                "bash_tool",
                json!({"command": "python /mnt/out/by_path.py > x"}),
            ),
            tool(
                "create_file",
                json!({"path": "/mnt/out/by_name.sh", "file_text": "a"}),
            ),
            tool(
                "bash",
                json!({"command": "cd /mnt/out && chmod +x by_name.sh"}),
            ),
            tool(
                "create_file",
                json!({"path": "/mnt/out/marked.svg", "file_text": "<svg/>", MAY_HAVE_CHANGED_MARK: true}),
            ),
            tool(
                "create_file",
                json!({"path": "/mnt/out/missed.html", "file_text": "<p>hi</p>"}),
            ),
            tool(
                "str_replace",
                json!({"path": "/mnt/out/missed.html", "old_str": "nowhere", "new_str": "x"}),
            ),
            tool(
                "create_file",
                json!({"path": "/mnt/out/broken.txt", "file_text": "abc"}),
            ),
            tool(
                "str_replace",
                json!({"path": "/mnt/out/broken.txt", "old_str": "abc"}),
            ),
            tool(
                "create_file",
                json!({"path": "/mnt/out/empty_edit.csv", "file_text": "abc"}),
            ),
            tool(
                "str_replace",
                json!({"path": "/mnt/out/empty_edit.csv", "old_str": "", "new_str": "x"}),
            ),
            tool("bash_tool", json!({"description": "no command"})),
            tool("create_file", json!({"path": "/mnt/out/no_text.md"})),
            tool("str_replace", json!({"old_str": "a", "new_str": "b"})),
            presented(&[
                "/mnt/out/early.py",
                "/mnt/out/by_path.py",
                "/mnt/out/by_name.sh",
                "/mnt/out/marked.svg",
            ]),
            presented(&[
                "/mnt/out/missed.html",
                "/mnt/out/broken.txt",
                "/mnt/out/empty_edit.csv",
                "/mnt/out/no_text.md",
            ]),
        ],
    )]);
    let files = files_of(messages_of(&k)[0]);
    let marked: Vec<(&str, bool)> = files
        .iter()
        .map(|f| (f.name.as_str(), f.may_have_changed_later))
        .collect();
    assert_eq!(
        marked,
        vec![
            ("early.py", false),
            ("by_path.py", true),
            ("by_name.sh", true),
            ("marked.svg", true),
            ("missed.html", true),
            ("broken.txt", true),
            ("empty_edit.csv", true),
            ("no_text.md", false),
        ]
    );
    assert_eq!(
        files[7].contents,
        FileContents::NotInExport,
        "a create_file without its text isn't kept"
    );
    assert_eq!(files[3].kind, FileKind::Svg);
    assert_eq!(files[4].kind, FileKind::WebPage);
    assert_eq!(
        files[0].kind,
        FileKind::Code {
            language: "python".to_string()
        }
    );
}

#[test]
fn a_widget_is_kept_as_a_web_page_named_by_its_title() {
    let k = kept(vec![message(
        1,
        0,
        "assistant",
        0,
        vec![
            tool(
                "visualize:show_widget",
                json!({"title": "comparison", "widget_code": "<table></table>"}),
            ),
            tool("visualize:show_widget", json!({"widget_code": "<p></p>"})),
            tool("visualize:show_widget", json!({"title": "no code"})),
            tool("present_files", json!({"filepaths": "not a list"})),
        ],
    )]);
    let files = files_of(messages_of(&k)[0]);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].name.as_str(), "comparison.html");
    assert_eq!(files[0].kind, FileKind::WebPage);
    assert_eq!(files[1].name.as_str(), "widget.html");
    assert_eq!(k.main.files[0].text, "<table></table>");
}

/// Your attachments' extracted text is kept as a file; uploaded files are
/// names only; a file with no name is called by its number.
#[test]
fn attachments_are_kept_as_text_and_uploaded_files_by_name() {
    let mut m = message(1, 0, "human", 0, vec![text("see attached")]);
    m["attachments"] = json!([
        {"file_name": "nda.txt", "file_type": "txt", "extracted_content": "The parties agree."},
        {"file_name": "", "extracted_content": "untitled"},
        {"file_name": "scan.pdf"},
    ]);
    m["files"] = json!([{"file_uuid": "x", "file_name": "photo.png"}, {"file_uuid": "y"}]);
    let k = kept(vec![m]);
    let message = messages_of(&k)[0];
    let names: Vec<(&str, FileContents)> = message
        .attachments
        .iter()
        .map(|f| (f.name.as_str(), f.contents))
        .collect();
    assert_eq!(
        names,
        vec![
            ("nda.txt", FileContents::Stored),
            ("file 2", FileContents::Stored),
            ("scan.pdf", FileContents::NotInExport),
            ("photo.png", FileContents::NotInExport),
            ("file 5", FileContents::NotInExport),
        ]
    );
    assert_eq!(message.attachments[0].kind, FileKind::Text);
    let texts: Vec<&str> = k.main.files.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(texts, vec!["The parties agree.", "untitled"]);
}

/// Your review travels in `_claude_timeline_user`; an automatic flag a file
/// carries is never taken (the scan writes those).
#[test]
fn your_review_is_kept_and_automatic_flags_in_the_file_are_not() {
    let mut mine = message(1, 0, "human", 0, vec![text("WRONG")]);
    mine["_claude_timeline_user"] = json!({"critical": true, "caps": false});
    mine["_claude_timeline_auto"] = json!({"caps": true, "critical": true, "angry": true});
    let mut theirs = message(2, 1, "assistant", 1, vec![text("ok")]);
    theirs["_claude_timeline_user"] = json!("ignored on Claude's messages");
    let k = kept(vec![mine, theirs]);
    let flags = messages_of(&k)[0].flags.unwrap();
    assert_eq!(flags.auto, None);
    assert_eq!(
        flags.user,
        FlagOverrides {
            caps: Some(false),
            critical: Some(true),
            angry: None
        }
    );
    assert_eq!(messages_of(&k)[1].flags, None);
}

#[test]
fn a_review_that_isnt_one_fails_naming_its_message() {
    let mut mine = message(1, 0, "human", 0, vec![text("hi")]);
    mine["_claude_timeline_user"] = json!("yes please");
    let err = keep_conversation(&conversation(vec![mine]), false).unwrap_err();
    let KeepError::Review { message_id, .. } = &err;
    assert_eq!(*message_id, MessageId(id(1).parse().unwrap()));
    assert!(err.to_string().contains("_claude_timeline_user"), "{err}");
    assert!(std::error::Error::source(&err).is_some());
}

/// A fresh export has its retried resends removed (after its branches are
/// pruned); an annotated download this tool wrote is kept as it is.
#[test]
fn resends_are_removed_from_a_fresh_export_only() {
    let messages = vec![
        message(1, 0, "human", 0, vec![text("again")]),
        message(2, 1, "human", 1, vec![text("again")]),
        message(3, 2, "assistant", 2, vec![text("ok")]),
    ];
    let fresh = keep_conversation(&conversation(messages.clone()), false).unwrap();
    assert_eq!(messages_of(&fresh).len(), 2);
    let annotated = keep_conversation(&conversation(messages), true).unwrap();
    assert_eq!(messages_of(&annotated).len(), 3);
    assert_eq!(annotated.main.kept_path.len(), 3);
}

/// A replaced branch becomes a note where it began; one of 100 words or
/// more not repeated on the kept path is kept as a conversation of its own,
/// named with its start in UTC and linked both ways (§4d).
#[test]
fn replaced_branches_become_notes_or_conversations_of_their_own() {
    let long = vec!["word"; 120].join(" ");
    let k = kept(vec![
        message(1, 0, "human", 0, vec![text("start")]),
        message(2, 1, "assistant", 1, vec![text("ok")]),
        // A resend, replaced by message 4: a note.
        message(3, 2, "human", 2, vec![text("question")]),
        message(4, 2, "human", 3, vec![text("question")]),
        message(5, 4, "assistant", 4, vec![text("answer")]),
        // A long dictated message never answered, replaced by message 7.
        message(6, 5, "human", 5, vec![text(&long)]),
        message(7, 5, "human", 6, vec![text("shorter")]),
        message(8, 7, "assistant", 7, vec![text("done")]),
    ]);
    let notes: Vec<_> = k
        .main
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Note(n) => Some(n),
            Entry::Message(_) => None,
        })
        .collect();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].key.id, MessageId(id(3).parse().unwrap()));
    assert_eq!(notes[0].messages, 1);
    assert_eq!(notes[0].words_not_repeated, 0);
    assert_eq!(
        notes[0].replaced_by,
        Some(MessageId(id(4).parse().unwrap()))
    );
    assert_eq!(notes[0].kept_as, None);
    assert_eq!(notes[1].words_not_repeated, 120);
    let branch_id = branch_conversation_id(
        ConversationId(CONV.parse().unwrap()),
        MessageId(id(6).parse().unwrap()),
    );
    assert_eq!(notes[1].kept_as, Some(branch_id));
    assert_eq!(k.branches.len(), 1);
    let branch = &k.branches[0];
    assert_eq!(branch.branch_of, ConversationId(CONV.parse().unwrap()));
    assert_eq!(branch.conversation.conversation_id, branch_id);
    assert_eq!(
        branch.conversation.name.0,
        "Plans: earlier branch from 2023-11-14 22:18 UTC"
    );
    let branch_messages: Vec<&StoredMessage> = branch
        .conversation
        .entries
        .iter()
        .filter_map(Entry::as_message)
        .collect();
    assert_eq!(branch_messages.len(), 1);
    assert_eq!(branch_messages[0].key.conversation_id, branch_id);
    // Entries come in key order, notes among the messages.
    let keys: Vec<_> = k.main.entries.iter().map(Entry::key).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    // Processing the same branch again gives the same id.
    assert_eq!(
        branch_id,
        branch_conversation_id(
            ConversationId(CONV.parse().unwrap()),
            MessageId(id(6).parse().unwrap())
        )
    );
}

/// A conversation with a message of unknown time isn't pruned (its most
/// recent message can't be found), so even a long resend stays in it.
#[test]
fn a_conversation_with_a_message_of_unknown_time_keeps_every_message() {
    let long = vec!["word"; 120].join(" ");
    let mut untimed = message(3, 1, "human", 0, vec![text(&long)]);
    untimed.as_object_mut().unwrap().remove("created_at");
    let k = kept(vec![
        message(1, 0, "human", 0, vec![text("start")]),
        untimed,
        message(2, 1, "human", 5, vec![text("kept")]),
    ]);
    assert!(k.branches.is_empty());
    assert_eq!(messages_of(&k).len(), 3);
}

/// A message of unknown time is kept, keyed at the zero date (§4e).
#[test]
fn a_message_of_unknown_time_is_kept_at_the_zero_date() {
    let mut untimed = message(1, 0, "human", 0, vec![text("when?")]);
    untimed.as_object_mut().unwrap().remove("created_at");
    untimed
        .as_object_mut()
        .unwrap()
        .remove("parent_message_uuid");
    let timed = json!({"uuid": id(2), "sender": "assistant", "created_at": at(5), "content": [text("now")]});
    let k = kept(vec![untimed, timed]);
    let m = messages_of(&k);
    assert_eq!(m[0].key.at, UNKNOWN_TIME);
    assert_eq!(m[0].key.time(), MessageTime::Unknown);
    assert_eq!(m[0].parent, None);
    assert_eq!(m[1].sender, Sender::Assistant);
    assert_eq!(
        latest_time(&k.main.kept_path),
        Some(
            chrono::DateTime::parse_from_rfc3339(&at(5))
                .unwrap()
                .with_timezone(&chrono::Utc)
        )
    );
    assert_eq!(latest_time(&[]), None);
}

#[test]
fn a_messages_parent_is_kept_when_it_names_one() {
    let k = kept(vec![
        message(1, 0, "human", 0, vec![text("a")]),
        message(2, 1, "assistant", 1, vec![text("b")]),
    ]);
    assert_eq!(messages_of(&k)[0].parent, None, "the conversation's start");
    assert_eq!(
        messages_of(&k)[1].parent,
        Some(MessageId(id(1).parse().unwrap()))
    );
}

#[test]
fn each_kind_of_file_is_named_by_its_extension() {
    let code = |l: &str| FileKind::Code {
        language: l.to_string(),
    };
    for (name, kind) in [
        ("a.SVG", FileKind::Svg),
        ("a.htm", FileKind::WebPage),
        ("a.markdown", FileKind::Markdown),
        ("a.tsv", FileKind::Text),
        ("a.log", FileKind::Text),
        ("a.gs", code("javascript")),
        ("a.mjs", code("javascript")),
        ("a.cjs", code("javascript")),
        ("a.js", code("javascript")),
        ("a.ts", code("typescript")),
        ("a.json", code("json")),
        ("a.css", code("css")),
        ("a.sh", code("bash")),
        ("a.bash", code("bash")),
        ("a.rs", code("rust")),
        ("a.java", code("java")),
        ("a.h", code("c")),
        ("a.c", code("c")),
        ("a.cc", code("cpp")),
        ("a.cpp", code("cpp")),
        ("a.hpp", code("cpp")),
        ("a.go", code("go")),
        ("a.rb", code("ruby")),
        ("a.sql", code("sql")),
        ("a.yml", code("yaml")),
        ("a.yaml", code("yaml")),
        ("a.xml", code("xml")),
        ("report.docx", FileKind::Other),
        ("Makefile", FileKind::Other),
    ] {
        assert_eq!(kind_of(name), kind, "{name}");
    }
}

/// Plan §12.3: each message stands where the file lists it. A message of
/// unknown time in the middle stays in the middle (its time used to sort it
/// first), and the keys follow the file's order.
#[test]
fn each_message_is_placed_where_the_file_lists_it() {
    let mut untimed = message(2, 1, "assistant", 0, vec![text("no time")]);
    untimed.as_object_mut().unwrap().remove("created_at");
    let k = kept(vec![
        message(1, 0, "human", 5, vec![text("first")]),
        untimed,
        message(3, 2, "human", 9, vec![text("third")]),
    ]);
    let order: Vec<(String, i64)> = k
        .main
        .entries
        .iter()
        .map(|e| (e.key().id.0.to_string(), e.key().position.0))
        .collect();
    assert_eq!(order, vec![(id(1), 0), (id(2), 1), (id(3), 2)]);
}

/// Plan §12.3: a replaced branch's note stands where the file lists the
/// branch's first message.
#[test]
fn a_note_stands_where_its_branch_began_in_the_file() {
    let k = kept(vec![
        message(1, 0, "human", 0, vec![text("start")]),
        message(2, 1, "assistant", 1, vec![text("ok")]),
        message(3, 2, "human", 2, vec![text("first try")]),
        message(4, 2, "human", 3, vec![text("second try")]),
    ]);
    let order: Vec<(String, i64, bool)> = k
        .main
        .entries
        .iter()
        .map(|e| {
            (
                e.key().id.0.to_string(),
                e.key().position.0,
                e.as_message().is_none(),
            )
        })
        .collect();
    assert_eq!(
        order,
        vec![
            (id(1), 0, false),
            (id(2), 1, false),
            (id(3), 2, true),
            (id(4), 3, false)
        ]
    );
}
