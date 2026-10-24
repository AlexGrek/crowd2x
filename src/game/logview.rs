//! The log panel: the highlighted unit's activity and important world notices.
//!
//! The simulation writes lines from any thread into a bounded lock-free queue
//! ([`crate::sim::Log`]) and never reads them back. This is the reader — the
//! one place that drains it, keeps the last few and puts them on screen.
//!
//! Draining rather than peeking is what makes the queue bounded work: a line
//! delivered here is gone from the queue, so the queue only ever holds what
//! has happened since the last frame. The ring below is the part that
//! remembers recent tagged messages so changing selection can refilter them,
//! even while paused. Only the last six matching messages appear on screen.

use bevy::prelude::*;
use std::collections::VecDeque;

use crate::sim::Uid;
use crate::sim::log::{CAPACITY, Entry};
use crate::state::AppState;
use crate::ui::{FONT_BODY, PANEL, TEXT_DIM};

use super::actors::Sim;
use super::hud;
use super::selection::Selected;

/// How many lines are kept and shown.
///
/// Small on purpose: this sits over the map, and a panel that grows to cover
/// the thing it is describing has stopped being a HUD.
pub const SHOWN: usize = 6;

/// The lines the panel is showing, oldest first.
///
/// A resource rather than only the `Text`, so a QA assertion can ask what the
/// simulation said without parsing what was drawn.
#[derive(Resource, Default)]
pub struct LogView {
    lines: Vec<String>,
    history: VecDeque<Entry>,
    selected: Option<Uid>,
}

impl LogView {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Whether any line held contains `needle`.
    pub fn contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|line| line.contains(needle))
    }

    /// Recent simulation output, including messages hidden by the HUD filter.
    /// QA's simulation assertions must not depend on who the player selected.
    pub fn recent(&self) -> impl Iterator<Item = &str> {
        self.history
            .iter()
            .rev()
            .take(SHOWN)
            .map(|entry| entry.message.as_str())
    }

    fn update(&mut self, fresh: Vec<Entry>, selected: Option<Uid>) -> bool {
        if fresh.is_empty() && self.selected == selected {
            return false;
        }
        self.selected = selected;
        for entry in fresh {
            self.history.push_back(entry);
            if self.history.len() > CAPACITY {
                self.history.pop_front();
            }
        }
        let mut lines: Vec<_> = self
            .history
            .iter()
            .rev()
            .filter(|entry| entry.visible_for(selected))
            .take(SHOWN)
            .map(|entry| entry.message.clone())
            .collect();
        lines.reverse();
        if self.lines == lines {
            return false;
        }
        self.lines = lines;
        true
    }
}

#[derive(Component)]
struct LogPanel;

pub struct LogViewPlugin;

impl Plugin for LogViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LogView>()
            .add_systems(OnExit(AppState::Game), clear)
            .add_systems(
                // Selection and panel actions settle in Update, including a
                // deselection with no new messages while the game is paused.
                PostUpdate,
                drain_log.run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

/// The panel, for whoever is laying the screen out.
///
/// Handed over as a bundle rather than spawned by a system of its own: it
/// hangs under the speed and pause controls in [`hud`]'s top right corner, and
/// where a panel sits is a fact about the screen rather than about the log. It
/// takes the same backing as the panel in the other corner, from there, so the
/// two cannot drift apart.
///
/// The width is capped because this is the one panel whose contents are not
/// written by us — an entity id is long, and a log line at its natural width
/// would reach across the map and into the numbers in the far corner.
pub fn panel() -> impl Bundle {
    (
        Name::new("game log"),
        Node {
            max_width: px(150),
            ..hud::panel_node()
        },
        BackgroundColor(PANEL),
        hud::absorbs_clicks(),
        children![(
            LogPanel,
            Text::new(String::new()),
            TextFont::from_font_size(FONT_BODY),
            TextColor(TEXT_DIM),
        )],
    )
}

/// Cleared on the way out, not on the way in: the world is rebuilt from its
/// seed each time the screen opens, so lines from the last visit would be
/// describing a world that no longer exists.
fn clear(mut view: ResMut<LogView>) {
    *view = LogView::default();
}

fn drain_log(
    sim: Res<Sim>,
    selected: Res<Selected>,
    mut view: ResMut<LogView>,
    mut panels: Query<&mut Text, With<LogPanel>>,
) {
    if !view.update(sim.0.log.drain_entries(), selected.get()) {
        return;
    }

    let text = view.lines.join("\n");
    for mut panel in &mut panels {
        **panel = text.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_keeps_the_most_recent_lines() {
        let mut view = LogView::default();
        let log = crate::sim::Log::new();
        for i in 0..SHOWN + 3 {
            log.important(format!("line {i}"));
        }
        view.update(log.drain_entries(), None);
        assert_eq!(view.lines().len(), SHOWN);
        // The end of a log is the part worth showing.
        assert_eq!(view.lines()[0], format!("line {}", 3));
        assert_eq!(view.lines()[SHOWN - 1], format!("line {}", SHOWN + 2));
    }

    #[test]
    fn a_short_log_is_kept_whole() {
        let mut view = LogView::default();
        let log = crate::sim::Log::new();
        log.important("only");
        view.update(log.drain_entries(), None);
        assert_eq!(view.lines(), ["only"]);
        assert!(view.contains("onl"));
        assert!(!view.contains("nothing"));
    }

    #[test]
    fn only_the_selected_units_activity_and_important_notices_are_visible() {
        use crate::sim::{EntityType, Log, Uid};
        let a = Uid::new(EntityType::Human, 1);
        let b = Uid::new(EntityType::Human, 2);
        let log = Log::new();
        log.unit(a, "a ate");
        log.unit(b, "b drank");
        log.push("street lights changed");
        log.important("saved the world");
        log.related(a, b, "a met b");

        let mut view = LogView::default();
        view.update(log.drain_entries(), Some(a));
        assert_eq!(view.lines(), ["a ate", "saved the world", "a met b"]);
        assert!(view.recent().any(|line| line == "b drank"));

        // No fresh log messages: a paused selection still changes the panel.
        assert!(view.update(vec![], Some(b)));
        assert_eq!(view.lines(), ["b drank", "saved the world", "a met b"]);
        assert!(view.update(vec![], None));
        assert_eq!(view.lines(), ["saved the world"]);
        assert!(!view.update(vec![], None));
    }

    #[test]
    fn hidden_crowd_activity_does_not_fill_the_visible_six_lines() {
        use crate::sim::{EntityType, Log, Uid};
        let a = Uid::new(EntityType::Human, 1);
        let b = Uid::new(EntityType::Human, 2);
        let log = Log::new();
        log.important("a world notice");
        log.unit(a, "a ate");
        for _ in 0..SHOWN * 3 {
            log.unit(b, "b drank");
        }
        let mut view = LogView::default();
        view.update(log.drain_entries(), Some(a));
        assert_eq!(view.lines(), ["a world notice", "a ate"]);
    }

    #[test]
    fn recent_history_stays_bounded_across_frames() {
        let mut view = LogView::default();
        let log = crate::sim::Log::new();
        for _ in 0..CAPACITY + 10 {
            log.push("routine chatter");
            view.update(log.drain_entries(), None);
        }
        assert_eq!(view.history.len(), CAPACITY);
        assert!(view.lines().is_empty());
    }
}
