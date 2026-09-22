use crate::api::schema::{PortInfo, PortStopParams, ResponseResult};
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    // ponytail: scans the process table and TCP listeners on the server thread per
    // request (a few ms); cache or move off-thread if clients poll much faster.
    fn pane_ports(&self) -> Vec<PortInfo> {
        let mut owners = Vec::new();
        for (ws_idx, workspace) in self.state.workspaces.iter().enumerate() {
            for pane_id in workspace.tabs.iter().flat_map(|tab| tab.layout.pane_ids()) {
                let Some((runtime, workspace_id)) = self.lookup_runtime(ws_idx, pane_id) else {
                    continue;
                };
                let (Some(pid), Some(public_pane_id)) =
                    (runtime.child_pid(), self.public_pane_id(ws_idx, pane_id))
                else {
                    continue;
                };
                owners.push((pid, public_pane_id, workspace_id));
            }
        }
        let root_pids = owners.iter().map(|(pid, ..)| *pid).collect::<Vec<_>>();
        crate::platform::listening_ports(&root_pids)
            .into_iter()
            .filter_map(|port| {
                let (_, pane_id, workspace_id) =
                    owners.iter().find(|(pid, ..)| *pid == port.root_pid)?;
                Some(PortInfo {
                    port: port.port,
                    pid: port.pid,
                    process: port.process,
                    pane_id: pane_id.clone(),
                    workspace_id: workspace_id.clone(),
                })
            })
            .collect()
    }

    pub(super) fn handle_port_list(&self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::PortList {
                ports: self.pane_ports(),
            },
        )
    }

    pub(super) fn handle_port_stop(&self, id: String, params: PortStopParams) -> String {
        // Only processes that currently own the port inside a pane may be stopped, so this
        // method cannot kill arbitrary host processes.
        if !self
            .pane_ports()
            .iter()
            .any(|port| port.port == params.port && port.pid == params.pid)
        {
            return encode_error(id, "port_not_found", "no pane process listens on that port");
        }
        crate::platform::signal_processes(&[params.pid], crate::platform::Signal::Kill);
        encode_success(id, ResponseResult::Ok {})
    }
}
