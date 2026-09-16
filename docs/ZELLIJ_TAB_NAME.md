# Zellij Tab Names

jcode keeps the **zellij tab** it runs in named after what the session is working
on, so the tab bar explains what can be found in the tab instead of showing the
positional default (`Tab #1`).

## Why jcode pushes the name itself

jcode already publishes its session summary as the pane title with the OSC 2
escape sequence (`crossterm::terminal::SetTitle`), which zellij shows in the pane
frame. Zellij does **not** derive the tab name from that pane title: a tab keeps
its positional default until something explicitly renames it. Verified on zellij
0.44.3 with both an OSC 2 title and `zellij action rename-pane` on a single-pane
tab: `zellij action list-tabs` kept reporting `Tab #1`.

So every terminal-title update also runs:

```
zellij action rename-tab -t <tab_id> "<summary>"
```

`crates/jcode-tui/src/tui/zellij_tab.rs` owns this. It resolves the tab from
`zellij action list-panes --json` (zellij exports the pane id as
`ZELLIJ_PANE_ID`/`ZELLIJ`, but no tab id) and the call runs on a detached thread,
so a title update never blocks the UI. The name is only pushed when the tab does
not already carry it.

## Who owns the tab name

The summary is the same single-line string as the pane title: an explicit
`/rename` wins, then the current todo/goal title, then the generated session
title, behind the connection icon. It is stripped of control characters (an
embedded ESC from a model-generated title must not reach zellij's UI) and capped
at 48 characters.

Several jcode panes can share one tab, and they must not fight over the tab name.
The name is therefore pushed only when:

- this pane is the focused pane of its tab, or
- the tab holds exactly one pane.

Re-sync happens on every terminal-title update and whenever a pane gains focus
(`App::set_client_focus`), so the tab name follows the pane the user is on.

## Opting out

Set `JCODE_ZELLIJ_TAB_NAME=0` (also `false`, `off`, `no`) to leave zellij tab
names alone. Outside zellij the code returns immediately.
