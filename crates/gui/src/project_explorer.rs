//! プロジェクトエクスプローラ — Zed `project_panel` の可視エントリフラット化パターンを簡略移植。
//! 参照: zed `crates/project_panel`（`expanded_dir_ids` + 表示行の DFS）

use std::collections::HashSet;
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
                    vec![TreeNode::file("main.rs"), TreeNode::file("project_explorer.rs")],
                )],
            ),
            TreeNode::file("README.md"),
            TreeNode::file("Cargo.toml"),
        ],
    )
}

/// 初期展開（Zed がルート近辺を開いておく挙動の簡易版）
pub fn default_expanded_set() -> HashSet<Vec<String>> {
    HashSet::from([
        vec!["src".to_string()],
        vec!["src".to_string(), "components".to_string()],
        vec!["crates".to_string()],
        vec!["crates".to_string(), "gui".to_string()],
    ])
}

/// `expanded` に載っているディレクトリだけ子を列挙する DFS（visible entries の生成）
pub fn flatten_visible(node: &TreeNode, expanded: &HashSet<Vec<String>>, out: &mut Vec<VisibleRow>) {
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
}
