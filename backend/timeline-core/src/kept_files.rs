//! The files a conversation's export holds (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4).
//!
//! Claude writes a file with its file tool (`create_file`, the full text)
//! and later edits it with find-and-replace steps (`str_replace`).
//! [`FileReplay`] replays them in order to get each file's final text. A file
//! Claude changed afterwards by running a command can't be replayed, so its
//! kept copy may be older than the final one: when a later command names
//! the file, the copy is marked "may have been changed later" (plan C9). The
//! page marks this before slimming a file it sends, since the commands are
//! dropped (§7b), with the `create_file` input field
//! [`MAY_HAVE_CHANGED_MARK`]; the mark is honoured here too.
//!
//! A file Claude presented (`present_files`) whose text isn't in the export
//! (a Word document or an image made by running a script) is kept by name
//! only.

use std::collections::HashMap;

use serde_json::Value;

use crate::model::{ChatMessage, ContentPiece, PieceType};
use crate::stored_message::FileKind;

/// The field the page adds to a `create_file` call's input when a later
/// command names the file (§7b).
pub const MAY_HAVE_CHANGED_MARK: &str = "_claude_timeline_may_have_changed";

/// One file's replayed state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayedFile {
    pub text: String,
    pub may_have_changed_later: bool,
}

/// Every file Claude wrote in a conversation, by path, after replaying its
/// edits.
#[derive(Debug, Default)]
pub struct FileReplay {
    files: HashMap<String, ReplayedFile>,
}

/// A tool call's name and input, when `piece` is one.
pub fn tool_call(piece: &ContentPiece) -> Option<(&str, &Value)> {
    if piece.piece_type != PieceType::Other("tool_use".to_string()) {
        return None;
    }
    let name = piece.extra.get("name")?.as_str()?;
    Some((name, piece.extra.get("input")?))
}

fn text_field<'a>(input: &'a Value, name: &str) -> Option<&'a str> {
    input.get(name)?.as_str()
}

/// The part of a path after its last `/`.
pub fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl FileReplay {
    /// Replays every file call in `messages`, in order.
    pub fn of(messages: &[ChatMessage]) -> Self {
        let mut replay = FileReplay::default();
        for piece in messages.iter().flat_map(|m| &m.content) {
            let Some((name, input)) = tool_call(piece) else {
                continue;
            };
            match name {
                "create_file" => replay.create(input),
                "str_replace" => replay.edit(input),
                _ if is_command(name) => replay.command_names(input),
                _ => {}
            }
        }
        replay
    }

    fn create(&mut self, input: &Value) {
        let (Some(path), Some(text)) = (text_field(input, "path"), text_field(input, "file_text"))
        else {
            return;
        };
        let marked = input
            .get(MAY_HAVE_CHANGED_MARK)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.files.insert(
            path.to_string(),
            ReplayedFile {
                text: text.to_string(),
                may_have_changed_later: marked,
            },
        );
    }

    /// Replaces the first occurrence of `old_str`, as Claude's tool does
    /// (it refuses an `old_str` that isn't unique). An edit whose text isn't
    /// found can't be replayed: the copy is marked.
    fn edit(&mut self, input: &Value) {
        let Some(path) = text_field(input, "path") else {
            return;
        };
        let Some(file) = self.files.get_mut(path) else {
            return;
        };
        let (Some(old), Some(new)) = (text_field(input, "old_str"), text_field(input, "new_str"))
        else {
            file.may_have_changed_later = true;
            return;
        };
        match file.text.find(old) {
            Some(at) if !old.is_empty() => file.text.replace_range(at..at + old.len(), new),
            _ => file.may_have_changed_later = true,
        }
    }

    /// A command naming a file Claude wrote, by its path or its name, may
    /// have changed it.
    fn command_names(&mut self, input: &Value) {
        let Some(command) = text_field(input, "command") else {
            return;
        };
        for (path, file) in self.files.iter_mut() {
            if command.contains(path.as_str()) || command.contains(base_name(path)) {
                file.may_have_changed_later = true;
            }
        }
    }

    /// The file at `path`, if Claude wrote it with its file tool.
    pub fn get(&self, path: &str) -> Option<&ReplayedFile> {
        self.files.get(path)
    }
}

/// The tool calls that run a command.
pub fn is_command(tool: &str) -> bool {
    matches!(tool, "bash_tool" | "bash")
}

/// How a file is shown, from its name's extension.
pub fn kind_of(name: &str) -> FileKind {
    let extension = match name.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => String::new(),
    };
    let code = |language: &str| FileKind::Code {
        language: language.to_string(),
    };
    match extension.as_str() {
        "svg" => FileKind::Svg,
        "html" | "htm" => FileKind::WebPage,
        "md" | "markdown" => FileKind::Markdown,
        "txt" | "csv" | "log" | "tsv" => FileKind::Text,
        "py" => code("python"),
        // Apps Script is JavaScript.
        "js" | "mjs" | "cjs" | "gs" => code("javascript"),
        "ts" => code("typescript"),
        "json" => code("json"),
        "css" => code("css"),
        "sh" | "bash" => code("bash"),
        "rs" => code("rust"),
        "java" => code("java"),
        "c" | "h" => code("c"),
        "cpp" | "cc" | "hpp" => code("cpp"),
        "go" => code("go"),
        "rb" => code("ruby"),
        "sql" => code("sql"),
        "yaml" | "yml" => code("yaml"),
        "xml" => code("xml"),
        _ => FileKind::Other,
    }
}
