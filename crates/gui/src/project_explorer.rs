//! プロジェクトエクスプローラ — Zed `project_panel` の可視エントリフラット化パターンを簡略移植。
//! 参照: zed `crates/project_panel`（`expanded_dir_ids` + 表示行の DFS）

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
/// ファイルツリー 1 ノード（Zed の Entry ツリーに相当する簡易版）
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

/// パネルに並べる 1 行（Zed `visible_entries` / `EntryDetails` の最小相当）
#[derive(Clone, Debug)]
pub struct VisibleRow {
    /// ルートからのパスセグメント（ワークスペース相対）
    pub path: Vec<String>,
    pub depth: usize,
    pub is_dir: bool,
    /// ディレクトリのみ意味あり — 現在展開されているか
    pub is_expanded: bool,
}

impl TreeNode {
    pub fn dir(name: impl Into<String>, children: Vec<TreeNode>) -> Self {
        Self {
            name: name.into(),
            is_dir: true,
            children,
        }
    }

    pub fn file(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_dir: false,
            children: vec![],
        }
    }
}

/// Zed 風デモツリー（ネストした src / components 構造）
pub fn default_sample_tree() -> TreeNode {
    TreeNode::dir(
        "",
        vec![
            TreeNode::dir(
                "src",
                vec![
                    TreeNode::dir(
                        "components",
                        vec![
                            TreeNode::file("EditorCore.tsx"),
                            TreeNode::file("ChatWindow.tsx"),
                        ],
                    ),
                    TreeNode::dir("hooks", vec![TreeNode::file("useAgent.ts")]),
                    TreeNode::file("App.tsx"),
                    TreeNode::file("index.tsx"),
                ],
            ),
            TreeNode::dir(
                "crates",
                vec![TreeNode::dir(
                    "gui",
                    vec![
                        TreeNode::file("main.rs"),
                        TreeNode::file("project_explorer.rs"),
                    ],
                )],
            ),
            TreeNode::file("README.md"),
            TreeNode::file("Cargo.toml"),
        ],
    )
}

/// 初期展開（Zed がルート近辺を開いておく挙動の簡易版 — デモツリー用）
pub fn default_expanded_set() -> HashSet<Vec<String>> {
    HashSet::from([
        vec!["src".to_string()],
        vec!["src".to_string(), "components".to_string()],
        vec!["crates".to_string()],
        vec!["crates".to_string(), "gui".to_string()],
    ])
}

/// ルート直下のディレクトリだけ開く（実ディスクツリーの初期表示）
pub fn expanded_first_level(tree: &TreeNode) -> HashSet<Vec<String>> {
    tree.children
        .iter()
        .filter(|c| c.is_dir)
        .map(|c| vec![c.name.clone()])
        .collect()
}

/// ワークスペース相対セグメントを絶対パスへ
pub fn absolute_path(workspace: &Path, segments: &[String]) -> PathBuf {
    segments
        .iter()
        .fold(workspace.to_path_buf(), |acc, s| acc.join(s))
}

/// リフレッシュ後、存在しないディレクトリパスを展開集合から除去
pub fn prune_expanded(workspace: &Path, expanded: &mut HashSet<Vec<String>>) {
    expanded.retain(|segs| absolute_path(workspace, segs).is_dir());
}

/// `workspace_root` を走査してツリー構築（空・不存在なら空のルート）
pub fn read_tree_from_disk(workspace_root: &Path) -> io::Result<TreeNode> {
    if !workspace_root.is_dir() {
        return Ok(TreeNode::dir("", vec![]));
    }
    let children = read_directory_children(workspace_root)?;
    Ok(TreeNode::dir("", children))
}

fn read_directory_children(dir: &Path) -> io::Result<Vec<TreeNode>> {
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n != "." && n != "..")
                .unwrap_or(true)
        })
        .collect();

    entries.sort_by(|a, b| {
        let ad = a.path().is_dir();
        let bd = b.path().is_dir();
        bd.cmp(&ad).then_with(|| {
            a.file_name()
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.file_name().to_string_lossy().to_lowercase())
        })
    });

    let mut out = Vec::new();
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        if path.is_dir() {
            let kids = read_directory_children(&path).unwrap_or_default();
            out.push(TreeNode::dir(name, kids));
        } else {
            out.push(TreeNode::file(name));
        }
    }
    Ok(out)
}

/// `parent` 直下で `base` / `base 1` … と重複しない名前を返す
pub fn unique_child_name(parent: &Path, base: &str) -> String {
    if !parent.join(base).exists() {
        return base.to_string();
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{base} {n}");
        if !parent.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

/// `path` から `workspace` への相対セグメント（失敗時はファイル名のみ）
pub fn path_to_segments(workspace: &Path, path: &Path) -> Vec<String> {
    path.strip_prefix(workspace)
        .map(|p| {
            p.components()
                .filter_map(|c| {
                    use std::path::Component;
                    match c {
                        Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_else(|_| {
            path.file_name()
                .map(|s| vec![s.to_string_lossy().into_owned()])
                .unwrap_or_default()
        })
}

/// `expanded` に載っているディレクトリだけ子を列挙する DFS（visible entries の生成）
pub fn flatten_visible(
    node: &TreeNode,
    expanded: &HashSet<Vec<String>>,
    out: &mut Vec<VisibleRow>,
) {
    let mut prefix = Vec::new();
    flatten_children(&node.children, &mut prefix, 0, expanded, out);
}

fn flatten_children(
    children: &[TreeNode],
    parent_path: &mut Vec<String>,
    parent_depth: usize,
    expanded: &HashSet<Vec<String>>,
    out: &mut Vec<VisibleRow>,
) {
    for child in children {
        parent_path.push(child.name.clone());
        let path = parent_path.clone();
        let depth = parent_depth;

        if child.is_dir {
            let is_expanded = expanded.contains(&path);
            out.push(VisibleRow {
                path: path.clone(),
                depth,
                is_dir: true,
                is_expanded,
            });
            if is_expanded {
                flatten_children(
                    &child.children,
                    parent_path,
                    parent_depth + 1,
                    expanded,
                    out,
                );
            }
        } else {
            out.push(VisibleRow {
                path: path.clone(),
                depth,
                is_dir: false,
                is_expanded: false,
            });
        }
        parent_path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_respects_expanded() {
        let tree = default_sample_tree();
        let mut expanded = HashSet::new();
        expanded.insert(vec!["src".to_string()]);
        let mut rows = Vec::new();
        flatten_visible(&tree, &expanded, &mut rows);
        let names: Vec<_> = rows.iter().map(|r| r.path.join("/")).collect();
        assert!(names.iter().any(|n| n == "src"));
        assert!(names.iter().any(|n| n == "src/App.tsx"));
        assert!(!names.iter().any(|n| n == "src/components/EditorCore.tsx"));

        let mut expanded2 = expanded.clone();
        expanded2.insert(vec!["src".to_string(), "components".to_string()]);
        let mut rows2 = Vec::new();
        flatten_visible(&tree, &expanded2, &mut rows2);
        let names2: Vec<_> = rows2.iter().map(|r| r.path.join("/")).collect();
        assert!(names2.iter().any(|n| n == "src/components/EditorCore.tsx"));
    }

    #[test]
    fn unique_child_name_increments() {
        let tmp = std::env::temp_dir().join(format!("oa_explorer_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        assert_eq!(unique_child_name(&tmp, "New File"), "New File");
        fs::File::create(tmp.join("New File")).unwrap();
        assert_eq!(unique_child_name(&tmp, "New File"), "New File 1");
        let _ = fs::remove_dir_all(&tmp);
    }
}
