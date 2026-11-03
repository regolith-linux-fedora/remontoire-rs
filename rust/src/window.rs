//! The keybinding window: one expander per category in a single-column
//! list, placed at an edge of the monitor it is mapped on.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;

use crate::categories;
use crate::config::{self, Bindings};

/// Longer category and action names are shortened with an ellipsis (the
/// full name is in the tooltip), so the window stays narrower than the
/// screen.
const MAX_LABEL_WIDTH_CHARS: i32 = 60;
const EXPANDED_KEY: &str = "expanded-category-path-ids";

/// Build the (unshown) window for `bindings`.
pub fn build(app: &gtk::Application, bindings: &Bindings, settings: &gio::Settings) -> gtk::Window {
    let window = gtk::Window::builder().application(app).build();
    style_window(&window);

    if bindings.is_empty() {
        let warning = gtk::Label::new(Some("No Keybindings"));
        warning.style_context().add_class("error");
        window.add(&warning);
        return window;
    }

    let expanded = Rc::new(RefCell::new(categories::split(
        &settings.string(EXPANDED_KEY),
    )));

    let flowbox = gtk::FlowBox::new();
    flowbox.set_max_children_per_line(1);
    flowbox.set_min_children_per_line(1);
    flowbox.set_orientation(gtk::Orientation::Horizontal);
    flowbox.style_context().add_class("window");

    let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_propagate_natural_width(true);
    scroll.add(&flowbox);
    window.add(&scroll);

    for (category, keybindings) in bindings {
        let expander = add_category(&flowbox, category);
        expander.set_expanded(expanded.borrow().contains(category));
        let expanded = Rc::clone(&expanded);
        let settings = settings.clone();
        // Runs before the expander toggles: "not expanded" means opening.
        expander.connect_activate(move |expander| {
            let label = expander.label().map(|l| l.to_string()).unwrap_or_default();
            let mut expanded = expanded.borrow_mut();
            if expander.is_expanded() {
                expanded.remove(&label);
            } else {
                expanded.insert(&label);
            }
            // An unwritable key leaves the state unsaved, as before.
            let _ = settings.set_string(EXPANDED_KEY, &categories::join(&expanded));
        });

        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows.set_spacing(0);
        expander.add(&rows);
        for keybinding in keybindings {
            add_item(&rows, &keybinding.label, &keybinding.spec);
        }
    }
    window
}

fn add_category(container: &impl IsA<gtk::Container>, label: &str) -> gtk::Expander {
    let expander = gtk::Expander::new(Some(label));
    if let Some(label) = expander
        .label_widget()
        .and_then(|widget| widget.downcast::<gtk::Label>().ok())
    {
        limit_width(&label);
    }
    expander.style_context().add_class("category");
    container.add(&expander);
    expander
}

/// One row: the action on the left, its key tokens on the right in config
/// order.
fn add_item(container: &gtk::Box, action: &str, keybinding: &str) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.style_context().add_class("detail");

    let action_label = gtk::Label::new(Some(action));
    action_label.style_context().add_class("action");
    action_label.set_halign(gtk::Align::Start);
    limit_width(&action_label);
    row.pack_start(&action_label, true, true, 0);

    // Packed from the end, so the last token goes first.
    for token in config::parse_keybinding(keybinding).iter().rev() {
        let (text, style) = config::key_label(token);
        let key_label = gtk::Label::new(Some(text));
        key_label.style_context().add_class(style.css_class());
        key_label.set_halign(gtk::Align::End);
        row.pack_end(&key_label, false, false, 0);
    }
    container.add(&row);
}

fn limit_width(label: &gtk::Label) {
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(MAX_LABEL_WIDTH_CHARS);
    let text = label.label();
    if text.chars().count() > MAX_LABEL_WIDTH_CHARS as usize {
        label.set_tooltip_text(Some(&text));
    }
}

fn style_window(window: &gtk::Window) {
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_decorated(false);
    window.set_resizable(false);
    window.set_focus_on_map(false);
    window.set_deletable(false);
    window.set_accept_focus(false);
    window.set_type_hint(gdk::WindowTypeHint::Splashscreen);
    window.stick();
}

/// Where the window sits on its monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    North,
    South,
    East,
    West,
}

impl Position {
    /// The `window-position` setting; anything unknown is east.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            _ => Self::East,
        }
    }

    /// Top-left corner of a `size` window on `monitor`, `padding` away from
    /// the closest edge(s).
    pub fn place(
        self,
        monitor: &gdk::Rectangle,
        (width, height): (i32, i32),
        (x_padding, y_padding): (i32, i32),
    ) -> (i32, i32) {
        let centered_x = monitor.x() + (monitor.width() - width) / 2;
        let centered_y = monitor.y() + (monitor.height() - height) / 2;
        match self {
            Self::North => (centered_x, monitor.y() + y_padding),
            Self::South => (
                centered_x,
                monitor.y() + monitor.height() - height - y_padding,
            ),
            Self::West => (monitor.x() + x_padding, centered_y),
            Self::East => (
                monitor.x() + monitor.width() - width - x_padding,
                centered_y,
            ),
        }
    }
}

/// Keep `window` at `position` on the monitor the window manager mapped it
/// on, which is only known once the window exists on screen.
pub fn keep_placed(window: &gtk::Window, position: Position, padding: (i32, i32)) {
    window.connect_configure_event(move |window, _| {
        let monitor = window.window().and_then(|gdk_window| {
            gdk::Display::default().and_then(|display| display.monitor_at_window(&gdk_window))
        });
        if let Some(monitor) = monitor {
            let (x, y) = position.place(&monitor.geometry(), window.size(), padding);
            window.move_(x, y);
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_from_settings() {
        assert_eq!(Position::from_setting("north"), Position::North);
        assert_eq!(Position::from_setting("south"), Position::South);
        assert_eq!(Position::from_setting("west"), Position::West);
        assert_eq!(Position::from_setting("east"), Position::East);
        assert_eq!(Position::from_setting("unknown-position"), Position::East);
    }

    #[test]
    fn placement_on_a_lower_monitor() {
        let monitor = gdk::Rectangle::new(0, 800, 1280, 800);
        let size = (201, 51);
        let padding = (6, 7);
        assert_eq!(Position::North.place(&monitor, size, padding), (539, 807));
        assert_eq!(Position::South.place(&monitor, size, padding), (539, 1542));
        assert_eq!(Position::West.place(&monitor, size, padding), (6, 1174));
        assert_eq!(Position::East.place(&monitor, size, padding), (1073, 1174));
    }

    #[test]
    fn oversized_window_centres_like_c_division() {
        let monitor = gdk::Rectangle::new(0, 0, 100, 100);
        // (100 - 201) / 2 truncates toward zero: -50.
        assert_eq!(Position::North.place(&monitor, (201, 10), (0, 0)), (-50, 0));
    }
}
