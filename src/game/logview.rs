//! The log panel: what the simulation has been saying.
//!
//! The simulation writes lines from any thread into a bounded lock-free queue
//! ([`crate::sim::Log`]) and never reads them back. This is the reader — the
//! one place that drains it, keeps the last few and puts them on screen.
//!
//! Draining rather than peeking is what makes the queue bounded work: a line
//! delivered here is gone from the queue, so the queue only ever holds what
//! has happened since the last frame. The ring below is the part that
//! remembers, and it is deliberately short, because a panel over a game is for
//! the last thing that happened and not for the history.

use bevy::prelude::*;

use crate::state::AppState;
use crate::ui::{FONT_BODY, PANEL, TEXT_DIM};

use super::actors::Sim;

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
}

impl LogView {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Whether any line held contains `needle`.
    pub fn contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|line| line.contains(needle))
    }

    /// Add a line, dropping the oldest once there are more than [`SHOWN`].
    fn push(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > SHOWN {
            let excess = self.lines.len() - SHOWN;
            self.lines.drain(..excess);
        }
    }
}

#[derive(Component)]
struct LogPanel;

pub struct LogViewPlugin;

impl Plugin for LogViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LogView>()
            .add_systems(OnEnter(AppState::Game), spawn_panel)
            .add_systems(OnExit(AppState::Game), clear)
            .add_systems(
                Update,
                drain_log.run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        Name::new("game log"),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(4),
            left: px(4),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(4), px(3)),
            ..default()
        },
        BackgroundColor(PANEL),
        DespawnOnExit(AppState::Game),
        children![(
            LogPanel,
            Text::new(String::new()),
            TextFont::from_font_size(FONT_BODY),
            TextColor(TEXT_DIM),
        )],
    ));
}

/// Cleared on the way out, not on the way in: the world is rebuilt from its
/// seed each time the screen opens, so lines from the last visit would be
/// describing a world that no longer exists.
fn clear(mut view: ResMut<LogView>) {
    view.lines.clear();
}

fn drain_log(sim: Res<Sim>, mut view: ResMut<LogView>, mut panels: Query<&mut Text, With<LogPanel>>) {
    let fresh = sim.0.log.drain();
    if fresh.is_empty() {
        return;
    }
    for line in fresh {
        view.push(line);
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
        for i in 0..SHOWN + 3 {
            view.push(format!("line {i}"));
        }
        assert_eq!(view.lines().len(), SHOWN);
        // The end of a log is the part worth showing.
        assert_eq!(view.lines()[0], format!("line {}", 3));
        assert_eq!(view.lines()[SHOWN - 1], format!("line {}", SHOWN + 2));
    }

    #[test]
    fn a_short_log_is_kept_whole() {
        let mut view = LogView::default();
        view.push("only".into());
        assert_eq!(view.lines(), ["only"]);
        assert!(view.contains("onl"));
        assert!(!view.contains("nothing"));
    }
}
