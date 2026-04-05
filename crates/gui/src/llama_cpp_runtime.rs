use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct BundledLlamaManifest {
    pub llama_cpp_tag: String,
    pub llama_server_version: String,
    pub platform: String,
    pub asset_name: String,
    pub source_release_url: String,
    pub llama_server_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatestLlamaRelease {
    pub tag_name: String,
    pub release_url: String,
    pub asset_name: String,
    pub download_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlamaCppUpdateNotice {
    pub current_tag: String,
    pub latest_tag: String,
    pub release_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseResponse {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubAssetResponse>,
}

#[derive(Debug, Deserialize)]
struct GithubAssetResponse {
    name: String,
    browser_download_url: String,
}

pub fn bundled_server_binary() -> Option<PathBuf> {
    resolve_dir_with_file(binary_name()).map(|dir| dir.join(binary_name()))
}

pub fn bundled_runtime_search_dirs() -> Vec<PathBuf> {
    candidate_runtime_dirs()
}

pub fn load_bundled_manifest() -> Result<BundledLlamaManifest, String> {
    let path = resolve_dir_with_file("manifest.json")
        .map(|dir| dir.join("manifest.json"))
        .ok_or_else(|| {
            format!(
                "内蔵 llama.cpp manifest が見つかりません。探索先: {}",
                format_search_dirs(&candidate_runtime_dirs())
            )
        })?;
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("内蔵 llama.cpp manifest の読み込みに失敗しました ({}): {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "内蔵 llama.cpp manifest の JSON 解析に失敗しました ({}): {e}",
            path.display()
        )
    })
}

pub fn fetch_latest_release() -> Result<LatestLlamaRelease, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10))
        .build();
    let resp = agent
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .set("User-Agent", "Open Agents")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GitHub Releases API への接続に失敗しました: {e}"))?;
    let body = resp
        .into_string()
        .map_err(|e| format!("GitHub Releases API 応答の読み取りに失敗しました: {e}"))?;
    let release: GithubReleaseResponse = serde_json::from_str(&body)
        .map_err(|e| format!("GitHub Releases API 応答の JSON 解析に失敗しました: {e}"))?;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name.contains("win-cpu-x64"))
        .ok_or_else(|| "Windows x64 CPU 向け llama.cpp asset が見つかりませんでした".to_string())?;
    Ok(LatestLlamaRelease {
        tag_name: release.tag_name,
        release_url: release.html_url,
        asset_name: asset.name,
        download_url: asset.browser_download_url,
    })
}

pub fn compute_update_notice(
    manifest: &BundledLlamaManifest,
    latest: &LatestLlamaRelease,
) -> Option<LlamaCppUpdateNotice> {
    let current_tag = manifest.llama_cpp_tag.trim();
    let latest_tag = latest.tag_name.trim();
    if current_tag.is_empty() || latest_tag.is_empty() || current_tag == latest_tag {
        return None;
    }
    Some(LlamaCppUpdateNotice {
        current_tag: current_tag.to_string(),
        latest_tag: latest_tag.to_string(),
        release_url: latest.release_url.clone(),
    })
}

fn resolve_dir_with_file(file_name: &str) -> Option<PathBuf> {
    resolve_dir_with_file_from_candidates(&candidate_runtime_dirs(), file_name)
}

fn resolve_dir_with_file_from_candidates(candidates: &[PathBuf], file_name: &str) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|dir| dir.join(file_name).is_file())
        .cloned()
}

fn candidate_runtime_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            push_unique_dir(&mut dirs, dir);
            if let Some(parent) = dir.parent() {
                push_unique_dir(&mut dirs, parent);
            }
        }
    }
    let repo_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/llama.cpp/windows-x64");
    push_unique_path(&mut dirs, repo_dir);
    dirs
}

fn push_unique_dir(target: &mut Vec<PathBuf>, dir: &Path) {
    push_unique_path(target, dir.to_path_buf());
}

fn push_unique_path(target: &mut Vec<PathBuf>, dir: PathBuf) {
    let normalized = dir.canonicalize().unwrap_or(dir);
    if !target.iter().any(|existing| existing == &normalized) {
        target.push(normalized);
    }
}

fn format_search_dirs(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "llama-server.exe"
    }
    #[cfg(not(windows))]
    {
        "llama-server"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!("open_agents_{label}_{stamp}"))
    }

    #[test]
    fn resolves_first_candidate_with_llama_server() {
        let first = unique_temp_dir("llama_first");
        let second = unique_temp_dir("llama_second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join(binary_name()), b"stub").unwrap();

        let resolved = resolve_dir_with_file_from_candidates(&[first.clone(), second.clone()], binary_name());
        assert_eq!(resolved, Some(second));

        let _ = fs::remove_dir_all(&first);
        let _ = fs::remove_dir_all(&resolved.unwrap());
    }

    #[test]
    fn update_notice_is_none_when_tags_match() {
        let manifest = BundledLlamaManifest {
            llama_cpp_tag: "b8668".into(),
            llama_server_version: "b8668".into(),
            platform: "windows-x64-cpu".into(),
            asset_name: "llama-b8668-bin-win-cpu-x64.zip".into(),
            source_release_url: "https://example.com".into(),
            llama_server_sha256: "abc".into(),
        };
        let latest = LatestLlamaRelease {
            tag_name: "b8668".into(),
            release_url: "https://example.com/release".into(),
            asset_name: "llama-b8668-bin-win-cpu-x64.zip".into(),
            download_url: "https://example.com/download".into(),
        };
        assert_eq!(compute_update_notice(&manifest, &latest), None);
    }

    #[test]
    fn update_notice_reports_latest_release_when_tags_differ() {
        let manifest = BundledLlamaManifest {
            llama_cpp_tag: "b8668".into(),
            llama_server_version: "b8668".into(),
            platform: "windows-x64-cpu".into(),
            asset_name: "llama-b8668-bin-win-cpu-x64.zip".into(),
            source_release_url: "https://example.com".into(),
            llama_server_sha256: "abc".into(),
        };
        let latest = LatestLlamaRelease {
            tag_name: "b8670".into(),
            release_url: "https://example.com/release".into(),
            asset_name: "llama-b8670-bin-win-cpu-x64.zip".into(),
            download_url: "https://example.com/download".into(),
        };
        let notice = compute_update_notice(&manifest, &latest).unwrap();
        assert_eq!(notice.current_tag, "b8668");
        assert_eq!(notice.latest_tag, "b8670");
    }
}
