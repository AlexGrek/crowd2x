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
//! `graphs` turns it to the last week drawn ([`super::graphs`]): mental
//! health, health, the budget and personal money, an hour a point;
//! `numbers` turns it back. It opens on the numbers.
//!
//! A translucent modal: the world stays visible beneath it, with its
//! own [`Scope`], so the corner buttons behind it stop taking the pointer
//! and the highlight cannot be walked out of it.

use bevy::prelude::*;

use crate::sim::fields::Stage;
use crate::sim::statistics::{FieldStock, OutpostCount};
use crate::sim::{GameState, Statistics};
use crate::state::AppState;
use crate::ui::keyboard::MODAL_Z;
use crate::ui::nav::{Activated, Focus, Focusable, NavSystems, Scope};
use crate::ui::{button, hover_highlight, label, plain_button, FONT_BODY, OVERLAY, TEXT, TEXT_ACCENT};

use super::actors::Sim;
use super::graphs::{self, Chart, ChartTitle};

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

/// The menu's tabs: four of numbers and one of graphs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    /// Who is alive and who is dead.
    #[default]
    Overview,
    /// What the warehouse, the factory, the farm and the reserve hold; the fields and the outposts.
    Stock,
    /// Workers per job against its posts.
    Jobs,
    /// The budget, the books, the cash.
    Economy,
    /// The last week, an hour a point.
    Graphs,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Overview, Tab::Stock, Tab::Jobs, Tab::Economy, Tab::Graphs];

    /// The tab button's label.
    pub fn name(self) -> &'static str {
        match self {
            Tab::Overview => "overview",
            Tab::Stock => "stock",
            Tab::Jobs => "jobs",
            Tab::Economy => "economy",
            Tab::Graphs => "graphs",
        }
    }
}

/// The text of one numbers tab, rewritten as the world changes.
#[derive(Component)]
struct StatisticsText(Tab);

/// A tab's button.
#[derive(Component, Clone, Copy)]
struct TabButton(Tab);

/// A tab's page, shown while it is the open one.
#[derive(Component, Clone, Copy)]
struct TabPage(Tab);

/// Which tab the open menu is on.
#[derive(Resource, Default)]
struct OnTab(Tab);

pub struct StatisticsPlugin;

impl Plugin for StatisticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatisticsMenu>()
            .init_resource::<OnTab>()
            .add_systems(OnExit(AppState::Game), forget_the_menu)
            .add_systems(
                Update,
                (open, close, turn_tab, mark_tab, hover_highlight::<OpenStatistics>, update_text, update_graphs)
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
    mut tab: ResMut<OnTab>,
    mut images: ResMut<Assets<Image>>,
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
                // The tabs at the top, under the corners' two rows of buttons,
                // so they stay where they are however long a page runs.
                justify_content: JustifyContent::FlexStart,
                padding: UiRect::top(px(40)),
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(OVERLAY),
            GlobalZIndex(MODAL_Z),
            DespawnOnExit(AppState::Game),
        ))
        .id();
    let row = commands.spawn((Node { column_gap: px(4), ..default() }, ChildOf(overlay))).id();
    // One page a tab, only the first shown; spawned after the row, so under it.
    for tab in Tab::ALL {
        let page = commands
            .spawn((
                TabPage(tab),
                Node { display: if tab == Tab::Overview { Display::Flex } else { Display::None }, ..default() },
                ChildOf(overlay),
            ))
            .id();
        if tab == Tab::Graphs {
            graphs::spawn(&mut commands, &mut images, page);
        } else {
            commands.spawn((StatisticsText(tab), label(String::new(), FONT_BODY * 2.0 / 3.0, TEXT), ChildOf(page)));
        }
    }
    for (column, tab) in Tab::ALL.into_iter().enumerate() {
        commands.spawn((
            TabButton(tab),
            button(tab.name(), Focusable::new(column as i32, 0).in_scope(STATISTICS_MENU), px(40)),
            ChildOf(row),
        ));
    }
    let close = commands
        .spawn((
            CloseStatistics,
            button("close", Focusable::new(Tab::ALL.len() as i32, 0).in_scope(STATISTICS_MENU), px(32)),
            ChildOf(row),
        ))
        .id();
    focus.0 = Some(close);
    *scope = Scope(STATISTICS_MENU);
    menu.0 = Some(overlay);
    tab.0 = Tab::Overview;
}

/// A tab's button pressed: show its page and hide the rest.
fn turn_tab(
    mut activated: MessageReader<Activated>,
    buttons: Query<&TabButton>,
    mut pages: Query<(&TabPage, &mut Node)>,
    mut tab: ResMut<OnTab>,
) {
    let Some(turn_to) = activated.read().filter_map(|Activated(entity)| buttons.get(*entity).ok()).last() else { return };
    tab.0 = turn_to.0;
    for (which, mut node) in &mut pages {
        node.display = if which.0 == tab.0 { Display::Flex } else { Display::None };
    }
}

/// The open tab's label in the accent colour, the others plain.
fn mark_tab(
    menu: Res<StatisticsMenu>,
    tab: Res<OnTab>,
    buttons: Query<(&TabButton, &Children)>,
    mut labels: Query<&mut TextColor>,
) {
    if menu.0.is_none() {
        return;
    }
    for (button, children) in &buttons {
        let wanted = if button.0 == tab.0 { TEXT_ACCENT } else { TEXT };
        for child in children.iter() {
            if let Ok(mut colour) = labels.get_mut(child) {
                if colour.0 != wanted {
                    colour.0 = wanted;
                }
            }
        }
    }
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

/// Keep the graphs up with the hours, while they are shown.
fn update_graphs(
    menu: Res<StatisticsMenu>,
    tab: Res<OnTab>,
    sim: Option<Res<Sim>>,
    mut images: ResMut<Assets<Image>>,
    mut charts: Query<&mut Chart>,
    mut titles: Query<(&ChartTitle, &mut Text)>,
) {
    if menu.0.is_none() || tab.0 != Tab::Graphs {
        return;
    }
    let Some(sim) = sim else { return };
    graphs::draw(&sim.0, &mut images, &mut charts, &mut titles);
}

/// Keep the menu saying what is true, while it is open on a tab of numbers.
/// Every numbers tab is written, not only the open one: one walk of the
/// arena either way, and a tab turned to is never a frame behind.
fn update_text(
    menu: Res<StatisticsMenu>,
    tab: Res<OnTab>,
    sim: Option<Res<Sim>>,
    mut texts: Query<(&StatisticsText, &mut Text)>,
) {
    if menu.0.is_none() || tab.0 == Tab::Graphs {
        return;
    }
    let Some(sim) = sim else { return };
    let stats = GameState::statistics(&sim.0);
    let mut stock = stock(&stats);
    let reserve = sim.0.superfridge();
    if reserve.exists() {
        let dispatch = sim.0.dispatch();
        stock.push_str(&format!(
            "

reserve    {:.4} / {:.4} ({:.1}%)
dispatch   {} waiting  {} delivering",
            reserve.level(sim.0.bins()), reserve.capacity(), reserve.percent(sim.0.bins()),
            dispatch.waiting(), dispatch.orders().len() - dispatch.waiting()
        ));
    }
    for (column, mut text) in &mut texts {
        let wanted = match column.0 {
            Tab::Overview => overview(&stats),
            Tab::Stock => stock.clone(),
            Tab::Jobs => jobs(&stats),
            Tab::Economy => economy(&stats),
            Tab::Graphs => continue,
        };
        // Compared before writing: the same string written again still costs
        // a re-layout.
        if **text != wanted {
            **text = wanted;
        }
    }
}

/// All the numbers at once, tab after tab.
pub fn describe(stats: &Statistics) -> String {
    [overview(stats), stock(stats), jobs(stats), economy(stats)].join("

")
}

/// The overview tab: who is alive and who is dead.
fn overview(stats: &Statistics) -> String {
    let kinds: Vec<String> = stats.alive.iter().map(|(kind, count)| format!("{} {count}", plural(kind.name()))).collect();
    format!(
        "alive      {:>4}   {}

dead       {:>4}   lying {}  carried {}  cremated {}",
        stats.living(), kinds.join("  "), stats.died, stats.lying, stats.carried, stats.cremated
    )
}

/// The stock tab: each site's stores, the fields and the outposts.
fn stock(stats: &Statistics) -> String {
    let mut out = String::new();
    for stock in &stats.sites {
        let name = stock.site;
        if stock.present {
            let items: Vec<String> = stock.items.iter().map(|(item, count)| format!("{count:>4}   {}", item.name())).collect();
            out.push_str(&format!("{name:<10} {}
", items.join("  ")));
        } else {
            out.push_str(&format!("{name:<10}        none
"));
        }
    }
    out.push_str(&fields(&stats.fields));
    out.push_str(&outposts(&stats.outposts));
    out.truncate(out.trim_end().len());
    out
}

/// The jobs tab: workers per job against its posts, and the jobless.
fn jobs(stats: &Statistics) -> String {
    let mut out = String::new();
    for (index, job) in stats.jobs.iter().enumerate() {
        let heading = if index == 0 { "jobs" } else { "" };
        out.push_str(&format!("{heading:<10} {:>4}   {} of {} posts
", job.workers, plural(job.kind.name()), job.posts));
    }
    out.push_str(&format!("{:<10} {:>4}   without a job", "", stats.jobless));
    out
}

/// The economy tab: the budget, the day's books, the cash held.
fn economy(stats: &Statistics) -> String {
    let economy = &stats.economy;
    let mut out = format!("budget     {}   tax {}%
", economy.budget, economy.tax_percent);
    if let Some(book) = economy.today {
        out.push_str(&format!("today      {}   wages {}
", book.balance().signed(), book.wages));
        out.push_str(&format!("           care {}  taxes {}
", book.care, book.taxes.signed()));
        out.push_str(&format!("           sales {}  estates {}
", book.sales.signed(), book.estates.signed()));
    }
    out.push_str(&format!("cash       {}   {} held
", economy.accounts, economy.cash));
    out.push_str(&format!("           average {}
", economy.average));
    out.push_str(&format!("           {} in debt for {}
", economy.in_debt, economy.debt));
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

/// `outposts      1   built  12 of 26 tiles going up`.
fn outposts(count: &OutpostCount) -> String {
    match count.going_up {
        Some((built, tiles)) => format!("{:<10} {:>4}   built  {built} of {tiles} tiles going up
", "outposts", count.built),
        None if count.built == 0 => format!("{:<10} {:>4}   none decreed
", "outposts", 0),
        None => format!("{:<10} {:>4}   built
", "outposts", count.built),
    }
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
            outposts: OutpostCount { built: 1, going_up: Some((12, 26)) },
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
        assert!(text.contains("outposts      1   built  12 of 26 tiles going up"), "{text}");
    }

    #[test]
    fn a_map_without_a_warehouse_or_a_factory_says_so() {
        let stats = Statistics {
            alive: vec![(EntityType::Human, 1)],
            sites: vec![site("warehouse", false, 0, 0), site("factory", false, 0, 0)],
            fields: FieldStock::default(),
            outposts: OutpostCount::default(),
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
        assert!(text.contains("outposts      0   none decreed"), "{text}");
    }
}
