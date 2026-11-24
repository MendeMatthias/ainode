//! The user's own knowledge (0.7.0, Mende: "the user can feed it with its own
//! knowledge"): PDF, Markdown and text files dropped into the chat. The
//! webview reads the file and splits it into passages (pdf.js for PDF, bundled,
//! offline); this module only stores, lists and deletes them. What is kept is
//! the text that was read, one JSON file per added file under
//! `$DATA/knowledge/`, never the original file, and nothing ever leaves the
//! Mac. It counts against the space the user allowed (backend `usage`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::state;

/// One passage: a title ("notes.md > Setup", "paper.pdf, page 4") and its text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    pub t: String,
    pub x: String,
}

/// What the Status list shows, without the passages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnFile {
    pub id: String,
    pub name: String,
    /// "pdf", "md" or "txt"
    pub kind: String,
    /// Unix seconds.
    pub added: u64,
    pub chars: u64,
    /// How many passages it was split into.
    pub count: u64,
    /// Bytes this entry takes on disk.
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    meta: OwnFile,
    passages: Vec<Passage>,
}

/// A passage as the chat's search worker gets it: which file, then the passage.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub f: String,
    pub t: String,
    pub x: String,
}

/// Limits, so one dropped file cannot fill the disk or freeze the search:
/// about a 3,000-page book per file, and 200 MB of text in all.
pub const MAX_FILE_CHARS: u64 = 8_000_000;
pub const MAX_PASSAGES: usize = 20_000;
pub const MAX_PASSAGE_CHARS: usize = 4_000;
pub const MAX_TOTAL_BYTES: u64 = 200_000_000;

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("knowledge")
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn valid_kind(k: &str) -> bool {
    matches!(k, "pdf" | "md" | "txt")
}

/// The name as shown: the file's base name only, no control characters, at
/// most 120 characters.
pub fn clean_name(name: &str) -> Result<String, String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let clean: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    if clean.is_empty() {
        return Err("The file has no name".into());
    }
    Ok(clean)
}

/// Every stored file, newest first. A file that does not parse is skipped.
pub fn list(data_dir: &Path) -> Vec<OwnFile> {
    let mut out: Vec<OwnFile> = read_all(data_dir).into_iter().map(|(s, _)| s.meta).collect();
    out.sort_by(|a, b| b.added.cmp(&a.added).then(a.name.cmp(&b.name)));
    out
}

fn read_all(data_dir: &Path) -> Vec<(Stored, u64)> {
    let Ok(rd) = std::fs::read_dir(dir(data_dir)) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let p = e.path();
            let id = p.file_stem()?.to_str()?.to_string();
            if p.extension()? != "json" || !valid_id(&id) {
                return None;
            }
            let raw = std::fs::read(&p).ok()?;
            let mut s: Stored = serde_json::from_slice(&raw).ok()?;
            s.meta.id = id;
            s.meta.bytes = raw.len() as u64;
            Some((s, raw.len() as u64))
        })
        .collect()
}

/// Bytes of everything stored.
pub fn total_bytes(data_dir: &Path) -> u64 {
    crate::disk::tree_bytes(&dir(data_dir))
}

/// Every passage of every file, for the search worker.
pub fn passages(data_dir: &Path) -> Vec<Found> {
    let mut all = read_all(data_dir);
    all.sort_by_key(|a| std::cmp::Reverse(a.0.meta.added));
    all.into_iter()
        .flat_map(|(s, _)| {
            let f = s.meta.name.clone();
            s.passages.into_iter().map(move |p| Found { f: f.clone(), t: p.t, x: p.x })
        })
        .collect()
}

/// Check and store one file's passages. `room` is how many bytes the user's
/// allowed space still has. The same text added twice is stored once (the id
/// is its hash), under the newer name.
pub fn add(data_dir: &Path, name: &str, kind: &str, passages: Vec<Passage>, room: u64, now: u64) -> Result<OwnFile, String> {
    let name = clean_name(name)?;
    if !valid_kind(kind) {
        return Err("AInode reads PDF, Markdown and text files".into());
    }
    let passages: Vec<Passage> = passages
        .into_iter()
        .map(|p| Passage { t: p.t.chars().take(200).collect(), x: p.x.trim().to_string() })
        .filter(|p| !p.x.is_empty())
        .collect();
    if passages.is_empty() {
        return Err(format!("{name} has no text AInode can read"));
    }
    if passages.len() > MAX_PASSAGES || passages.iter().any(|p| p.x.chars().count() > MAX_PASSAGE_CHARS) {
        return Err(format!("{name} is too long to add"));
    }
    let chars: u64 = passages.iter().map(|p| p.x.chars().count() as u64).sum();
    if chars > MAX_FILE_CHARS {
        return Err(format!("{name} is too long to add: {chars} characters, at most {MAX_FILE_CHARS}"));
    }
    let mut h = Sha256::new();
    for p in &passages {
        h.update(p.t.as_bytes());
        h.update([0]);
        h.update(p.x.as_bytes());
        h.update([0]);
    }
    let id: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
    let meta = OwnFile { id: id.clone(), name, kind: kind.into(), added: now, chars, count: passages.len() as u64, bytes: 0 };
    let stored = Stored { meta: meta.clone(), passages };
    // As write_json_atomic will write it, so the room check counts real bytes.
    let raw = serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string())?;
    let size = raw.len() as u64;
    let path = dir(data_dir).join(format!("{id}.json"));
    let replaced = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > room.saturating_add(replaced) {
        return Err(format!(
            "Not enough room in the space you allowed for {} ({} MB). Remove a model or a file, or allow more.",
            meta.name,
            size.div_ceil(1_000_000)
        ));
    }
    if total_bytes(data_dir).saturating_sub(replaced).saturating_add(size) > MAX_TOTAL_BYTES {
        return Err("Your files already hold 200 MB of text, the most AInode keeps. Remove one first.".into());
    }
    state::create_private_dir_all(&dir(data_dir))?;
    state::write_json_atomic(&path, &stored)?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(size);
    Ok(OwnFile { bytes, ..meta })
}

pub fn remove(data_dir: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("unknown file".into());
    }
    match std::fs::remove_file(dir(data_dir).join(format!("{id}.json"))) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: &str, x: &str) -> Passage {
        Passage { t: t.into(), x: x.into() }
    }

    #[test]
    fn add_list_search_feed_and_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert!(list(d).is_empty());
        let a = add(d, "/Users/x/Notes/plan.md", "md", vec![p("plan.md > Goal", "Ship 0.7.0."), p("plan.md", "  ")], 1_000_000, 10).unwrap();
        assert_eq!(a.name, "plan.md", "only the base name is kept");
        assert_eq!(a.count, 1, "empty passages are dropped");
        assert!(a.bytes > 0 && total_bytes(d) == a.bytes);
        let b = add(d, "paper.pdf", "pdf", vec![p("paper.pdf, page 1", "Lattices.")], 1_000_000, 20).unwrap();
        assert_eq!(list(d).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["paper.pdf", "plan.md"]);
        let all = passages(d);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].f, "paper.pdf");
        // The same text again: one entry, not two.
        add(d, "plan-copy.md", "md", vec![p("plan.md > Goal", "Ship 0.7.0.")], 1_000_000, 30).unwrap();
        assert_eq!(list(d).len(), 2);
        remove(d, &b.id).unwrap();
        remove(d, &b.id).unwrap(); // idempotent
        assert_eq!(list(d).len(), 1);
    }

    #[test]
    fn refuses_what_does_not_fit_or_is_not_text() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert!(add(d, "scan.pdf", "pdf", vec![p("scan.pdf, page 1", "")], 1_000_000, 1).is_err());
        assert!(add(d, "a.docx", "docx", vec![p("a", "text")], 1_000_000, 1).is_err());
        assert!(add(d, "", "md", vec![p("a", "text")], 1_000_000, 1).is_err());
        let err = add(d, "big.md", "md", vec![p("a", &"x".repeat(1000))], 10, 1).unwrap_err();
        assert!(err.contains("Not enough room"), "{err}");
        assert!(add(d, "long.md", "md", vec![p("a", &"x".repeat(MAX_PASSAGE_CHARS + 1))], u64::MAX, 1).is_err());
        assert!(list(d).is_empty());
        // Ids are checked before any path is built from them.
        assert!(remove(d, "../consent").is_err());
        assert!(remove(d, "0123456789ABCDEF").is_err());
    }
}
