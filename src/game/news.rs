//! The news menu: every article the propagandist has ever published, newest
//! first, a page at a time, over the whole screen.
//!
//! `news`, beside `find` in the world's corner, opens it; `close` or `esc`
//! shuts it, `newer` and `older` turn the pages. It reads
//! `GameState::news` — the archive of articles (`sim::news`), which keeps
//! every one for good — and **keeps reading it while it is open**: the world
//! runs on behind it, a new article goes up at the top of the first page, and
//! the lines are rewritten only when they change. Bad news is red, good news
//! green.
//!
//! An opaque modal, like the economy menu — lines of small coloured text do
//! not read over the world and the log — with its own [`Scope`], so the corner buttons behind it stop taking the pointer
//! and the highlight cannot be walked out of it.

use bevy::prelude::*;

use crate::sim::news::{time_of, News};
use crate::state::AppState;
use crate::ui::keyboard::MODAL_Z;
use crate::ui::nav::{Activated, Focus, Focusable, NavSystems, Scope};
use crate::ui::{button, hover_highlight, label, plain_button, FONT_BODY, FONT_TITLE, MODAL, TEXT, TEXT_DANGER, TEXT_DIM};

use super::actors::Sim;

/// The focus layer the menu occupies while it is open.
const NEWS_MENU: u8 = 1;

/// Articles on a page.
pub const PAGE: usize = 8;

/// Good news is green.
const GOOD: Color = Color::srgb(0.55, 0.88, 0.52);

/// The open news menu, if there is one, and the page it shows (0 the newest).
/// A resource, like [`super::statistics::StatisticsMenu`], so
/// [`super::leave`] can ask whether `esc` closes it or leaves the game.
#[derive(Resource, Default)]
pub struct NewsMenu {
    pub root: Option<Entity>,
    pub page: usize,
}

/// The button in the world's corner that opens the menu.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct OpenNews;

/// The menu's buttons.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum NewsButton {
    Newer,
    Older,
    Close,
}

/// The line under the title: how many, how many on the phones, the page.
#[derive(Component)]
struct NewsStatus;

/// One article's line on the page, by its place on it.
#[derive(Component)]
struct NewsLine(usize);

pub struct NewsPlugin;

impl Plugin for NewsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NewsMenu>()
            .add_systems(OnExit(AppState::Game), forget_the_menu)
            .add_systems(
                Update,
                (open, press, hover_highlight::<OpenNews>, update_text)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// The button that opens the menu, for the corner to put where it likes.
pub fn open_button() -> impl Bundle {
    (OpenNews, plain_button("news", px(24)))
}

/// The overlay goes with every other `DespawnOnExit`; what points at it, and
/// the scope it raised, go here.
fn forget_the_menu(mut menu: ResMut<NewsMenu>, mut scope: ResMut<Scope>) {
    if menu.root.take().is_some() {
        *scope = Scope(0);
    }
    menu.page = 0;
}

/// A click on `news`, or a script pressing it by name.
fn open(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    clicked: Query<&Interaction, (With<OpenNews>, Changed<Interaction>)>,
    buttons: Query<(), With<OpenNews>>,
    mut menu: ResMut<NewsMenu>,
    mut scope: ResMut<Scope>,
    mut focus: ResMut<Focus>,
) {
    let pressed = clicked.iter().any(|interaction| *interaction == Interaction::Pressed)
        | activated.read().any(|Activated(entity)| buttons.contains(*entity));
    if !pressed || menu.root.is_some() || scope.0 != 0 {
        return;
    }

    let overlay = commands
        .spawn((
            Name::new("news menu"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(2),
                ..default()
            },
            BackgroundColor(MODAL),
            GlobalZIndex(MODAL_Z),
            DespawnOnExit(AppState::Game),
            children![
                label("the news", FONT_TITLE, TEXT),
                (NewsStatus, label(String::new(), FONT_BODY * 2.0 / 3.0, TEXT_DIM)),
            ],
        ))
        .id();
    for place in 0..PAGE {
        commands.spawn((
            NewsLine(place),
            Node { width: percent(92), ..default() },
            label(String::new(), FONT_BODY * 2.0 / 3.0, TEXT),
            ChildOf(overlay),
        ));
    }
    let row = commands
        .spawn((Node { column_gap: px(6), margin: UiRect::top(px(3)), ..default() }, ChildOf(overlay)))
        .id();
    let mut close = None;
    for (column, (which, text)) in
        [(NewsButton::Newer, "newer"), (NewsButton::Older, "older"), (NewsButton::Close, "close")].into_iter().enumerate()
    {
        let id = commands
            .spawn((which, button(text, Focusable::new(column as i32, 0).in_scope(NEWS_MENU), px(34)), ChildOf(row)))
            .id();
        if which == NewsButton::Close {
            close = Some(id);
        }
    }
    focus.0 = close;
    *scope = Scope(NEWS_MENU);
    menu.root = Some(overlay);
    menu.page = 0;
}

/// `newer`, `older` or `close` pressed.
fn press(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    buttons: Query<&NewsButton>,
    sim: Option<Res<Sim>>,
    mut menu: ResMut<NewsMenu>,
    mut scope: ResMut<Scope>,
) {
    for Activated(entity) in activated.read() {
        let Ok(which) = buttons.get(*entity) else { continue };
        match which {
            NewsButton::Newer => menu.page = menu.page.saturating_sub(1),
            NewsButton::Older => {
                let last = sim.as_ref().map_or(0, |sim| pages(sim.0.news()) - 1);
                menu.page = (menu.page + 1).min(last);
            }
            NewsButton::Close => close_news(&mut commands, &mut menu, &mut scope),
        }
    }
}

/// Close the menu, from its own button or from `esc` ([`super::leave`]).
pub(super) fn close_news(commands: &mut Commands, menu: &mut NewsMenu, scope: &mut Scope) {
    if let Some(overlay) = menu.root.take() {
        commands.entity(overlay).despawn();
    }
    menu.page = 0;
    *scope = Scope(0);
}

/// Keep the menu saying what is true, while it is open.
fn update_text(
    menu: Res<NewsMenu>,
    sim: Option<Res<Sim>>,
    mut status: Query<&mut Text, (With<NewsStatus>, Without<NewsLine>)>,
    mut lines: Query<(&NewsLine, &mut Text, &mut TextColor), Without<NewsStatus>>,
) {
    if menu.root.is_none() {
        return;
    }
    let Some(sim) = sim else { return };
    let page = describe(sim.0.news(), sim.0.clock().elapsed(), menu.page);
    // Compared before writing: the same string written again still costs a
    // re-layout.
    for mut text in &mut status {
        if **text != page.status {
            **text = page.status.clone();
        }
    }
    for (line, mut text, mut color) in &mut lines {
        let (wanted, good) = page.lines.get(line.0).cloned().unwrap_or_default();
        if **text != wanted {
            **text = wanted;
        }
        let tone = if good { GOOD } else { TEXT_DANGER };
        if color.0 != tone {
            color.0 = tone;
        }
    }
}

/// How many pages the archive takes: one at least, empty or not.
pub fn pages(news: &News) -> usize {
    news.articles().len().div_ceil(PAGE).max(1)
}

/// One page of the menu: the line under the title, and each article's line
/// with whether it is good news.
#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    pub status: String,
    pub lines: Vec<(String, bool)>,
}

/// Page `page` of the archive at world second `now`, newest first:
/// `day 3 Wed  13:30  bad  Ann Lee, a guard, died of thirst at 40, 22 on ...`.
/// A page past the end shows the last.
pub fn describe(news: &News, now: f64, page: usize) -> Page {
    let articles = news.articles();
    let page = page.min(pages(news) - 1);
    let lines = articles
        .iter()
        .rev()
        .skip(page * PAGE)
        .take(PAGE)
        .map(|article| {
            let good = article.story.is_good();
            let tone = if good { "good" } else { "bad " };
            (format!("{}  {tone}  {}", time_of(article.published), article.story.headline()), good)
        })
        .collect();
    let status = if articles.is_empty() {
        format!("nothing published yet   {} waiting for the propagandist", news.waiting())
    } else {
        format!(
            "{} published   {} on the phones   {} waiting for the propagandist   page {} of {}",
            articles.len(),
            news.current(now).len(),
            news.waiting(),
            page + 1,
            pages(news)
        )
    };
    Page { status, lines }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::item::ItemKind;
    use crate::sim::log::Log;
    use crate::sim::news::{Story, SHELF_LIFE};
    use crate::sim::testing::a_death;

    /// Twenty articles: ten deaths and ten records, alternating, an hour apart.
    fn archive() -> News {
        let log = Log::default();
        let mut news = News::default();
        for n in 0..20u32 {
            let story = if n % 2 == 0 { a_death(n as u64 + 1) } else { Story::Record { item: ItemKind::Scrap, total: 100 * (n + 1) } };
            news.report(story);
            let _ = news.publish(n as f64 * 3600.0, &log);
        }
        news.report(a_death(99));
        news
    }

    #[test]
    fn the_menu_reads_the_archive_newest_first_a_page_at_a_time() {
        let news = archive();
        assert_eq!(pages(&news), 3);
        let now = 19.0 * 3600.0;
        let first = describe(&news, now, 0);
        assert_eq!(first.status, "20 published   20 on the phones   1 waiting for the propagandist   page 1 of 3");
        assert_eq!(first.lines.len(), PAGE);
        assert_eq!(first.lines[0], ("day 2 Tue  03:00  good  a record: 2000 scrap metal made".to_string(), true));
        assert_eq!(
            first.lines[1],
            ("day 2 Tue  02:00  bad   Ann Lee, a guard, died of thirst at 4, 2 on day 1 Mon  12:00".to_string(), false)
        );
        let last = describe(&news, now, 2);
        assert_eq!(last.lines.len(), 4, "the oldest four");
        assert!(last.lines[3].0.starts_with("day 1 Mon  08:00  bad "), "{:?}", last.lines[3]);
        assert_eq!(describe(&news, now, 9), last, "past the end is the last page");
        // Off the phones, still in the archive.
        assert!(describe(&news, now + 2.0 * SHELF_LIFE, 0).status.starts_with("20 published   0 on the phones"));
    }

    #[test]
    fn with_nothing_published_the_menu_says_so() {
        let mut news = News::default();
        assert_eq!(describe(&news, 0.0, 0), Page { status: "nothing published yet   0 waiting for the propagandist".to_string(), lines: vec![] });
        news.report(a_death(1));
        assert_eq!(describe(&news, 0.0, 0).status, "nothing published yet   1 waiting for the propagandist");
        assert_eq!(pages(&news), 1);
    }
}
