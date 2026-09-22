use serde::{Deserialize, Serialize};

/// A git repository found under the server's configured repo roots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RepoInfo {
    pub name: String,
    pub path: String,
    /// Parent directory relative to its scan root; repos sharing it form one group.
    pub group: String,
    /// Folder name of the group, used as the label of a space opened on the whole group.
    pub group_label: String,
    pub group_path: String,
}
