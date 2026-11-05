//! Scratchpad: a hidden place for windows that you can show and hide with one key.
//!
//! Windows in the scratchpad live on a hidden named workspace. They do not show in the
//! overview, in bars, or when you scroll through workspaces. `toggle-scratchpad` brings the
//! most recently hidden window to the current workspace as a centered floating window, and
//! hides it again when it is focused.

use niri_config::workspace::WorkspaceName;
use niri_config::{Action, Workspace as WorkspaceConfig, WorkspaceReference};
use niri_ipc::SizeChange;

use crate::niri::State;

/// Name of the hidden workspace that holds scratchpad windows.
pub const SCRATCHPAD_WORKSPACE: &str = "scratchpad";

/// Size of a tiled window when it first goes to the scratchpad, as a percent of the screen.
const SCRATCHPAD_SIZE_PERCENT: f64 = 70.;

fn is_scratchpad_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(SCRATCHPAD_WORKSPACE)
}

impl State {
    /// Returns whether the window is on the scratchpad workspace, and the id of its workspace.
    fn scratchpad_window_location(&self, id: u64) -> Option<(bool, u64)> {
        for (_, _, ws) in self.niri.layout.workspaces() {
            if ws.windows().any(|m| m.id().get() == id) {
                let on_scratchpad = ws.name().is_some_and(|n| is_scratchpad_name(n));
                return Some((on_scratchpad, ws.id().get()));
            }
        }
        None
    }

    /// Forgets windows that were closed.
    fn scratchpad_prune(&mut self) {
        let ids = std::mem::take(&mut self.niri.scratchpad_windows);
        self.niri.scratchpad_windows = ids
            .into_iter()
            .filter(|&id| self.scratchpad_window_location(id).is_some())
            .collect();
    }

    /// Makes sure the hidden scratchpad workspace exists. Returns false if it cannot.
    fn ensure_scratchpad_workspace(&mut self) -> bool {
        let existing = self
            .niri
            .layout
            .find_workspace_by_name(SCRATCHPAD_WORKSPACE)
            .map(|(_, ws)| (ws.id(), ws.hidden));

        match existing {
            None => {
                let config = WorkspaceConfig {
                    name: WorkspaceName(SCRATCHPAD_WORKSPACE.to_owned()),
                    open_on_output: None,
                    hidden: Some(true),
                    layout: None,
                };
                self.niri.layout.ensure_named_workspace(&config);
            }
            Some((ws_id, false)) => {
                // The workspace exists but is visible (for example, the user declared it in the
                // config without `hidden true`). Hide it, unless the user is looking at it.
                let active = self.niri.layout.active_workspace().map(|ws| ws.id());
                if active != Some(ws_id) {
                    self.niri
                        .layout
                        .toggle_workspace_visibility(SCRATCHPAD_WORKSPACE.to_owned());
                }
            }
            Some((_, true)) => (),
        }

        self.niri
            .layout
            .find_workspace_by_name(SCRATCHPAD_WORKSPACE)
            .is_some()
    }

    /// Sends a window to the scratchpad.
    pub fn scratchpad_hide(&mut self, id: u64) {
        if !self.ensure_scratchpad_workspace() {
            return;
        }

        self.scratchpad_prune();
        self.niri.scratchpad_windows.retain(|&w| w != id);
        self.niri.scratchpad_windows.push(id);

        // Make it floating first, so it comes back as a floating window. A tiled window gets a
        // comfortable floating size; a window that is already floating keeps its size.
        let was_floating = self.niri.layout.workspaces().any(|(_, _, ws)| {
            ws.windows()
                .find(|m| m.id().get() == id)
                .is_some_and(|m| ws.is_floating(&m.window))
        });
        self.do_action(Action::MoveWindowToFloatingById(id), false);
        if !was_floating {
            let size = SizeChange::SetProportion(SCRATCHPAD_SIZE_PERCENT);
            self.do_action(Action::SetWindowWidthById { id, change: size }, false);
            self.do_action(Action::SetWindowHeightById { id, change: size }, false);
        }
        self.do_action(
            Action::MoveWindowToWorkspaceById {
                window_id: id,
                reference: WorkspaceReference::Name(SCRATCHPAD_WORKSPACE.to_owned()),
                focus: false,
            },
            false,
        );
    }

    /// Hides the focused scratchpad window, or shows the most recently hidden one.
    pub fn scratchpad_toggle(&mut self) {
        self.scratchpad_prune();

        let focused = self.niri.layout.focus().map(|m| m.id().get());
        if let Some(id) = focused {
            if self.niri.scratchpad_windows.contains(&id) {
                self.scratchpad_hide(id);
                return;
            }
        }

        let Some(active_ws) = self.niri.layout.active_workspace().map(|ws| ws.id().get()) else {
            return;
        };

        let ids = self.niri.scratchpad_windows.clone();
        let locations: Vec<_> = ids
            .iter()
            .filter_map(|&id| Some((id, self.scratchpad_window_location(id)?)))
            .collect();

        // Prefer the most recently hidden window. Otherwise, take a scratchpad window that is
        // shown on another workspace.
        let pick = locations
            .iter()
            .rev()
            .find(|(_, (on_scratchpad, _))| *on_scratchpad)
            .or_else(|| locations.iter().rev().find(|(_, (_, ws))| *ws != active_ws))
            .map(|(id, _)| *id);

        let Some(id) = pick else {
            // All scratchpad windows are already here; focus the newest one.
            if let Some(&id) = ids.last() {
                self.do_action(Action::FocusWindow(id), false);
            }
            return;
        };

        self.do_action(
            Action::MoveWindowToWorkspaceById {
                window_id: id,
                reference: WorkspaceReference::Id(active_ws),
                focus: true,
            },
            false,
        );
        self.do_action(Action::MoveWindowToFloatingById(id), false);
        self.do_action(Action::FocusWindow(id), false);
        self.do_action(Action::CenterWindowById(id), false);
    }
}
