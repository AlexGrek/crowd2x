//! Live economy settings. Commands change this world; the YAML stays on disk.

use bevy::prelude::*;
use crate::sim::economy::{Tariff, Tariffs};
use crate::sim::JobKind;
use crate::state::AppState;
use crate::ui::keyboard::MODAL_Z;
use crate::ui::nav::{Activated, Focus, Focusable, NavSystems, Scope};
use crate::ui::{button, hover_highlight, label, plain_button, FONT_BODY, FONT_TITLE, MODAL, TEXT, TEXT_ACCENT};
use super::actors::{Sim, SimInput};

const ECONOMY_MENU: u8 = 1;

#[derive(Resource)]
pub struct EconomyMenu {
    pub root: Option<Entity>,
    selected: Tariff,
}

impl Default for EconomyMenu {
    fn default() -> Self { Self { root: None, selected: Tariff::Tax } }
}

#[derive(Component)]
pub struct OpenEconomy;
#[derive(Component, Clone, Copy)]
enum Control { Select(Tariff), Lower, Raise, Defaults, Close }
#[derive(Component)]
struct Value(Tariff);
#[derive(Component)]
struct SelectedText;

pub struct EconomyPlugin;
impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EconomyMenu>()
            .add_systems(OnExit(AppState::Game), forget)
            .add_systems(Update, (open, act, update, hover_highlight::<OpenEconomy>).chain()
                .after(NavSystems).run_if(in_state(AppState::Game)));
    }
}

pub fn open_button() -> impl Bundle { (OpenEconomy, plain_button("economy", px(34))) }

/// Read on a new game and on `defaults`. A missing file uses embedded defaults;
/// a malformed or unreadable one also tells the player why it was ignored.
pub fn load_tariffs() -> (Tariffs, Option<String>) {
    let path = std::env::var_os("CROWD2X_ECONOMY").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "economy.yaml".into());
    let result = match std::fs::read_to_string(&path) {
        Ok(text) => Tariffs::from_yaml(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Tariffs::default(), None),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(tariffs) => (tariffs, None),
        Err(error) => (Tariffs::default(), Some(format!("{}: {error}; using built-in economy defaults", path.display()))),
    }
}

fn forget(mut menu: ResMut<EconomyMenu>, mut scope: ResMut<Scope>) {
    if menu.root.take().is_some() { *scope = Scope(0); }
}

fn open(
    mut commands: Commands, mut activated: MessageReader<Activated>,
    clicked: Query<&Interaction, (With<OpenEconomy>, Changed<Interaction>)>,
    buttons: Query<(), With<OpenEconomy>>, mut menu: ResMut<EconomyMenu>,
    mut scope: ResMut<Scope>, mut focus: ResMut<Focus>,
) {
    let pressed = clicked.iter().any(|interaction| *interaction == Interaction::Pressed)
        | activated.read().any(|Activated(entity)| buttons.contains(*entity));
    if !pressed || menu.root.is_some() || scope.0 != 0 { return; }
    let root = commands.spawn((Name::new("economy menu"), Node {
        position_type: PositionType::Absolute, width: percent(100), height: percent(100),
        flex_direction: FlexDirection::Column, align_items: AlignItems::Center,
        justify_content: JustifyContent::Center, row_gap: px(2), ..default()
    }, BackgroundColor(MODAL), GlobalZIndex(MODAL_Z), DespawnOnExit(AppState::Game))).id();
    commands.spawn((label("economy", FONT_TITLE, TEXT), ChildOf(root)));
    let columns = commands.spawn((Node { column_gap: px(4), ..default() }, ChildOf(root))).id();
    let tariffs = Tariff::every();
    let split = JobKind::COUNT;
    let mut selected = None;
    for (column, rows) in [&tariffs[..split], &tariffs[split..]].into_iter().enumerate() {
        let col = commands.spawn((Node { flex_direction: FlexDirection::Column, ..default() }, ChildOf(columns))).id();
        for (row, &tariff) in rows.iter().enumerate() {
            let line = commands.spawn((Node { align_items: AlignItems::Center, column_gap: px(1), height: px(8), ..default() }, ChildOf(col))).id();
            let id = commands.spawn((Control::Select(tariff), button(tariff.label(),
                Focusable::new(row as i32, column as i32).in_scope(ECONOMY_MENU), px(87)), ChildOf(line)))
                .insert(Node { width: px(87), height: px(8), align_items: AlignItems::Center, padding: UiRect::horizontal(px(2)), ..default() }).id();
            commands.spawn((Value(tariff), label("", FONT_BODY, TEXT), TextLayout::no_wrap(), Node { width: px(62), height: px(8), ..default() }, ChildOf(line)));
            if tariff == menu.selected { selected = Some(id); }
        }
    }
    commands.spawn((SelectedText, label("", FONT_BODY, TEXT_ACCENT), ChildOf(root)));
    let footer = commands.spawn((Node { column_gap: px(4), ..default() }, ChildOf(root))).id();
    for (col, (control, text)) in [(Control::Lower, "lower"), (Control::Raise, "raise"), (Control::Defaults, "defaults"), (Control::Close, "close")].into_iter().enumerate() {
        commands.spawn((control, button(text, Focusable::new(split as i32, col as i32).in_scope(ECONOMY_MENU), px(40)), ChildOf(footer)));
    }
    focus.0 = selected;
    *scope = Scope(ECONOMY_MENU);
    menu.root = Some(root);
}

fn act(mut commands: Commands, mut activated: MessageReader<Activated>, controls: Query<&Control>,
    mut menu: ResMut<EconomyMenu>, mut scope: ResMut<Scope>, sim: Option<Res<Sim>>, mut input: ResMut<SimInput>) {
    for Activated(entity) in activated.read() {
        if menu.root.is_none() { continue; }
        let Ok(control) = controls.get(*entity) else { continue; };
        match *control {
            Control::Select(tariff) => menu.selected = tariff,
            Control::Close => close_economy(&mut commands, &mut menu, &mut scope),
            Control::Lower | Control::Raise => {
                let Some(sim) = sim.as_ref() else { continue; };
                let tariff = menu.selected;
                let direction = if matches!(control, Control::Lower) { -1 } else { 1 };
                let value = sim.0.economy().tariffs().get(tariff).saturating_add(direction * tariff.step());
                input.0.set_tariff(tariff, value);
            }
            Control::Defaults => {
                let (tariffs, warning) = load_tariffs();
                if let Some(warning) = warning {
                    warn!("{warning}");
                    if let Some(sim) = sim.as_ref() { sim.0.log.important(warning); }
                }
                // Widen first so either cash interval can be restored before clamping it.
                input.0.set_tariff(Tariff::CashMin, 0);
                input.0.set_tariff(Tariff::CashMax, tariffs.get(Tariff::CashMax));
                for tariff in Tariff::every() { input.0.set_tariff(tariff, tariffs.get(tariff)); }
            }
        }
    }
}

pub(super) fn close_economy(commands: &mut Commands, menu: &mut EconomyMenu, scope: &mut Scope) {
    if let Some(root) = menu.root.take() { commands.entity(root).despawn(); }
    *scope = Scope(0);
}

fn update(menu: Res<EconomyMenu>, sim: Option<Res<Sim>>, mut values: Query<(&Value, &mut Text), Without<SelectedText>>,
    mut selected: Query<&mut Text, With<SelectedText>>) {
    if menu.root.is_none() { return; }
    let Some(sim) = sim else { return; };
    let tariffs = sim.0.economy().tariffs();
    for (value, mut text) in &mut values {
        let wanted = tariffs.value_text(value.0);
        if **text != wanted { **text = wanted; }
    }
    let wanted = format!("{}: {}", menu.selected.label(), tariffs.value_text(menu.selected));
    for mut text in &mut selected {
        if **text != wanted { **text = wanted.clone(); }
    }
}
