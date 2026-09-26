// Help system - Self-documenting keybindings

use ratatui::{prelude::*, widgets::*};

/// Context in which a keybind is active
///
/// One variant per place input is actually handled, so a binding cannot end up
/// filed under a context that no longer exists. Names follow `App::tab` (see the
/// tab titles in `tui::mod`), plus `TriggerEdit` for the modal that opens over
/// whichever tab launched it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum KeyContext {
    Global,      // Available everywhere
    Info,        // Device Info tab (0)
    Depth,       // Key Depth tab (1)
    KeyMapping,  // Key Mapping tab (2)
    TriggerEdit, // Per-key / all-keys edit modal
    #[cfg(feature = "notify")]
    Notify, // Notify tab (3)
}

/// A single keybinding definition
pub(crate) struct Keybind {
    pub keys: &'static str,
    pub description: &'static str,
    pub context: KeyContext,
}

/// All TUI keybindings — single source of truth
///
/// Transcribed from the input handler in `tui::mod` (and `tabs::notify`), so it
/// lists what the TUI actually does rather than what it once did. Two rules
/// keep that honest:
///
/// * a binding is filed under the context whose `match` arm implements it, and
/// * anything the handler swallows without an effect is not listed at all.
///
/// When you add or move a key, update this in the same change — a help popup
/// that advertises a dead key, or omits a live one, is worse than none.
pub(crate) const TUI_KEYBINDS: &[Keybind] = &[
    // ── Global ──
    Keybind {
        keys: "q",
        description: "Quit",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Esc",
        description: "Close the top layer (help, hex entry, modal, picker)",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "? / F1",
        description: "Toggle this help",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Tab / Shift+Tab",
        description: "Next / previous tab",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Alt+1..5",
        description: "Jump straight to a tab",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "↑ / k, ↓ / j",
        description: "Move the selection",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "← / h, → / l",
        description: "Decrease / increase the selected value",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Shift+← / →",
        description: "Coarser step (±10 on RGB, ×10 on spinners)",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Ctrl+1..4",
        description: "Switch to profile 1-4",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Ctrl+p",
        description: "Cycle to the next profile",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "r",
        description: "Refresh device info",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "c",
        description: "Reconnect to the device",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "d",
        description: "Device picker",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "m",
        description: "Toggle key-depth monitoring",
        context: KeyContext::Global,
    },
    // ── Device Info tab ──
    Keybind {
        keys: "p",
        description: "Apply the per-key LED colour to the board",
        context: KeyContext::Info,
    },
    Keybind {
        keys: "#",
        description: "Type a hex colour on a colour field (then Enter)",
        context: KeyContext::Info,
    },
    Keybind {
        keys: "Enter / Backspace",
        description: "Confirm / erase in hex-colour entry",
        context: KeyContext::Info,
    },
    // ── Key Depth tab ──
    Keybind {
        keys: "v",
        description: "Toggle bar chart / time series",
        context: KeyContext::Depth,
    },
    Keybind {
        keys: "Space",
        description: "Select/deselect the key tracked in the time series",
        context: KeyContext::Depth,
    },
    Keybind {
        keys: "x",
        description: "Clear depth history",
        context: KeyContext::Depth,
    },
    // ── Key Mapping tab ──
    Keybind {
        keys: "v",
        description: "Toggle list / keyboard-layout view",
        context: KeyContext::KeyMapping,
    },
    Keybind {
        keys: "s",
        description: "Cycle the sort order",
        context: KeyContext::KeyMapping,
    },
    Keybind {
        keys: "Enter / e",
        description: "Edit the selected key",
        context: KeyContext::KeyMapping,
    },
    Keybind {
        keys: "g",
        description: "Edit all keys at once",
        context: KeyContext::KeyMapping,
    },
    Keybind {
        keys: "PgUp / PgDn",
        description: "Jump ten rows",
        context: KeyContext::KeyMapping,
    },
    Keybind {
        keys: "f",
        description: "Open the layer filter (Esc/Enter closes it)",
        context: KeyContext::KeyMapping,
    },
    // ── Trigger edit modal ──
    Keybind {
        keys: "Esc",
        description: "Close without saving",
        context: KeyContext::TriggerEdit,
    },
    Keybind {
        keys: "Enter",
        description: "Act on the focused field: open its picker, flip it, or save",
        context: KeyContext::TriggerEdit,
    },
    Keybind {
        keys: "Ctrl+s",
        description: "Save and close",
        context: KeyContext::TriggerEdit,
    },
    Keybind {
        keys: "Tab / ↓, Shift+Tab / ↑",
        description: "Next / previous field",
        context: KeyContext::TriggerEdit,
    },
    Keybind {
        keys: "← / h, → / l",
        description: "Change the field (Shift for a coarse step)",
        context: KeyContext::TriggerEdit,
    },
    Keybind {
        keys: "type / Backspace",
        description: "Filter the open picker / erase a character",
        context: KeyContext::TriggerEdit,
    },
    Keybind {
        keys: "Tab (in a picker)",
        description: "Add the highlighted key as a chord modifier",
        context: KeyContext::TriggerEdit,
    },
    // ── Notify tab ──
    Keybind {
        keys: "p",
        description: "Preview the effect on the keyboard",
        context: KeyContext::Notify,
    },
    Keybind {
        keys: "s",
        description: "Start/stop the notify daemon",
        context: KeyContext::Notify,
    },
    Keybind {
        keys: "c",
        description: "Clear all animations",
        context: KeyContext::Notify,
    },
    Keybind {
        keys: "w",
        description: "Save effects.toml",
        context: KeyContext::Notify,
    },
    Keybind {
        keys: "a / x / Delete",
        description: "Add / delete keyframe",
        context: KeyContext::Notify,
    },
    Keybind {
        keys: "Enter",
        description: "Edit keyframes / confirm",
        context: KeyContext::Notify,
    },
    Keybind {
        keys: "Tab / Shift+Tab",
        description: "Move focus (list → keyframes → variables)",
        context: KeyContext::Notify,
    },
];

/// Physical keyboard shortcuts from the manual
pub(crate) const KEYBOARD_SHORTCUTS: &[(&str, &str)] = &[
    // Profile switching
    ("Fn+F9", "Profile 1"),
    ("Fn+F10", "Profile 2"),
    ("Fn+F11", "Profile 3"),
    ("Fn+F12", "Profile 4"),
    // LED controls
    ("Fn+\\", "Cycle 7 colors + RGB"),
    ("Fn+↑", "Brightness up"),
    ("Fn+↓", "Brightness down"),
    ("Fn+←", "LED speed down"),
    ("Fn+→", "LED speed up"),
    ("Fn+=", "LED settings"),
    ("Fn+L", "LED mode cycle"),
    ("Fn+Home", "Effect 1-5"),
    ("Fn+PgUp", "Effect 6-10"),
    ("Fn+End", "Effect 11-15"),
    ("Fn+PgDn", "Effect 16-20"),
    // Connection modes
    ("Fn+E/R/T", "Bluetooth 1/2/3 (long=pair)"),
    ("Fn+Y", "2.4GHz mode (long=pair)"),
    // Utility
    ("Fn+Space", "Battery check"),
    ("Fn+W", "WASD/Arrow swap"),
    ("Fn+L_Win", "Win key lock"),
    ("Fn+I", "Insert"),
    ("Fn+P", "Print Screen"),
    ("Fn+C", "Calculator"),
    // Media (Windows)
    ("Fn+F1", "File Explorer"),
    ("Fn+F2", "Mail"),
    ("Fn+F3", "Browser"),
    ("Fn+F4", "Lock PC"),
    ("Fn+F5", "Display off"),
    ("Fn+F6/F8", "Play/Pause"),
    ("Fn+F7", "Volume down"),
    ("Fn+M", "Mute"),
    ("Fn+<", "Volume down"),
    ("Fn+>", "Volume up"),
];

/// Render help popup with all keybindings
pub(crate) fn render_help_popup(f: &mut Frame, area: Rect) {
    // Calculate popup size (80% width, 80% height)
    let popup_width = (area.width as f32 * 0.85) as u16;
    let popup_height = (area.height as f32 * 0.85) as u16;
    let popup_x = (area.width - popup_width) / 2;
    let popup_y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Split into two columns: TUI shortcuts and Keyboard shortcuts
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(popup_area);

    // Left column: TUI Keybindings
    let mut tui_lines: Vec<Line> = vec![Line::from(Span::styled(
        "── Global ──",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];

    let mut current_context = KeyContext::Global;
    for kb in TUI_KEYBINDS {
        if kb.context != current_context {
            current_context = kb.context;
            let section_name = match current_context {
                KeyContext::Global => "Global",
                KeyContext::Info => "Info Tab",
                KeyContext::Depth => "Depth Tab",
                KeyContext::KeyMapping => "Key Mapping Tab",
                KeyContext::TriggerEdit => "Trigger Edit Modal",
                #[cfg(feature = "notify")]
                KeyContext::Notify => "Notify Tab",
            };
            tui_lines.push(Line::from(""));
            tui_lines.push(Line::from(Span::styled(
                format!("── {section_name} ──"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        tui_lines.push(Line::from(vec![
            Span::styled(format!("{:14}", kb.keys), Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::raw(kb.description),
        ]));
    }

    let tui_help = Paragraph::new(tui_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" TUI Shortcuts [? to close] ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(tui_help, columns[0]);

    // Right column: Physical Keyboard Shortcuts
    let mut kb_lines: Vec<Line> = vec![Line::from(Span::styled(
        "── Profiles ──",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];

    let sections = [
        (0, 4, "Profiles"),
        (4, 12, "LED Controls"),
        (12, 14, "Connection"),
        (14, 19, "Utility"),
        (19, 29, "Media (Win)"),
    ];

    for (idx, (start, end, name)) in sections.into_iter().enumerate() {
        if idx > 0 {
            kb_lines.push(Line::from(""));
            kb_lines.push(Line::from(Span::styled(
                format!("── {name} ──"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        for (key, desc) in &KEYBOARD_SHORTCUTS[start..end] {
            kb_lines.push(Line::from(vec![
                Span::styled(format!("{key:14}"), Style::default().fg(Color::Magenta)),
                Span::raw(" "),
                Span::raw(*desc),
            ]));
        }
    }

    let kb_help = Paragraph::new(kb_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Physical Keyboard (Fn+key) ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(kb_help, columns[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render the help popup into a fixed-size buffer and return it as plain text.
    ///
    /// The table is data, so the only way it rots is quietly: a context that
    /// stops rendering a header, a key that is implemented but unlisted, or a
    /// listing for a handler that no longer exists. These tests read the real
    /// rendered output rather than the constant, so they notice both.
    fn rendered(width: u16, height: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|f| render_help_popup(f, f.area()))
            .expect("draw help popup");
        let buf = terminal.backend().buffer().clone();
        buf.content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .chunks(width as usize)
            .map(|row| row.iter().copied().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_context_gets_a_header() {
        let text = rendered(200, 120);
        for header in [
            "── Global ──",
            "── Info Tab ──",
            "── Depth Tab ──",
            "── Key Mapping Tab ──",
            "── Trigger Edit Modal ──",
            #[cfg(feature = "notify")]
            "── Notify Tab ──",
        ] {
            assert!(text.contains(header), "missing section header: {header}");
        }
    }

    #[test]
    fn binds_implemented_by_the_handler_are_listed() {
        // Keys the input handler acts on but which this table once omitted or
        // described wrongly. If one is dropped, the help is lying again.
        let text = rendered(200, 120);
        for (key, why) in [
            ("Ctrl+p", "cycles profiles, same as Ctrl+1..4"),
            ("Alt+1..5", "jumps straight to a tab"),
            ("#", "starts hex-colour entry on a colour field"),
            ("Backspace", "erases a hex digit or a picker filter"),
            ("Delete", "deletes a keyframe in the Notify tab"),
            ("Cycle the sort order", "'s' sorts; it does not set a mode"),
            ("Select/deselect", "Space tracks a key, it does not pause"),
        ] {
            assert!(text.contains(key), "{key} is undocumented ({why})");
        }
    }

    #[test]
    fn no_listing_for_handlers_that_no_longer_exist() {
        // The Triggers/Remaps tabs were replaced by the Key Mapping tab and the
        // edit modal; their bindings must not linger in the popup.
        let text = rendered(200, 120);
        for gone in ["Triggers Tab", "Remaps Tab", "SnapTap mode", "macro editor"] {
            assert!(!text.contains(gone), "help still advertises {gone}");
        }
    }
}
