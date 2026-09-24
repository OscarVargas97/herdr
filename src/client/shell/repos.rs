use crate::api::schema::{Method, RepoInfo, ResponseResult};

use super::render::put_text;
use super::*;

/// Repos change rarely; the list also refreshes on machine switch and config reload.
const REPO_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RepoTarget {
    /// Toggles the group's collapsed state.
    Group(String),
    /// Opens the group's shared parent folder as one space.
    OpenGroup(String),
    Repo(usize),
}

/// Text shown for a group header. `group` is the full path relative to the
/// root (e.g. "Multiplex/KMS" for repos two levels deep), which reads like a
/// breadcrumb instead of a folder name; every header should show just the
/// immediate parent folder's own name, so this looks up `group_label` from
/// any repo in that group instead.
fn group_display_label<'a>(repos: &'a [RepoInfo], group: &'a str) -> &'a str {
    repos
        .iter()
        .find(|repo| repo.group == group)
        .map(|repo| repo.group_label.as_str())
        .unwrap_or(group)
}

/// Flattened rows: one header per group followed by its repos unless collapsed.
pub(super) fn rows(repos: &[RepoInfo], collapsed: &HashSet<String>) -> Vec<RepoTarget> {
    let mut rows = Vec::new();
    for (index, repo) in repos.iter().enumerate() {
        if index == 0 || repos[index - 1].group != repo.group {
            rows.push(RepoTarget::Group(repo.group.clone()));
        }
        if !collapsed.contains(&repo.group) {
            rows.push(RepoTarget::Repo(index));
        }
    }
    rows
}

pub(super) fn render_repo_panel(
    buffer: &mut Buffer,
    area: Rect,
    workspaces: &[crate::protocol::ClientShellWorkspace],
    config: &ClientShellConfig,
    repos: &[RepoInfo],
    collapsed: &HashSet<String>,
    scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    if area.height < 2 {
        return;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(palette.surface_dim),
    );
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " repos",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.repo_body = body;
    if body.is_empty() {
        return;
    }
    let rows = rows(repos, collapsed);
    hits.repo_max_scroll = rows.len().saturating_sub(body.height as usize);
    *scroll = (*scroll).min(hits.repo_max_scroll);
    for (offset, row) in rows
        .iter()
        .skip(*scroll)
        .take(body.height as usize)
        .enumerate()
    {
        let rect = Rect::new(body.x, body.y + offset as u16, body.width, 1);
        let open = space_for(repos, row)
            .and_then(|(label, _)| workspaces.iter().find(|workspace| workspace.label == label));
        if open.is_some_and(|workspace| workspace.focused) {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        let text = match row {
            RepoTarget::Group(group) | RepoTarget::OpenGroup(group) => {
                let marker = if collapsed.contains(group) {
                    "▸"
                } else {
                    "▾"
                };
                let label = group_display_label(repos, group);
                format!(" {marker} {label}")
            }
            RepoTarget::Repo(index) => format!("   {}", repos[*index].name),
        };
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &text,
            Style::default().fg(if open.is_some() {
                palette.text
            } else {
                palette.overlay0
            }),
        );
        match row {
            // The marker toggles the group; the rest of the header opens its folder as a space.
            RepoTarget::Group(group) => {
                let marker = Rect {
                    width: rect.width.min(3),
                    ..rect
                };
                hits.repo_rows.push((marker, row.clone()));
                hits.repo_rows.push((
                    Rect {
                        x: marker.right(),
                        width: rect.width - marker.width,
                        ..rect
                    },
                    RepoTarget::OpenGroup(group.clone()),
                ));
            }
            _ => hits.repo_rows.push((rect, row.clone())),
        }
    }
}

/// Space label and cwd a row opens: the repo itself, or a group's shared parent folder.
/// Paths come from the machine that owns the repos, so they are passed through untouched.
fn space_for(repos: &[RepoInfo], target: &RepoTarget) -> Option<(String, String)> {
    match target {
        RepoTarget::Repo(index) => repos
            .get(*index)
            .map(|repo| (repo.name.clone(), repo.path.clone())),
        RepoTarget::Group(group) | RepoTarget::OpenGroup(group) => repos
            .iter()
            .find(|repo| &repo.group == group)
            .map(|repo| (repo.group_label.clone(), repo.group_path.clone())),
    }
}

impl ClientShellState {
    /// Fetches the active machine's repos, so each machine lists its own folders.
    pub(crate) fn tick_repos(&mut self, now: std::time::Instant, outcome: &mut ClientShellInput) {
        if self.repos_endpoint.as_ref() != Some(&self.active_endpoint_id) {
            // Another machine's paths are meaningless here; never show or open them.
            self.repos_endpoint = Some(self.active_endpoint_id.clone());
            self.repos_next_poll = None;
            self.repo_scroll = 0;
            if !self.repos.is_empty() {
                self.repos.clear();
                outcome.repaint = true;
            }
        }
        let request_pending = self
            .pending_requests
            .values()
            .any(|pending| matches!(pending.kind, PendingEndpointKind::RepoList));
        if request_pending
            || self.sidebar_collapsed
            || self.repos_next_poll.is_some_and(|next| now < next)
        {
            return;
        }
        let method = Method::RepoList(Default::default());
        self.repos_next_poll = Some(now + REPO_POLL_INTERVAL);
        // Endpoints without repo support stay silent instead of raising a notice every poll.
        if !self.endpoint_is_online(&self.active_endpoint_id)
            || !self.supports_endpoint_method(&method)
        {
            if !self.repos.is_empty() {
                self.repos.clear();
                outcome.repaint = true;
            }
            return;
        }
        self.push_endpoint_method_with_kind(method, PendingEndpointKind::RepoList, outcome);
    }

    pub(super) fn complete_repo_list(
        &mut self,
        result: Result<ResponseResult, ClientShellEndpointError>,
    ) -> bool {
        let repos = match result {
            Ok(ResponseResult::RepoList { repos }) => repos,
            _ => Vec::new(),
        };
        let changed = self.repos != repos;
        self.repos = repos;
        changed
    }

    pub(super) fn activate_repo_row(&mut self, target: RepoTarget, outcome: &mut ClientShellInput) {
        match target {
            RepoTarget::Group(group) => {
                if !self.collapsed_repo_groups.remove(&group) {
                    self.collapsed_repo_groups.insert(group);
                }
            }
            target => {
                let Some((label, path)) = space_for(&self.repos, &target) else {
                    return;
                };
                // ponytail: an open space is matched by label, so renaming it makes
                // the next click create a new one.
                let existing = self.snapshot.as_deref().and_then(|snapshot| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.label == label)
                        .map(|workspace| workspace.workspace_id.clone())
                });
                let method = match existing {
                    Some(workspace_id) => {
                        Method::WorkspaceFocus(crate::api::schema::WorkspaceTarget { workspace_id })
                    }
                    None => Method::WorkspaceCreate(crate::api::schema::WorkspaceCreateParams {
                        source_workspace_id: None,
                        cwd: Some(path),
                        focus: true,
                        label: Some(label),
                        env: Default::default(),
                    }),
                };
                self.push_endpoint_method(method, outcome);
            }
        }
        outcome.repaint = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(group: &str, name: &str) -> RepoInfo {
        RepoInfo {
            name: name.into(),
            path: format!("/home/me/{group}/{name}"),
            group: group.into(),
            group_label: group.into(),
            group_path: format!("/home/me/{group}"),
        }
    }

    #[test]
    fn rows_group_repos_and_hide_collapsed_members() {
        let repos = vec![
            repo("dev", "solo"),
            repo("personal", "app"),
            repo("work", "api"),
        ];
        let collapsed = HashSet::from(["personal".to_owned()]);
        assert_eq!(
            rows(&repos, &collapsed),
            vec![
                RepoTarget::Group("dev".into()),
                RepoTarget::Repo(0),
                RepoTarget::Group("personal".into()),
                RepoTarget::Group("work".into()),
                RepoTarget::Repo(2),
            ]
        );
        assert_eq!(
            space_for(&repos, &RepoTarget::OpenGroup("personal".into())),
            Some(("personal".into(), "/home/me/personal".into()))
        );
        assert_eq!(
            space_for(&repos, &RepoTarget::Repo(2)),
            Some(("api".into(), "/home/me/work/api".into()))
        );
    }

    #[test]
    fn group_header_label_uses_the_immediate_parent_not_the_full_relative_path() {
        // A repo two levels below the scan root (e.g. ~/Repos/Multiplex/KMS/app)
        // has `group` set to the full relative path, but `group_label` set to
        // just the immediate parent folder's own name.
        let nested = RepoInfo {
            name: "app".into(),
            path: "/home/me/Multiplex/KMS/app".into(),
            group: "Multiplex/KMS".into(),
            group_label: "KMS".into(),
            group_path: "/home/me/Multiplex/KMS".into(),
        };
        let repos = vec![nested];
        assert_eq!(group_display_label(&repos, "Multiplex/KMS"), "KMS");
    }
}
