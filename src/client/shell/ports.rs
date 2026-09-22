use std::collections::HashMap;

use crate::api::schema::{Method, PortInfo, PortStopParams, ResponseResult};

use super::render::put_text;
use super::*;

const PORT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);
const FORWARD_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const FORWARD_BUTTON: &str = "+ forward";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PortTarget {
    Open(u16),
    /// Kills the pane process listening on the port.
    Stop {
        port: u16,
        pid: u32,
    },
    /// Closes a manual tunnel; the remote listener is not ours to kill.
    Close(u16),
    /// Asks for a port number to forward by hand.
    Forward,
}

/// A remote port tunneled to this machine through `ssh -L`, keyed by (machine, remote port).
pub(crate) struct PortForward {
    pub(super) local_port: u16,
    /// Added by hand, so it stays even though no pane process owns the listener.
    pub(super) manual: bool,
    child: std::process::Child,
}

impl Drop for PortForward {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) type PortForwards = HashMap<(ClientEndpointId, u16), PortForward>;

/// One line of the ports section: a pane listener, or a manual forward with no pane owner.
pub(super) struct PortRow<'a> {
    port: u16,
    pane: Option<&'a PortInfo>,
    local_port: Option<u16>,
}

pub(super) fn port_rows<'a>(
    ports: &'a [PortInfo],
    endpoint_id: &ClientEndpointId,
    forwards: &PortForwards,
) -> Vec<PortRow<'a>> {
    let local_port = |port: u16| {
        forwards
            .get(&(endpoint_id.clone(), port))
            .map(|forward| forward.local_port)
    };
    let mut rows = ports
        .iter()
        .map(|pane| PortRow {
            port: pane.port,
            pane: Some(pane),
            local_port: local_port(pane.port),
        })
        .collect::<Vec<_>>();
    let mut manual = forwards
        .iter()
        .filter(|((endpoint, port), _)| {
            endpoint == endpoint_id && !ports.iter().any(|pane| pane.port == *port)
        })
        .map(|((_, port), forward)| PortRow {
            port: *port,
            pane: None,
            local_port: Some(forward.local_port),
        })
        .collect::<Vec<_>>();
    manual.sort_by_key(|row| row.port);
    rows.extend(manual);
    rows
}

/// Rows the ports section needs: divider, title, spacer, and at least one content line.
/// Hidden only when the machine cannot report ports at all.
pub(super) fn ports_section_height(supported: bool, rows: usize, available: u16) -> u16 {
    if !supported {
        return 0;
    }
    (rows.max(1) as u16 + 3).min(available / 3)
}

pub(super) fn render_port_panel(
    buffer: &mut Buffer,
    area: Rect,
    workspaces: &[crate::protocol::ClientShellWorkspace],
    config: &ClientShellConfig,
    rows: &[PortRow<'_>],
    endpoint_id: &ClientEndpointId,
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
        " ports",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    // Manual forwarding tunnels a remote machine's port; the local machine has nothing to tunnel.
    if matches!(endpoint_id, ClientEndpointId::Ssh(_)) && config.mouse_capture {
        let width = (FORWARD_BUTTON.len() as u16 + 1).min(area.width);
        let button = Rect::new(area.right().saturating_sub(width), area.y + 1, width, 1);
        put_text(
            buffer,
            button.x,
            button.y,
            button.width,
            FORWARD_BUTTON,
            Style::default().fg(palette.accent),
        );
        hits.port_rows.push((button, PortTarget::Forward));
    }
    let body_height = area.height.saturating_sub(3);
    if rows.is_empty() && body_height > 0 {
        put_text(
            buffer,
            area.x,
            area.y + 3,
            area.width,
            " no ports in panes",
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        );
        return;
    }
    // ponytail: rows past the section height are cut off; add scrolling when a
    // session routinely listens on more ports than fit.
    for (offset, row) in rows.iter().take(body_height as usize).enumerate() {
        let rect = Rect::new(area.x, area.y + 3 + offset as u16, area.width, 1);
        let forwarded = row
            .local_port
            .map(|local| format!("→{local}"))
            .unwrap_or_default();
        let label = match row.pane {
            Some(pane) => {
                let space = workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == pane.workspace_id)
                    .map(|workspace| workspace.label.as_str())
                    .unwrap_or_default();
                format!(" :{}{forwarded} {} · {space}", row.port, pane.process)
            }
            None => format!(" :{}{forwarded} manual", row.port),
        };
        let stop = Rect {
            x: rect.right().saturating_sub(2),
            width: rect.width.min(2),
            ..rect
        };
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width.saturating_sub(stop.width),
            &label,
            Style::default().fg(if row.local_port.is_some() {
                palette.accent
            } else {
                palette.text
            }),
        );
        put_text(
            buffer,
            stop.x,
            stop.y,
            stop.width,
            "✕",
            Style::default().fg(palette.red),
        );
        hits.port_rows.push((
            stop,
            match row.pane {
                Some(pane) => PortTarget::Stop {
                    port: pane.port,
                    pid: pane.pid,
                },
                None => PortTarget::Close(row.port),
            },
        ));
        hits.port_rows.push((
            Rect {
                width: rect.width - stop.width,
                ..rect
            },
            PortTarget::Open(row.port),
        ));
    }
}

/// Uses the same local port as the remote one when it is free, like VS Code does.
fn free_local_port(preferred: u16) -> std::io::Result<u16> {
    match std::net::TcpListener::bind(("127.0.0.1", preferred)) {
        Ok(_) => Ok(preferred),
        Err(_) => Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?
            .local_addr()?
            .port()),
    }
}

fn spawn_forward(
    target: &str,
    local_port: u16,
    remote_port: u16,
    manual: bool,
) -> std::io::Result<PortForward> {
    let mut command = std::process::Command::new("ssh");
    crate::platform::configure_background_command(&mut command);
    command
        .args([
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "BatchMode=yes",
        ])
        // `localhost` lets sshd reach dev servers bound to either 127.0.0.1 or ::1.
        .arg("-L")
        .arg(format!("127.0.0.1:{local_port}:localhost:{remote_port}"))
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    Ok(PortForward {
        local_port,
        manual,
        child: command.spawn()?,
    })
}

/// Opens the browser once the tunnel accepts connections, without blocking the client loop.
fn open_when_ready(local_port: u16) {
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + FORWARD_READY_TIMEOUT;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], local_port));
        while std::time::Instant::now() < deadline {
            if std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(300))
                .is_ok()
            {
                let _ = crate::platform::open_url(&format!("http://localhost:{local_port}"));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });
}

impl ClientShellState {
    /// Polls the active endpoint's pane listeners while the expanded sidebar can show them.
    pub(crate) fn tick_ports(&mut self, now: std::time::Instant, outcome: &mut ClientShellInput) {
        self.reap_port_forwards(outcome);
        if self.ports_endpoint.as_ref() != Some(&self.active_endpoint_id) {
            // A list from another machine must not be shown or acted on here.
            self.ports_endpoint = Some(self.active_endpoint_id.clone());
            self.ports_next_poll = None;
            if !self.ports.is_empty() {
                self.ports.clear();
                outcome.repaint = true;
            }
        }
        let request_pending = self
            .pending_requests
            .values()
            .any(|pending| matches!(pending.kind, PendingEndpointKind::PortList));
        if request_pending
            || self.sidebar_collapsed
            || self.ports_next_poll.is_some_and(|next| now < next)
        {
            return;
        }
        let method = Method::PortList(Default::default());
        self.ports_next_poll = Some(now + PORT_POLL_INTERVAL);
        // Endpoints without port support stay silent instead of raising a notice every poll.
        let supported = self.endpoint_is_online(&self.active_endpoint_id)
            && self.supports_endpoint_method(&method);
        if supported != self.ports_supported {
            self.ports_supported = supported;
            outcome.repaint = true;
        }
        if !supported {
            if !self.ports.is_empty() {
                self.ports.clear();
                outcome.repaint = true;
            }
            return;
        }
        self.push_endpoint_method_with_kind(method, PendingEndpointKind::PortList, outcome);
    }

    /// Drops tunnels whose ssh exited, surfacing why so a failed forward is not silent.
    fn reap_port_forwards(&mut self, outcome: &mut ClientShellInput) {
        let exited = self
            .port_forwards
            .iter_mut()
            .filter_map(|(key, forward)| {
                forward.child.try_wait().ok().flatten().map(|_| key.clone())
            })
            .collect::<Vec<_>>();
        for key in exited {
            let Some(mut forward) = self.port_forwards.remove(&key) else {
                continue;
            };
            let mut stderr = String::new();
            if let Some(mut pipe) = forward.child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
            }
            let reason = stderr
                .lines()
                .last()
                .unwrap_or("ssh exited")
                .trim()
                .to_owned();
            self.set_endpoint_error(format!("port {} forward stopped: {reason}", key.1));
            outcome.repaint = true;
        }
    }

    pub(super) fn complete_port_list(
        &mut self,
        result: Result<ResponseResult, ClientShellEndpointError>,
    ) -> bool {
        let ports = match result {
            Ok(ResponseResult::PortList { ports }) => ports,
            _ => Vec::new(),
        };
        // Close automatic tunnels whose pane listener went away on the machine this list
        // describes; manual ones were never tied to a pane.
        let endpoint = self.active_endpoint_id.clone();
        self.port_forwards
            .retain(|(forward_endpoint, remote_port), forward| {
                forward.manual
                    || *forward_endpoint != endpoint
                    || ports.iter().any(|port| port.port == *remote_port)
            });
        let changed = self.ports != ports;
        self.ports = ports;
        changed
    }

    pub(super) fn activate_port_row(&mut self, target: PortTarget, outcome: &mut ClientShellInput) {
        match target {
            PortTarget::Open(port) => self.open_port(port, false),
            PortTarget::Stop { port, pid } => {
                self.port_forwards
                    .remove(&(self.active_endpoint_id.clone(), port));
                self.push_endpoint_method(Method::PortStop(PortStopParams { port, pid }), outcome);
                // Refresh right away so the stopped listener disappears without waiting a poll.
                self.ports_next_poll = None;
            }
            PortTarget::Close(port) => {
                self.port_forwards
                    .remove(&(self.active_endpoint_id.clone(), port));
            }
            PortTarget::Forward => {
                self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                    title: "forward port",
                    input: TextEditor::new("", true),
                    target: ClientRenameTarget::ForwardPort,
                }));
            }
        }
        outcome.repaint = true;
    }

    /// Forwards a port typed into the "forward port" prompt.
    pub(super) fn forward_port_from_input(&mut self, input: &str) {
        match input.trim().parse::<u16>() {
            Ok(port) if port > 0 => self.open_port(port, true),
            _ => self.set_endpoint_error(format!("'{}' is not a port number", input.trim())),
        }
    }

    fn open_port(&mut self, port: u16, manual: bool) {
        let ClientEndpointId::Ssh(profile_id) = &self.active_endpoint_id else {
            open_when_ready(port);
            return;
        };
        let key = (self.active_endpoint_id.clone(), port);
        if let Some(forward) = self.port_forwards.get(&key) {
            open_when_ready(forward.local_port);
            return;
        }
        let target = crate::client::endpoint::EndpointCatalog::load_profiles()
            .ok()
            .and_then(|profiles| {
                profiles
                    .into_iter()
                    .find(|profile| &profile.id == profile_id)
                    .map(|profile| profile.target)
            });
        let Some(target) = target else {
            self.set_endpoint_error("could not find this machine's SSH target to forward the port");
            return;
        };
        // ponytail: one `ssh -L` process per forwarded port, authenticated on its own;
        // multiplex over herdr's connection if key-less or MFA logins make that painful.
        match free_local_port(port)
            .and_then(|local_port| spawn_forward(&target, local_port, port, manual))
        {
            Ok(forward) => {
                // Manual forwards are often databases or other non-HTTP services, so like
                // VS Code they only open the tunnel; clicking the row opens the browser.
                if !manual {
                    open_when_ready(forward.local_port);
                }
                self.port_forwards.insert(key, forward);
            }
            Err(err) => {
                self.set_endpoint_error(format!("could not start ssh for port {port}: {err}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_section_shows_whenever_supported_and_caps_at_a_third() {
        assert_eq!(ports_section_height(false, 5, 30), 0);
        assert_eq!(ports_section_height(true, 0, 30), 4);
        assert_eq!(ports_section_height(true, 2, 30), 5);
        assert_eq!(ports_section_height(true, 20, 30), 10);
    }

    #[test]
    fn free_local_port_prefers_the_remote_port_and_falls_back_when_taken() {
        let taken = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let taken_port = taken.local_addr().unwrap().port();
        let fallback = free_local_port(taken_port).unwrap();
        assert_ne!(fallback, taken_port);
        drop(taken);
        assert_eq!(free_local_port(taken_port).unwrap(), taken_port);
    }
}
