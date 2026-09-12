//! The bottom bar: who is selected, and what can be done to them.
//!
//! ```text
//! +-----------------------------------+
//! | life                              |   <- the open menu, if there is one
//! | [despawn] [freeze]                |
//! +-----------------------------------+
//! | [##] human                        |
//! | [##] [life][debug][stats][brains][close]
//! +-----------------------------------+
//! ```
//!
//! [`selection`] decides *who*; this decides what that looks like and what the
//! buttons do. It appears when somebody is selected and is gone the moment
//! nobody is — there is no empty state, because a panel about nobody is a
//! panel that is only in the way.
//!
//! [`selection`]: super::selection
//!
//! # Nothing here is [`Focusable`] either
//!
//! [`Focusable`]: crate::ui::nav::Focusable
//!
//! For the reason the two corners are not: the arrows pan the camera on this
//! screen and `A` zooms, so `nav`'s one highlight would be walked around by
//! the camera controls and pressed by the zoom. These are mouse buttons, and
//! selecting is a mouse act to begin with. They answer to [`Activated`] all
//! the same, so a QA script presses them by name like anything else.
//!
//! # The panel is rebuilt, the readouts are written
//!
//! Two different rates, and mixing them up is how a HUD ends up rebuilding
//! itself sixty times a second:
//!
//! * **Structure** — which portrait, which menu — is rebuilt only when the
//!   selection or the open menu changes, which is when somebody pressed
//!   something.
//! * **Numbers** — the debug menu's fields, the freeze button's own label —
//!   are rewritten in place every frame, because they follow the simulation
//!   rather than the player. That is what "updated in real time" means for the
//!   debug menu: a position that ticks along while you read it.
//!
//! # What it does to the world, it asks for
//!
//! `despawn` and `freeze` do not touch [`Sim`]. They push a [`Command`] onto
//! [`SimInput`], and the next spawn pass carries it out — the same route a QA
//! script's spawn takes, and the reason this screen still has exactly one
//! writer of the world.

use bevy::prelude::*;

use crate::characters::{dog, human};
use crate::sim::{EntityType, GameEntity, MoveOutcome, Uid};
use crate::state::AppState;
use crate::ui::nav::{Activated, NavSystems};
use crate::ui::{
    hover_highlight, label, labelled_button, plain_button, FONT_BODY, PANEL, TEXT, TEXT_ACCENT,
    TEXT_DANGER, TEXT_DIM,
};

use super::actors::{facing_of, look_from, Sim, SimInput};
use super::hud;
use super::selection::Selected;

/// Size of the portrait, in canvas pixels. Two pixels to a source texel, so
/// the art stays on whole pixels of the interface.
const PORTRAIT: f32 = 32.0;

/// How wide a field name is in the debug menu, so the values line up.
const FIELD_WIDTH: usize = 8;

/// Which menu is open under the bar, if any.
///
/// One at a time: these are four views of one unit, not four panels, and two
/// of them open at once would cover the map they are about.
#[derive(Resource, Default, PartialEq, Eq)]
struct OpenMenu(Option<Menu>);

/// The four things there are to know about a unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    /// What can be done to it: despawn, freeze.
    Life,
    /// Everything the simulation holds about it, as it changes.
    Debug,
    /// Empty for now — there are no statistics to keep yet.
    Stats,
    /// Empty for now — a wanderer's whole mind is one goal, and the debug
    /// menu already shows it.
    Brains,
}

impl Menu {
    /// The button's label and the menu's title, from one place so the two
    /// cannot disagree.
    fn label(self) -> &'static str {
        match self {
            Menu::Life => "life",
            Menu::Debug => "debug",
            Menu::Stats => "stats",
            Menu::Brains => "brains",
        }
    }
}

/// A button on the bar or in a menu, and what pressing it asks for.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Open this menu, or close it if it is the one already open.
    Open(Menu),
    /// Stop looking at this unit.
    Close,
    /// Take it out of the world.
    Despawn,
    /// Hold it still, or let it go.
    ToggleFreeze,
}

/// The root of everything this module puts on screen. One at a time, and
/// despawned whole when the panel is rebuilt.
#[derive(Component)]
struct UnitPanel;

/// A piece of text that follows the simulation rather than the player.
#[derive(Component, Clone, Copy)]
enum Readout {
    /// Every field of the debug menu, rewritten as a block.
    Debug,
    /// The freeze button's own label, which flips to `unfreeze`.
    Freeze,
    /// The line by the portrait: what it is, and whether it is held.
    Title,
}

pub struct UnitPanelPlugin;

impl Plugin for UnitPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenMenu>()
            .add_systems(OnExit(AppState::Game), close_on_leaving)
            .add_systems(
                Update,
                (
                    press_actions,
                    close_menu_for_a_new_unit,
                    rebuild,
                    update_readouts,
                    hover_highlight::<Action>,
                )
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// The menu does not survive the screen, for the same reason the zoom and the
/// speed do not: the next visit is a new world.
fn close_on_leaving(mut menu: ResMut<OpenMenu>) {
    if menu.0.is_some() {
        menu.0 = None;
    }
}

/// A click on a button, or a script pressing one by name.
fn press_actions(
    mut activated: MessageReader<Activated>,
    clicked: Query<(&Action, &Interaction), Changed<Interaction>>,
    actions: Query<&Action>,
    mut selected: ResMut<Selected>,
    sim: Option<Res<Sim>>,
    mut input: ResMut<SimInput>,
    mut menu: ResMut<OpenMenu>,
) {
    let mut pressed: Vec<Action> = clicked
        .iter()
        .filter(|(_, interaction)| **interaction == Interaction::Pressed)
        .map(|(action, _)| *action)
        .collect();
    // Nothing on this screen is focusable, so `nav` writes this message for
    // these buttons and the two corners' and for nothing else.
    pressed.extend(
        activated
            .read()
            .filter_map(|message| actions.get(message.0).ok().copied()),
    );

    let Some(uid) = selected.get() else {
        return;
    };

    for action in pressed {
        match action {
            // Pressing the open menu's own button closes it: the button is
            // the way in, so it is the obvious way back out.
            Action::Open(wanted) => {
                let now = (menu.0 != Some(wanted)).then_some(wanted);
                if menu.0 != now {
                    menu.0 = now;
                }
            }
            // The panel goes because the selection does — `selection`'s frame
            // and this panel are two views of the same fact. Returning rather
            // than carrying on: every action after this one needs the
            // selection that has just been given up.
            Action::Close => {
                selected.clear();
                return;
            }
            Action::Despawn => {
                input.0.despawn(uid);
            }
            Action::ToggleFreeze => {
                input.0.freeze(uid, !is_frozen(sim.as_deref(), uid));
            }
        }
    }
}

/// A menu belongs to the unit it was opened on.
///
/// Selecting somebody else closes it rather than showing them the previous
/// unit's menu, which would be a debug view that silently changed subject.
/// Run before [`rebuild`] so the two land in the same frame and the panel is
/// built once.
fn close_menu_for_a_new_unit(selected: Res<Selected>, mut menu: ResMut<OpenMenu>) {
    if selected.is_changed() && menu.0.is_some() {
        menu.0 = None;
    }
}

/// Build the panel when there is something new to show, and take it away when
/// there is not.
fn rebuild(
    mut commands: Commands,
    assets: Res<AssetServer>,
    sheets: Option<Res<dog::DogSheets>>,
    selected: Res<Selected>,
    menu: Res<OpenMenu>,
    sim: Option<Res<Sim>>,
    panels: Query<Entity, With<UnitPanel>>,
) {
    if !selected.is_changed() && !menu.is_changed() {
        return;
    }
    for panel in &panels {
        commands.entity(panel).despawn();
    }

    let Some((uid, sim)) = selected.get().zip(sim) else {
        return;
    };
    let Some(entity) = sim.0.entities().get(uid) else {
        // Despawned this frame; `selection` clears the selection next.
        return;
    };
    let root = commands
        .spawn((
            Name::new("unit panel"),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(4),
                left: px(0),
                right: px(0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(3),
                ..default()
            },
            UnitPanel,
            DespawnOnExit(AppState::Game),
        ))
        .id();

    // The open menu sits above the bar rather than below it: the bar is the
    // thing anchored to the edge of the screen, and a menu that pushed it off
    // the bottom would move the buttons every time one was pressed.
    if let Some(open) = menu.0 {
        commands.entity(root).with_children(|parent| {
            parent
                .spawn((
                    Name::new("unit menu"),
                    menu_node(),
                    BackgroundColor(PANEL),
                    // So a press on the panel's own background is not a press
                    // on the map behind it.
                    hud::absorbs_clicks(),
                    children![label(open.label(), FONT_BODY, TEXT_DIM)],
                ))
                .with_children(|panel| menu_contents(panel, open, &sim, entity));
        });
    }

    commands.entity(root).with_children(|parent| {
        parent
            .spawn((
                Name::new("unit bar"),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    padding: UiRect::axes(px(4), px(3)),
                    ..default()
                },
                BackgroundColor(PANEL),
                hud::absorbs_clicks(),
            ))
            .with_children(|bar| {
                portrait(bar, &assets, sheets.as_deref(), entity);
                bar.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(3),
                        ..default()
                    },
                    children![
                        (Readout::Title, label(title(entity), FONT_BODY, TEXT_ACCENT)),
                        (
                            Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: px(3),
                                ..default()
                            },
                            children![
                                (Action::Open(Menu::Life), plain_button("life", px(20))),
                                (Action::Open(Menu::Debug), plain_button("debug", px(26))),
                                (Action::Open(Menu::Stats), plain_button("stats", px(24))),
                                (Action::Open(Menu::Brains), plain_button("brains", px(28))),
                                (Action::Close, plain_button("close", px(24))),
                            ],
                        ),
                    ],
                ));
            });
    });
}

/// What goes inside an open menu.
///
/// Spawned into the menu rather than returned as a bundle: the four menus are
/// four different shapes, and `impl Bundle` cannot be four types.
///
/// The two readouts are built holding what is true *now* rather than being
/// left blank for [`update_readouts`] to fill in: the panel is spawned through
/// `Commands`, so its text does not exist until the end of the frame, and a
/// `freeze` button that says `freeze` for one frame about a unit that is
/// already frozen is a button that looks like it did not work.
fn menu_contents(
    parent: &mut ChildSpawnerCommands,
    menu: Menu,
    sim: &Sim,
    entity: &dyn GameEntity,
) {
    match menu {
        Menu::Life => {
            parent.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(3),
                    ..default()
                },
                children![
                    (
                        Action::Despawn,
                        labelled_button(label("despawn", FONT_BODY, TEXT_DANGER), px(34)),
                    ),
                    (
                        Action::ToggleFreeze,
                        labelled_button(
                            (
                                Readout::Freeze,
                                label(freeze_label(entity), FONT_BODY, TEXT),
                            ),
                            px(34),
                        ),
                    ),
                ],
            ));
        }
        Menu::Debug => {
            parent.spawn((
                Readout::Debug,
                label(debug_block(sim, entity), FONT_BODY, TEXT),
            ));
        }
        // Deliberately empty, and saying so: a menu that opened onto nothing
        // at all would read as a menu that failed to load.
        Menu::Stats | Menu::Brains => {
            parent.spawn(label("empty", FONT_BODY, TEXT_DIM));
        }
    }
}

/// The picture of the unit, built from the same art the world draws it with.
///
/// A human is a stack of absolutely-positioned layers, exactly as the world
/// sprite is a stack of children; a dog is one frame of its idle strip. Both
/// come from [`super::actors`]'s own translation of the simulation, so the
/// portrait shows the person on the map and not a second, similar person.
fn portrait(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    sheets: Option<&dog::DogSheets>,
    entity: &dyn GameEntity,
) {
    let mut portrait = parent.spawn((
        Name::new("portrait"),
        Node {
            width: px(PORTRAIT),
            height: px(PORTRAIT),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)),
    ));

    match entity.kind() {
        Some(EntityType::Human) => {
            let look = look_from(entity);
            portrait.with_children(|stack| {
                for path in human::portrait_layers(&look) {
                    stack.spawn((layer_node(), ImageNode::new(assets.load(path))));
                }
            });
        }
        Some(EntityType::Dog) => {
            let Some(sheets) = sheets else {
                return;
            };
            let (image, atlas) = sheets.portrait(facing_of(entity));
            portrait.with_children(|stack| {
                stack.spawn((layer_node(), ImageNode::from_atlas_image(image, atlas)));
            });
        }
        // An id tagged with a type this build has no art for. The rest of the
        // panel still works, which is the point of not guessing a picture.
        None => {}
    }
}

/// One layer of a portrait: the whole square, stacked over the last.
fn layer_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    }
}

fn menu_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(px(4), px(3)),
        row_gap: px(3),
        min_width: px(90),
        ..default()
    }
}

/// Keep the readouts saying what is true, every frame.
///
/// This is the "updated in real time" half of the debug menu. It writes only
/// when the text actually changed: rewriting a `Text` with the same string
/// still costs a re-layout and a re-render of every glyph in it.
fn update_readouts(
    selected: Res<Selected>,
    sim: Option<Res<Sim>>,
    mut texts: Query<(&Readout, &mut Text)>,
) {
    let Some((uid, sim)) = selected.get().zip(sim) else {
        return;
    };
    let Some(entity) = sim.0.entities().get(uid) else {
        return;
    };

    for (readout, mut text) in &mut texts {
        let wanted = match readout {
            Readout::Debug => debug_block(&sim, entity),
            Readout::Freeze => freeze_label(entity).to_string(),
            Readout::Title => title(entity),
        };
        if **text != wanted {
            **text = wanted;
        }
    }
}

/// What the freeze button says, which is what pressing it will do.
fn freeze_label(entity: &dyn GameEntity) -> &'static str {
    if entity.is_frozen() {
        "unfreeze"
    } else {
        "freeze"
    }
}

/// The line beside the portrait: what this is, and whether it is being held.
fn title(entity: &dyn GameEntity) -> String {
    let name = name_of(entity.kind());
    if entity.is_frozen() {
        format!("{name}  (frozen)")
    } else {
        name.to_string()
    }
}

/// Everything the simulation holds about one entity, as a block of lines.
///
/// The universal facts come off the body, in the order somebody debugging
/// wants them — what it is, where it is, whether it is moving — and the kind's
/// own fields ([`GameEntity::debug_fields`]) follow, because those are what
/// differs between a human and a dog.
fn debug_block(sim: &Sim, entity: &dyn GameEntity) -> String {
    let (x, y) = entity.position();
    let cell = entity.center_position();
    let mut fields: Vec<(&'static str, String)> = vec![
        ("id", entity.uid().to_string()),
        ("cell", format!("{}, {}", cell.x, cell.y)),
        ("pos", format!("{x:.2}, {y:.2}")),
        (
            "facing",
            match entity.facing() {
                Some(facing) => format!("{facing:?}").to_lowercase(),
                None => "-".to_string(),
            },
        ),
        ("frozen", yes_no(entity.is_frozen())),
        ("move", describe_move(sim.0.last_move(entity.uid()))),
    ];
    fields.extend(entity.debug_fields());
    fields.push(("tick", sim.0.tick().to_string()));

    fields
        .into_iter()
        .map(|(name, value)| format!("{name:<FIELD_WIDTH$}{value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What became of its last move, in words.
///
/// `Blocked` carries the id of whatever was in the way, and naming it is the
/// entire reason this is worth showing: two units that keep blocking each
/// other are visible here as a pair of ids and nowhere else.
fn describe_move(outcome: Option<MoveOutcome>) -> String {
    match outcome {
        Some(MoveOutcome::Moved) => "moved".to_string(),
        Some(MoveOutcome::Idle) => "idle".to_string(),
        Some(MoveOutcome::Blocked { by: Some(other) }) => format!("blocked by {other}"),
        Some(MoveOutcome::Blocked { by: None }) => "blocked by the map".to_string(),
        None => "-".to_string(),
    }
}

fn yes_no(yes: bool) -> String {
    if yes { "yes" } else { "no" }.to_string()
}

/// Whether an entity is being held still, for a caller that has an id and a
/// world rather than the entity itself.
fn is_frozen(sim: Option<&Sim>, uid: Uid) -> bool {
    sim.and_then(|sim| sim.0.entities().get(uid))
        .is_some_and(|entity| entity.is_frozen())
}

/// The name of a kind, or a stand-in for a type this build does not know.
fn name_of(kind: Option<EntityType>) -> &'static str {
    match kind {
        Some(kind) => kind.name(),
        None => "unknown",
    }
}
