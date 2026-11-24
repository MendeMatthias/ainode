//! Each model on disk once (0.7.1). The engine keeps a model as 4 MiB
//! `.piece` files in its store, and `exportmodelpath` writes a second, runnable
//! copy (E19), so every model took its size twice. AInode never serves (seed
//! manual), so once the runnable copy is verified against the publisher's
//! sha256, the pieces are only a second copy: AInode deletes them.
//!
//! What upstream v0.34.15 does then, observed with the 350M on 2026-10-13
//! (`lab/evidence/2026-10-13/store-once/`):
//! - running: `listmodels` says `complete: false`, `used_bytes` stays stale;
//!   `exportmodelpath` still returns the intact copy;
//! - after a restart: `used_bytes` 0, no job starts, the copy is untouched,
//!   `exportmodelpath` still returns it;
//! - but `getmodel` answers `status: "local"` and never fetches again, even
//!   with the copy gone, after `unhostmodel`, after a restart, after deleting
//!   the artifact folder: the helper's own `catalog.json` still lists the model
//!   as complete. Removing that entry while the helper is stopped is what
//!   makes `getmodel` fetch again (proven the same day). `forget_in_catalog`
//!   does exactly that, and nothing else in the file is touched.
//!
//! Nothing here runs while the helper could write the same files: deleting
//! pieces happens only with no download running, and the catalog is edited
//! only with the helper stopped (backend.rs).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{state, validate};

/// `<store>/artifacts/<artifact_id>`, only for a well-formed id.
pub fn artifact_dir(store: &Path, artifact_id: &str) -> Result<PathBuf, String> {
    if artifact_id.len() != 96 || !artifact_id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(format!("not an artifact id: {artifact_id:?}"));
    }
    Ok(store.join("artifacts").join(artifact_id))
}

/// Delete the `.piece` files of one artifact (two levels: `<file index>/<n>.piece`)
/// and keep its index and proofs. Returns the bytes freed. Refuses anything
/// that is not strictly inside `store` (symlinks resolved).
pub fn delete_pieces(store: &Path, artifact_id: &str) -> Result<u64, String> {
    let dir = artifact_dir(store, artifact_id)?;
    if !dir.exists() {
        return Ok(0);
    }
    if !validate::strictly_inside(store, &dir) {
        return Err(format!("refusing {}: not inside {}", dir.display(), store.display()));
    }
    let mut freed = 0u64;
    for sub in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let Ok(ft) = sub.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(sub.path()).map_err(|e| e.to_string())?.flatten() {
            let p = f.path();
            let is_piece = p.extension().is_some_and(|x| x == "piece");
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            if is_piece && m.is_file() {
                std::fs::remove_file(&p).map_err(|e| format!("delete {}: {e}", p.display()))?;
                freed += m.len();
            }
        }
    }
    Ok(freed)
}

/// Delete one artifact's whole folder in the store (index, proofs, pieces).
pub fn delete_artifact(store: &Path, artifact_id: &str) -> Result<(), String> {
    let dir = artifact_dir(store, artifact_id)?;
    match std::fs::symlink_metadata(&dir) {
        Err(_) => Ok(()),
        Ok(m) if m.is_dir() && validate::strictly_inside(store, &dir) => {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("delete {}: {e}", dir.display()))
        }
        Ok(_) => Err(format!("refusing to delete {}", dir.display())),
    }
}

/// Drop the helper's record of one artifact from its `catalog.json`
/// (`{"models": [{"artifact_id": ...}, ...], ...}`), so `getmodel` fetches it
/// again. Only with the helper stopped. Returns whether an entry was removed.
/// A file of any other shape is left alone.
pub fn forget_in_catalog(catalog: &Path, artifact_id: &str) -> Result<bool, String> {
    artifact_dir(Path::new("/"), artifact_id)?; // the id check
    let raw = match std::fs::read(catalog) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    let mut v: Value = serde_json::from_slice(&raw).map_err(|e| format!("the engine's catalog is not JSON: {e}"))?;
    let models = v
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or("the engine's catalog has no models list; left as it is")?;
    let before = models.len();
    models.retain(|m| m.get("artifact_id").and_then(Value::as_str) != Some(artifact_id));
    if models.len() == before {
        return Ok(false);
    }
    state::write_json_atomic(catalog, &v)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "bf29ca79102f2403778fef5e351d65cdb1d45564e018d87f8741fc1731bf080c1efcb2668b7a7746acd2876de2c01df5";

    #[test]
    fn deletes_only_pieces_of_that_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("store");
        let a = store.join("artifacts").join(A);
        for (rel, n) in [("0/0.piece", 10), ("0/index.json", 3), ("1/0.piece", 20), ("1/0.proof.json", 4), ("1/1.piece", 30)] {
            let p = a.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, vec![b'x'; n]).unwrap();
        }
        let other = store.join("artifacts").join("c".repeat(96)).join("0/0.piece");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, b"keep").unwrap();
        assert_eq!(delete_pieces(&store, A).unwrap(), 60);
        assert!(a.join("0/index.json").exists() && a.join("1/0.proof.json").exists());
        assert!(!a.join("1/1.piece").exists());
        assert!(other.exists(), "another artifact is untouched");
        assert_eq!(delete_pieces(&store, A).unwrap(), 0, "a second run frees nothing");
        assert!(delete_pieces(&store, "../../etc").is_err());
        delete_artifact(&store, A).unwrap();
        assert!(!a.exists());
        assert!(other.exists());
    }

    #[test]
    fn forgets_one_entry_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let cat = tmp.path().join("catalog.json");
        let keep = "d".repeat(96);
        std::fs::write(
            &cat,
            serde_json::json!({"models": [{"artifact_id": A, "incomplete": false}, {"artifact_id": keep}], "peers": ["x:1"]}).to_string(),
        )
        .unwrap();
        assert!(forget_in_catalog(&cat, A).unwrap());
        let v: Value = serde_json::from_slice(&std::fs::read(&cat).unwrap()).unwrap();
        assert_eq!(v["models"].as_array().unwrap().len(), 1);
        assert_eq!(v["models"][0]["artifact_id"], keep);
        assert_eq!(v["peers"][0], "x:1");
        assert!(!forget_in_catalog(&cat, A).unwrap(), "already gone");
        std::fs::write(&cat, r#"{"something":"else"}"#).unwrap();
        assert!(forget_in_catalog(&cat, A).is_err(), "an unknown shape is left alone");
        assert!(!forget_in_catalog(&tmp.path().join("missing.json"), A).unwrap());
    }
}
