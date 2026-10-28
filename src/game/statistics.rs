//! The statistics menu: the world in numbers, over the whole screen.
//!
//! `statistics`, beside `save` in the world's corner, opens it; `close` or
//! `esc` shuts it. It reads [`GameState::statistics`] — who is alive, what
//! the warehouse and the steel factory hold, who works at what, the dead —
//! and **keeps reading it
//! while it is open**: the world runs on behind it, and the numbers are
//! rewritten when they change, so it is a window on the world and not a
//! photograph of it. Worked out once a frame and only while the menu is
//! open, one walk of the arena, which is nothing beside a tick.
//!
//! A translucent modal: the world stays visible beneath it, with its
//! own [`Scope`], so the corner buttons behind it stop taking the pointer
//! and the highlight cannot be walked out of it.

use bevy::prelude::*;

use crate::sim::fields::Stage;
use crate::sim::statistics::FieldStock;
use crate::sim::{GameState, Statistics};
use crate::state::AppState;
use crate::ui::keyboard::MODAL_Z;
use crate::ui::nav::{Activated, Focus, Focusable, NavSystems, Scope};
use crate::ui::{button, hover_highlight, label, plain_button, FONT_BODY, FONT_TITLE, OVERLAY, TEXT};

use super::actors::Sim;

/// The focus layer the menu occupies while it is open.
const STATISTICS_MENU: u8 = 1;

/// The open statistics menu, if there is one. A resource, like
/// [`super::hud::SpawnMenu`], so [`super::leave`] can ask whether `esc`
/// closes it or leaves the game.
#[derive(Resource, Default)]
pub struct StatisticsMenu(pub Option<Entity>);

/// The button in the world's corner that opens the menu.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct OpenStatistics;

/// The menu's `close` button.
#[derive(Component)]
struct CloseStatistics;

/// The menu's text, rewritten as the world changes.
#[derive(Component)]
struct StatisticsText(bool);

pub struct StatisticsPlugin;

impl Plugin for StatisticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatisticsMenu>()
            .add_systems(OnExit(AppState::Game), forget_the_menu)
            .add_systems(
                Update,
                (open, close, hover_highlight::<OpenStatistics>, update_text)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// The button that opens the menu, for the corner to put where it likes.
pub fn open_button() -> impl Bundle {
    (OpenStatistics, plain_button("statistics", px(48)))
}

/// The overlay goes with every other `DespawnOnExit`; what points at it,
/// and the scope it raised, go here.
fn forget_the_menu(mut menu: ResMut<StatisticsMenu>, mut scope: ResMut<Scope>) {
    if menu.0.take().is_some() {
        *scope = Scope(0);
    }
}

/// A click on `statistics`, or a script pressing it by name.
fn open(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    clicked: Query<&Interaction, (With<OpenStatistics>, Changed<Interaction>)>,
    buttons: Query<(), With<OpenStatistics>>,
    mut menu: ResMut<StatisticsMenu>,
    mut scope: ResMut<Scope>,
    mut focus: ResMut<Focus>,
) {
    let pressed = clicked.iter().any(|interaction| *interaction == Interaction::Pressed)
        | activated.read().any(|Activated(entity)| buttons.contains(*entity));
    if !pressed || menu.0.is_some() || scope.0 != 0 {
        return;
    }

    let overlay = commands
        .spawn((
            Name::new("statistics menu"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(OVERLAY),
            GlobalZIndex(MODAL_Z),
            DespawnOnExit(AppState::Game),
            children![
                label("statistics", FONT_TITLE, TEXT),
                (Node { column_gap: px(6), ..default() }, children![
                    (StatisticsText(false), label(String::new(), FONT_BODY * 2.0 / 3.0, TEXT)),
                    (StatisticsText(true), label(String::new(), FONT_BODY * 2.0 / 3.0, TEXT)),
                ]),
            ],
        ))
        .id();
    let close = commands
        .spawn((
            CloseStatistics,
            button("close", Focusable::new(0, 0).in_scope(STATISTICS_MENU), px(40)),
            ChildOf(overlay),
        ))
        .id();
    focus.0 = Some(close);
    *scope = Scope(STATISTICS_MENU);
    menu.0 = Some(overlay);
}

/// `close` pressed.
fn close(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    buttons: Query<(), With<CloseStatistics>>,
    mut menu: ResMut<StatisticsMenu>,
    mut scope: ResMut<Scope>,
) {
    if activated.read().any(|Activated(entity)| buttons.contains(*entity)) {
        close_statistics(&mut commands, &mut menu, &mut scope);
    }
}

/// Close the menu, from its own button or from `esc` ([`super::leave`]).
pub(super) fn close_statistics(commands: &mut Commands, menu: &mut StatisticsMenu, scope: &mut Scope) {
    if let Some(overlay) = menu.0.take() {
        commands.entity(overlay).despawn();
    }
    *scope = Scope(0);
}

/// Keep the menu saying what is true, while it is open.
fn update_text(menu: Res<StatisticsMenu>, sim: Option<Res<Sim>>, mut texts: Query<(&StatisticsText, &mut Text)>) {
    if menu.0.is_none() {
        return;
    }
    let Some(sim) = sim else { return };
    let mut wanted = describe(&GameState::statistics(&sim.0));
    let reserve = sim.0.superfridge();
    if reserve.exists() {
        let dispatch = sim.0.dispatch();
        wanted.push_str(&format!(
            "\n\nreserve    {:.4} / {:.4} ({:.1}%)\ndispatch   {} waiting  {} delivering",
            reserve.level(sim.0.bins()), reserve.capacity(), reserve.percent(sim.0.bins()),
            dispatch.waiting(), dispatch.orders().len() - dispatch.waiting()
        ));
    }
    let (left, jobs) = columns(&wanted);
    for (column, mut text) in &mut texts {
        let wanted = if column.0 { &jobs } else { &left };
        // Compared before writing: the same string written again still costs
        // a re-layout.
        if **text != *wanted {
            **text = wanted.clone();
        }
    }
}

/// Keep fourteen job rows beside the overview so the menu fits a 180px canvas.
fn columns(text: &str) -> (String, String) {
    let Some((overview, rest)) = text.split_once("\njobs") else { return (text.to_string(), String::new()); };
    let Some((jobs, accounts)) = rest.split_once("\ndead") else { return (text.to_string(), String::new()); };
    (format!("{overview}\ndead{accounts}"), format!("jobs{jobs}"))
}

/// The menu's text: four sections, a line or a few each, in columns.
pub fn describe(stats: &Statistics) -> String {
    let mut out = String::new();
    let kinds: Vec<String> = stats.alive.iter().map(|(kind, count)| format!("{} {count}", plural(kind.name()))).collect();
    out.push_str(&format!("alive      {:>4}   {}\n", stats.living(), kinds.join("  ")));

    out.push('\n');
    for stock in &stats.sites {
        let name = stock.site;
        if stock.present {
            let items: Vec<String> = stock.items.iter().map(|(item, count)| format!("{count:>4}   {}", item.name())).collect();
            out.push_str(&format!("{name:<10} {}\n", items.join("  ")));
        } else {
            out.push_str(&format!("{name:<10}        none\n"));
        }
    }
    out.push_str(&fields(&stats.fields));

    out.push('\n');
    for (index, job) in stats.jobs.iter().enumerate() {
        let heading = if index == 0 { "jobs" } else { "" };
        out.push_str(&format!("{heading:<10} {:>4}   {} of {} posts\n", job.workers, plural(job.kind.name()), job.posts));
    }
    out.push_str(&format!("{:<10} {:>4}   without a job\n", "", stats.jobless));

    out.push('\n');
    out.push_str(&format!(
        "dead       {:>4}   lying {}  carried {}  cremated {}",
        stats.died, stats.lying, stats.carried, stats.cremated
    ));
    let economy = &stats.economy;
    out.push_str(&format!("\n\nbudget     {}   tax {}%\n", economy.budget, economy.tax_percent));
    if let Some(book) = economy.today {
        out.push_str(&format!("today      {}   wages {}\n", book.balance().signed(), book.wages));
        out.push_str(&format!("           care {}  taxes {}\n", book.care, book.taxes.signed()));
        out.push_str(&format!("           sales {}  estates {}\n", book.sales.signed(), book.estates.signed()));
    }
    out.push_str(&format!("cash       {}   {} held\n", economy.accounts, economy.cash));
    out.push_str(&format!("           average {}\n", economy.average));
    out.push_str(&format!("           {} in debt for {}\n", economy.in_debt, economy.debt));
    out.push_str(&format!("owed       {} shifts  {} care days", economy.shifts, economy.care_days));
    out
}

/// `fields       80   10 seedling  20 young  30 in ear  20 ripe  quality 64%`.
fn fields(stock: &FieldStock) -> String {
    let Some(quality) = stock.quality else {
        return format!("{:<10} {:>4}   nothing sown\n", "fields", 0);
    };
    let stages: Vec<String> = Stage::ALL.iter().zip(stock.stages).map(|(stage, count)| format!("{count} {}", stage.name())).collect();
    format!("{:<10} {:>4}   {}  quality {quality}%\n", "fields", stock.sown(), stages.join("  "))
}

/// "human" to "humans", "office worker" to "office workers".
fn plural(name: &str) -> String {
    format!("{name}s")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::statistics::{JobCount, SiteStock};
    use crate::sim::{EntityType, ItemKind, JobKind};

    fn site(site: &'static str, present: bool, scrap: u32, steel: u32) -> SiteStock {
        SiteStock { site, present, items: vec![(ItemKind::Scrap, scrap), (ItemKind::Steel, steel)] }
    }

    #[test]
    fn the_menu_reads_the_living_the_warehouse_the_jobs_and_the_dead() {
        let stats = Statistics {
            alive: vec![(EntityType::Human, 12), (EntityType::Dog, 1)],
            sites: vec![site("warehouse", true, 30, 6), site("factory", true, 3, 1)],
            fields: FieldStock { stages: [10, 20, 30, 20], quality: Some(64) },
            jobs: vec![
                JobCount { kind: JobKind::OfficeWorker, workers: 5, posts: 12 },
                JobCount { kind: JobKind::Guard, workers: 3, posts: 3 },
            ],
            jobless: 4,
            died: 3,
            lying: 1,
            carried: 1,
            cremated: 1,
            economy: Default::default(),
        };
        let text = describe(&stats);
        assert!(text.contains("alive        13   humans 12  dogs 1"), "{text}");
        assert!(text.contains("warehouse    30   scrap metal     6   steel"), "{text}");
        assert!(text.contains("factory       3   scrap metal     1   steel"), "{text}");
        assert!(text.contains("fields       80   10 seedling  20 young  30 in ear  20 ripe  quality 64%"), "{text}");
        assert!(text.contains("jobs          5   office workers of 12 posts"), "{text}");
        assert!(text.contains("              3   guards of 3 posts"), "{text}");
        assert!(text.contains("              4   without a job"), "{text}");
        assert!(text.contains("dead          3   lying 1  carried 1  cremated 1"), "{text}");
    }

    #[test]
    fn a_map_without_a_warehouse_or_a_factory_says_so() {
        let stats = Statistics {
            alive: vec![(EntityType::Human, 1)],
            sites: vec![site("warehouse", false, 0, 0), site("factory", false, 0, 0)],
            fields: FieldStock::default(),
            jobs: Vec::new(),
            jobless: 1,
            died: 0,
            lying: 0,
            carried: 0,
            cremated: 0,
            economy: Default::default(),
        };
        let text = describe(&stats);
        assert!(text.contains("warehouse         none") && text.contains("factory           none"), "{text}");
        assert!(text.contains("fields        0   nothing sown"), "{text}");
    }
}
