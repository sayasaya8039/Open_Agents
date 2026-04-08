use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BundledLlamaBackend {
    Cuda,
    Vulkan,
    Cpu,
}

impl BundledLlamaBackend {
    pub const ALL: [Self; 3] = [Self::Cuda, Self::Vulkan, Self::Cpu];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cuda => "CUDA",
            Self::Vulkan => "Vulkan",
            Self::Cpu => "CPU",
        }
    }

    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
            Self::Cpu => "cpu",
        }
    }

    fn allows_legacy_root(self) -> bool {
        matches!(self, Self::Cuda)
    }
}

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
pub struct BundledLlamaRuntime {
    pub backend: BundledLlamaBackend,
    pub dir: PathBuf,
    pub binary_path: PathBuf,
    pub manifest: BundledLlamaManifest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundledLlamaRuntimeStatus {
    pub backend: BundledLlamaBackend,
    pub manifest: Option<BundledLlamaManifest>,
    pub error: Option<String>,
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

pub fn bundled_runtime_search_dirs_for_backend(backend: BundledLlamaBackend) -> Vec<PathBuf> {
    candidate_runtime_dirs_for_backend(backend)
}

pub fn upstream_runtime_search_dirs_for_backend(backend: BundledLlamaBackend) -> Vec<PathBuf> {
    candidate_upstream_runtime_dirs_for_backend(backend)
}

pub fn load_bundled_runtime_for_backend(
    backend: BundledLlamaBackend,
) -> Result<BundledLlamaRuntime, String> {
    let search_dirs = bundled_runtime_search_dirs_for_backend(backend);
    let dir = resolve_runtime_dir_from_candidates(&search_dirs).ok_or_else(|| {
        format!(
            "{} 向け内蔵 llama.cpp runtime が見つかりません。探索先: {}",
            backend.label(),
            format_search_dirs(&search_dirs)
        )
    })?;
    let binary_path = dir.join(binary_name());
    let manifest = load_manifest_from_dir(&dir, backend)?;
    Ok(BundledLlamaRuntime {
        backend,
        dir,
        binary_path,
        manifest,
    })
}

pub fn load_upstream_runtime_for_backend(
    backend: BundledLlamaBackend,
) -> Result<BundledLlamaRuntime, String> {
    let search_dirs = upstream_runtime_search_dirs_for_backend(backend);
    let dir = resolve_runtime_dir_from_candidates(&search_dirs).ok_or_else(|| {
        format!(
            "{} 向け upstream llama.cpp runtime が見つかりません。探索先: {}",
            backend.label(),
            format_search_dirs(&search_dirs)
        )
    })?;
    let binary_path = dir.join(binary_name());
    let manifest = load_manifest_from_dir(&dir, backend)?;
    Ok(BundledLlamaRuntime {
        backend,
        dir,
        binary_path,
        manifest,
    })
}

pub fn probe_bundled_runtime_statuses() -> Vec<BundledLlamaRuntimeStatus> {
    BundledLlamaBackend::ALL
        .into_iter()
        .map(|backend| match load_bundled_runtime_for_backend(backend) {
            Ok(runtime) => BundledLlamaRuntimeStatus {
                backend,
                manifest: Some(runtime.manifest),
                error: None,
            },
            Err(error) => BundledLlamaRuntimeStatus {
                backend,
                manifest: None,
                error: Some(error),
            },
        })
        .collect()
}

fn load_manifest_from_dir(
    dir: &Path,
    backend: BundledLlamaBackend,
) -> Result<BundledLlamaManifest, String> {
    let path = dir.join("manifest.json");
    let raw = fs::read_to_string(&path).map_err(|e| {
        format!(
            "{} 向け内蔵 llama.cpp manifest の読み込みに失敗しました ({}): {e}",
            backend.label(),
            path.display()
        )
    })?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "{} 向け内蔵 llama.cpp manifest の JSON 解析に失敗しました ({}): {e}",
            backend.label(),
            path.display(),
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

#[cfg(test)]
fn resolve_dir_with_file_from_candidates(
    candidates: &[PathBuf],
    file_name: &str,
) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|dir| dir.join(file_name).is_file())
        .cloned()
}

fn resolve_runtime_dir_from_candidates(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|dir| dir.join(binary_name()).is_file() && dir.join("manifest.json").is_file())
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
    let repo_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/llama.cpp/windows-x64");
    push_unique_path(&mut dirs, repo_dir);
    dirs
}

fn candidate_runtime_dirs_for_backend(backend: BundledLlamaBackend) -> Vec<PathBuf> {
    let roots = candidate_runtime_dirs();
    let mut dirs = Vec::new();
    for root in roots {
        if backend.allows_legacy_root() {
            push_unique_path(&mut dirs, root.clone());
        }
        push_unique_path(&mut dirs, root.join(backend.dir_name()));
    }
    dirs
}

fn candidate_upstream_runtime_dirs_for_backend(backend: BundledLlamaBackend) -> Vec<PathBuf> {
    let roots = candidate_runtime_dirs();
    let mut dirs = Vec::new();
    for root in roots {
        push_unique_path(&mut dirs, root.join("upstream").join(backend.dir_name()));
    }
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

        let resolved =
            resolve_dir_with_file_from_candidates(&[first.clone(), second.clone()], binary_name());
        assert_eq!(resolved, Some(second));

        let _ = fs::remove_dir_all(&first);
        let _ = fs::remove_dir_all(&resolved.unwrap());
    }

    #[test]
    fn cuda_backend_search_dirs_include_legacy_root() {
        let dirs = bundled_runtime_search_dirs_for_backend(BundledLlamaBackend::Cuda);
        assert!(!dirs.is_empty());
        let repo_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/llama.cpp/windows-x64");
        let normalized_repo = repo_dir.canonicalize().unwrap_or(repo_dir);
        assert!(dirs.iter().any(|dir| dir == &normalized_repo));
    }

    #[test]
    fn vulkan_backend_search_dirs_include_subdirectory() {
        let dirs = bundled_runtime_search_dirs_for_backend(BundledLlamaBackend::Vulkan);
        assert!(!dirs.is_empty());
        assert!(dirs.iter().any(|dir| dir.ends_with("vulkan")));
    }

    #[test]
    fn cpu_backend_search_dirs_only_use_cpu_subdirectory() {
        let dirs = bundled_runtime_search_dirs_for_backend(BundledLlamaBackend::Cpu);
        assert!(!dirs.is_empty());
        assert!(dirs.iter().all(|dir| dir.ends_with("cpu")));
    }

    #[test]
    fn upstream_cuda_search_dirs_use_upstream_subdirectory() {
        let dirs = upstream_runtime_search_dirs_for_backend(BundledLlamaBackend::Cuda);
        assert!(!dirs.is_empty());
        assert!(dirs
            .iter()
            .all(|dir| dir.ends_with(Path::new("upstream").join("cuda"))));
    }

    #[test]
    fn upstream_cpu_search_dirs_use_upstream_subdirectory() {
        let dirs = upstream_runtime_search_dirs_for_backend(BundledLlamaBackend::Cpu);
        assert!(!dirs.is_empty());
        assert!(dirs
            .iter()
            .all(|dir| dir.ends_with(Path::new("upstream").join("cpu"))));
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
