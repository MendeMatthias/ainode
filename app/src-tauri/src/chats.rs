//! Saved conversations (0.7.0, from the ideas list): local only, and only
//! when the user switches "Keep chats" on in Status (off by default, so
//! nothing of a conversation is written unless they ask). One JSON file per
//! chat under `$DATA/chats/`; listed, opened, continued and deleted from the
//! chat's History. They count against the space the user allowed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Msg {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub model_id: String,
    pub title: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    pub messages: Vec<Msg>,
}

/// What History lists.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMeta {
    pub id: String,
    pub model_id: String,
    pub title: String,
    pub updated: u64,
    pub count: usize,
}

pub const MAX_MESSAGES: usize = 1000;
pub const MAX_CHARS: usize = 4_000_000;

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("chats")
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn path(data_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("unknown chat".into());
    }
    Ok(dir(data_dir).join(format!("{id}.json")))
}

pub fn total_bytes(data_dir: &Path) -> u64 {
    crate::disk::tree_bytes(&dir(data_dir))
}

/// Saved chats of one model (or all), newest first.
pub fn list(data_dir: &Path, model_id: Option<&str>) -> Vec<ChatMeta> {
    let Ok(rd) = std::fs::read_dir(dir(data_dir)) else { return Vec::new() };
    let mut out: Vec<ChatMeta> = rd
        .flatten()
        .filter_map(|e| {
            let raw = std::fs::read(e.path()).ok()?;
            let c: Chat = serde_json::from_slice(&raw).ok()?;
            if !valid_id(&c.id) || model_id.is_some_and(|m| m != c.model_id) {
                return None;
            }
            Some(ChatMeta { count: c.messages.len(), id: c.id, model_id: c.model_id, title: c.title, updated: c.updated })
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.updated));
    out
}

pub fn load(data_dir: &Path, id: &str) -> Result<Chat, String> {
    let raw = std::fs::read(path(data_dir, id)?).map_err(|_| "That chat is gone".to_string())?;
    serde_json::from_slice(&raw).map_err(|e| e.to_string())
}

/// Write one chat. `room` is what the allowed space still has.
pub fn save(data_dir: &Path, mut c: Chat, room: u64, now: u64) -> Result<(), String> {
    let p = path(data_dir, &c.id)?;
    if c.messages.is_empty() {
        return Ok(());
    }
    if c.messages.len() > MAX_MESSAGES || c.messages.iter().map(|m| m.content.len()).sum::<usize>() > MAX_CHARS {
        return Err("This chat is too long to keep. Start a new one with Clear.".into());
    }
    if c.messages.iter().any(|m| m.role != "user" && m.role != "assistant") {
        return Err("unknown message role".into());
    }
    c.title = c.title.chars().filter(|ch| !ch.is_control()).take(80).collect();
    let before = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    if before == 0 {
        c.created = now;
    } else if let Ok(old) = load(data_dir, &c.id) {
        c.created = old.created;
    }
    c.updated = now;
    let size = serde_json::to_vec_pretty(&c).map_err(|e| e.to_string())?.len() as u64;
    if size > room.saturating_add(before) {
        return Err("Not enough room in the space you allowed to keep this chat.".into());
    }
    state::create_private_dir_all(&dir(data_dir))?;
    state::write_json_atomic(&p, &c)
}

pub fn delete(data_dir: &Path, id: &str) -> Result<(), String> {
    match std::fs::remove_file(path(data_dir, id)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(id: &str, model: &str, n: usize) -> Chat {
        Chat {
            id: id.into(),
            model_id: model.into(),
            title: "What is BTX?\u{7}".into(),
            created: 0,
            updated: 0,
            messages: (0..n).map(|i| Msg { role: if i % 2 == 0 { "user" } else { "assistant" }.into(), content: format!("m{i}") }).collect(),
        }
    }

    #[test]
    fn save_list_load_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        save(d, chat("00000000000000aa", "granite", 2), 1_000_000, 10).unwrap();
        save(d, chat("00000000000000bb", "gpt-oss", 4), 1_000_000, 20).unwrap();
        save(d, chat("00000000000000aa", "granite", 4), 1_000_000, 30).unwrap();
        let all = list(d, None);
        assert_eq!(all.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["00000000000000aa", "00000000000000bb"]);
        assert_eq!(list(d, Some("granite")).len(), 1);
        let c = load(d, "00000000000000aa").unwrap();
        assert_eq!((c.created, c.updated, c.messages.len()), (10, 30, 4));
        assert_eq!(c.title, "What is BTX?", "control characters dropped");
        delete(d, "00000000000000aa").unwrap();
        delete(d, "00000000000000aa").unwrap();
        assert_eq!(list(d, None).len(), 1);
    }

    #[test]
    fn refuses_bad_ids_roles_and_what_does_not_fit() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert!(save(d, chat("../../consent", "m", 2), 1_000_000, 1).is_err());
        assert!(load(d, "../settings").is_err());
        assert!(delete(d, "ABCDEF0123456789").is_err());
        let mut c = chat("00000000000000cc", "m", 2);
        c.messages[0].role = "system".into();
        assert!(save(d, c, 1_000_000, 1).is_err());
        assert!(save(d, chat("00000000000000dd", "m", 2), 10, 1).is_err());
        save(d, chat("00000000000000ee", "m", 0), 0, 1).unwrap();
        assert!(list(d, None).is_empty(), "an empty chat is not written");
    }
}
