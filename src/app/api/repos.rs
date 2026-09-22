use std::path::Path;

use crate::api::schema::{RepoInfo, ResponseResult};
use crate::app::App;

use super::responses::encode_success;

impl App {
    // ponytail: scans the roots on the server thread per request (a few ms for a
    // typical dev folder); cache it if roots grow large or clients poll faster.
    pub(super) fn handle_repo_list(&self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::RepoList {
                repos: scan(&self.state.sidebar_repos),
            },
        )
    }
}

/// Finds git repositories under the configured roots, sorted by group then name.
fn scan(config: &crate::config::ReposSidebarConfig) -> Vec<RepoInfo> {
    let mut repos = Vec::new();
    for root in &config.roots {
        let root = crate::worktree::expand_tilde_path(root);
        let root_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        scan_dir(&root, &root, &root_name, config.max_depth, &mut repos);
    }
    repos.sort_by(|a, b| {
        (a.group.to_lowercase(), a.name.to_lowercase())
            .cmp(&(b.group.to_lowercase(), b.name.to_lowercase()))
    });
    repos.dedup_by(|a, b| a.path == b.path);
    repos
}

fn scan_dir(root: &Path, dir: &Path, root_name: &str, depth: usize, repos: &mut Vec<RepoInfo>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        // A `.git` file marks a linked worktree or submodule; both open fine as spaces.
        if !path.join(".git").exists() {
            scan_dir(root, &path, root_name, depth - 1, repos);
            continue;
        }
        let group = dir
            .strip_prefix(root)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| root_name.to_owned());
        repos.push(RepoInfo {
            name,
            path: path.to_string_lossy().into_owned(),
            group,
            group_label: dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| root_name.to_owned()),
            group_path: dir.to_string_lossy().into_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_groups_repos_by_parent_and_skips_nested_repos() {
        let root = std::env::temp_dir()
            .join(format!("herdr-repos-scan-{}", std::process::id()))
            .join("dev");
        for dir in [
            "personal/app/.git",
            "personal/app/sub/.git",
            "work/api/.git",
            "solo/.git",
            "empty/x",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let config = crate::config::ReposSidebarConfig {
            roots: vec![root.to_string_lossy().into_owned()],
            max_depth: 3,
        };
        let repos = scan(&config);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        let found = repos
            .iter()
            .map(|repo| {
                (
                    repo.group.as_str(),
                    repo.name.as_str(),
                    repo.group_label.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            vec![
                ("dev", "solo", "dev"),
                ("personal", "app", "personal"),
                ("work", "api", "work"),
            ]
        );
        assert_eq!(repos[1].group_path, root.join("personal").to_string_lossy());
    }
}
