//! Keep the zellij tab name in sync with what a jcode pane is working on.
//!
//! Zellij never derives a tab name from a pane title, so a tab that hosts jcode
//! keeps showing the positional default (`Tab #1`) even though jcode already
//! publishes its session summary as the pane title (OSC 2). When jcode runs
//! inside zellij it therefore also pushes that summary to its own tab with
//! `zellij action rename-tab`, so the tab bar explains what can be found in the
//! tab instead of showing an opaque number.
//!
//! Only the pane the user is on names the tab, unless the tab holds a single
//! pane. Several jcode sessions sharing one tab would otherwise overwrite each
//! other's name on every title update.

use std::process::Command;

/// Opt-out switch: set to `0`, `false`, `off` or `no` to leave zellij tab names
/// alone.
const ENV_DISABLE: &str = "JCODE_ZELLIJ_TAB_NAME";

/// Zellij renders tab names in a narrow bar, so keep the pushed summary short.
const MAX_TAB_NAME_CHARS: usize = 48;

/// The `zellij action list-panes --json` fields this module needs.
#[derive(serde::Deserialize)]
struct ZellijPane {
    id: u64,
    tab_id: u64,
    is_focused: bool,
    tab_name: String,
}

/// Publish `summary` as the zellij tab name of this pane.
///
/// Cheap to call: it returns immediately outside zellij, and the tab lookup plus
/// `rename-tab` run on a detached thread so a title update never blocks the UI.
pub fn sync_tab_name(summary: &str) {
    if !in_zellij() || disabled() {
        return;
    }
    let name = normalize_tab_name(summary);
    if name.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let Some(tab_id) = owned_tab_id(&name) else {
            return;
        };
        rename_tab(tab_id, &name);
    });
}

/// Whether this process runs inside a zellij pane.
fn in_zellij() -> bool {
    ["ZELLIJ_SESSION_NAME", "ZELLIJ"]
        .iter()
        .any(|key| match std::env::var_os(key) {
            Some(value) => !value.is_empty(),
            None => false,
        })
}

fn disabled() -> bool {
    match std::env::var(ENV_DISABLE) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => false,
    }
}

/// Collapse a summary to a single safe tab-bar line. Session titles come from the
/// model, so control characters are stripped first: an embedded ESC would
/// otherwise inject escape sequences into zellij's own UI.
fn normalize_tab_name(summary: &str) -> String {
    let cleaned: String = summary
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_TAB_NAME_CHARS {
        return collapsed;
    }
    let truncated: String = collapsed
        .chars()
        .take(MAX_TAB_NAME_CHARS.saturating_sub(1))
        .collect();
    format!("{}…", truncated.trim_end())
}

/// Resolve the tab this pane lives in, but only when this pane is allowed to
/// name it: the pane the user is on, or any pane of a single-pane tab. Returns
/// `None` when the tab already carries `name`, so no zellij process is spawned
/// for an unchanged title.
fn owned_tab_id(name: &str) -> Option<u64> {
    let pane_id = current_pane_id()?;
    let raw = zellij_output(&["action", "list-panes", "--json"])?;
    tab_to_rename(&raw, pane_id, name)
}

/// Decide, from a `zellij action list-panes --json` payload, which tab (if any)
/// this pane should rename.
fn tab_to_rename(panes_json: &str, pane_id: u64, name: &str) -> Option<u64> {
    let panes: Vec<ZellijPane> = match serde_json::from_str(panes_json) {
        Ok(panes) => panes,
        Err(err) => {
            crate::logging::debug(&format!("zellij list-panes payload unreadable: {err}"));
            return None;
        }
    };
    let ours = panes.iter().find(|pane| pane.id == pane_id)?;
    if ours.tab_name == name {
        return None;
    }
    let panes_in_tab = panes
        .iter()
        .filter(|pane| pane.tab_id == ours.tab_id)
        .count();
    if !ours.is_focused && panes_in_tab > 1 {
        return None;
    }
    Some(ours.tab_id)
}

/// Zellij exports the pane id twice: `ZELLIJ_PANE_ID` on current versions and
/// the legacy `ZELLIJ`. Neither is guaranteed to be numeric, so fall through.
fn current_pane_id() -> Option<u64> {
    for key in ["ZELLIJ_PANE_ID", "ZELLIJ"] {
        let Some(raw) = std::env::var_os(key) else {
            continue;
        };
        let text = raw.to_string_lossy();
        match text.trim().parse::<u64>() {
            Ok(id) => return Some(id),
            Err(_) => continue,
        }
    }
    None
}

fn zellij_output(args: &[&str]) -> Option<String> {
    match Command::new("zellij").args(args).output() {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => {
            crate::logging::debug(&format!("zellij {:?} exited with {}", args, output.status));
            None
        }
        Err(err) => {
            crate::logging::debug(&format!("zellij {:?} could not run: {err}", args));
            None
        }
    }
}

fn rename_tab(tab_id: u64, name: &str) {
    let tab_id = tab_id.to_string();
    if zellij_output(&["action", "rename-tab", "-t", &tab_id, name]).is_none() {
        crate::logging::debug(&format!("zellij rename-tab -t {tab_id} did not succeed"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed copy of a real `list-panes --json` payload: tab 0 holds two panes
    /// (pane 0 focused), tab 1 holds panes 2-4 (pane 4 focused).
    const PANES: &str = r#"[
        {"id": 0, "is_focused": true,  "tab_id": 0, "tab_name": "Tab #1"},
        {"id": 1, "is_focused": false, "tab_id": 0, "tab_name": "Tab #1"},
        {"id": 2, "is_focused": false, "tab_id": 1, "tab_name": "Agents herstel"},
        {"id": 3, "is_focused": false, "tab_id": 1, "tab_name": "Agents herstel"},
        {"id": 4, "is_focused": true,  "tab_id": 1, "tab_name": "Agents herstel"}
    ]"#;

    #[test]
    fn tab_name_collapses_whitespace_and_strips_control_characters() {
        assert_eq!(
            normalize_tab_name("  Fix\n the\ttab   name  "),
            "Fix the tab name"
        );
        assert_eq!(
            normalize_tab_name("evil\x1b]2;injected\x07"),
            "evil ]2;injected"
        );
        assert_eq!(normalize_tab_name(""), "");
        assert_eq!(normalize_tab_name("   "), "");
    }

    #[test]
    fn tab_name_is_truncated_to_the_tab_bar_budget() {
        let long = "a".repeat(MAX_TAB_NAME_CHARS + 20);
        let name = normalize_tab_name(&long);
        assert_eq!(name.chars().count(), MAX_TAB_NAME_CHARS);
        assert!(name.ends_with('…'));
    }

    #[test]
    fn focused_pane_renames_its_own_tab() {
        // Pane 4 is focused and tab 1 still carries the previous name.
        assert_eq!(tab_to_rename(PANES, 4, "Nieuwe samenvatting"), Some(1));
        // A single-pane tab is renamed even without focus.
        assert_eq!(tab_to_rename(PANES, 0, "Alleen dit pane"), Some(0));
    }

    #[test]
    fn unfocused_pane_leaves_a_shared_tab_name_alone() {
        // Parallel sessions in one tab must not fight over the tab bar.
        assert_eq!(tab_to_rename(PANES, 2, "Andere samenvatting"), None);
    }

    #[test]
    fn unchanged_and_unknown_panes_skip_the_rename() {
        assert_eq!(tab_to_rename(PANES, 4, "Agents herstel"), None);
        assert_eq!(tab_to_rename(PANES, 99, "Onbekend pane"), None);
        assert_eq!(tab_to_rename("not json", 4, "stuk"), None);
    }
}
