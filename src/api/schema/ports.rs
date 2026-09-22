use serde::{Deserialize, Serialize};

/// A TCP port listened on by a process running inside a pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PortInfo {
    pub port: u16,
    pub pid: u32,
    pub process: String,
    pub pane_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PortStopParams {
    pub port: u16,
    pub pid: u32,
}
