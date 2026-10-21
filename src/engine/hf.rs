//! Downloads selected files of a Hugging Face model repository.

use std::{
    io::{Read as _, Write as _},
    path::{Component, Path, PathBuf},
};

use anyhow::{Result, anyhow};

/// Downloads every file in `repo` for which `wanted(path)` is true into
/// `dest` (keeping repo paths), reporting progress from 0 to 1.
pub fn download(repo: &str, wanted: impl Fn(&str) -> bool, dest: &Path, progress: impl Fn(f32)) -> Result<()> {
    let api = format!("https://huggingface.co/api/models/{repo}/tree/main?recursive=true");
    let listing: serde_json::Value = ureq::get(&api)
        .call()
        .map_err(|e| anyhow!("{e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| anyhow!("{e}"))?;
    let files: Vec<(String, u64)> = listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["type"] == "file")
        .filter_map(|f| {
            let path = f["path"].as_str()?.to_string();
            wanted(&path).then(|| (path, f["size"].as_u64().unwrap_or(0)))
        })
        .collect();
    if files.is_empty() {
        return Err(anyhow!("model listing was empty"));
    }
    let total: u64 = files.iter().map(|(_, s)| s).sum::<u64>().max(1);

    let mut done = 0u64;
    for (path, _) in &files {
        let target = safe_join(dest, path)?;
        std::fs::create_dir_all(target.parent().unwrap())?;
        let url = format!("https://huggingface.co/{repo}/resolve/main/{path}");
        let mut response = ureq::get(&url).call().map_err(|e| anyhow!("{path}: {e}"))?;
        let mut reader = response.body_mut().as_reader();
        let mut file = std::fs::File::create(&target)?;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            done += n as u64;
            progress(done as f32 / total as f32);
        }
    }
    Ok(())
}

/// Joins a repo-relative `path` onto `dest`, refusing anything that could
/// land outside it: absolute paths, `..`, backslashes or empty segments. The
/// file listing comes from the network, so it is not trusted.
fn safe_join(dest: &Path, path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    let plain = !path.is_empty()
        && !path.contains('\\')
        && relative.components().all(|c| matches!(c, Component::Normal(_)));
    if !plain {
        return Err(anyhow!("refusing to download unexpected path {path:?}"));
    }
    let target = dest.join(relative);
    if !target.starts_with(dest) {
        return Err(anyhow!("refusing to download unexpected path {path:?}"));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_plain_paths_only() {
        let dest = Path::new("/models/x");
        assert!(safe_join(dest, "coreml/ane/vocab.json").is_ok());
        assert!(safe_join(dest, "Encoder.mlmodelc/weights/weight.bin").is_ok());
        for bad in ["", "../escape", "a/../../b", "/etc/passwd", "a\\b", "./a", "a//b/.."] {
            assert!(safe_join(dest, bad).is_err(), "{bad:?} should be refused");
        }
    }
}
