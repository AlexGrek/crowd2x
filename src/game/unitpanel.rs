//! The bottom bar: who is selected, and what can be done to them.
//!
//! ```text
//! +-----------------------------------+
//! | life                              |   <- the open menu, if there is one
//! | [despawn] [freeze]                |
//! +-----------------------------------+
//! | [##] human                        |
//! | [##] [life][debug][stats][brains][items][talents][clothing][job][relationship][close]
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
//!   selection, the open menu, or the worn outfit changes.
//! * **Numbers** — the debug menu's fields, the freeze and hire buttons' own labels —
//!   are rewritten in place every frame, because they follow the simulation
//!   rather than the player. That is what "updated in real time" means for the
//!   debug menu: a position that ticks along while you read it.
//!
//! # What it does to the world, it asks for
//!
//! `despawn`, `freeze`, `hire` and `dismiss` do not touch [`Sim`]. They push a [`Command`] onto
//! [`SimInput`], and the next spawn pass carries it out — the same route a QA
//! script's spawn takes, and the reason this screen still has exactly one
//! writer of the world.

use bevy::prelude::*;

use crate::characters::{dog, human};
use crate::sim::brain::routines::READY_NEEDS;
use crate::sim::clothing::Clothing;
use crate::sim::{Clock, EntityType, GameEntity, ItemKind, MoveOutcome, Talent, Uid};
use crate::state::AppState;
use crate::ui::nav::{Activated, NavSystems};
use crate::ui::{
    hover_highlight, label, labelled_button, plain_button, FONT_BODY, PANEL, TEXT, TEXT_ACCENT,
    TEXT_DANGER, TEXT_DIM,
};

use super::actors::{facing_of, look_from, world_pos, Sim, SimInput};
use super::hud;
use super::follow::{self, CameraFollow};
use super::needs;
use super::selection::Selected;

/// Size of the portrait, in canvas pixels. Two pixels to a source texel, so
/// the art stays on whole pixels of the interface.
const PORTRAIT: f32 = 32.0;

/// The widest the row of menu buttons beside the portrait may be, in canvas
/// pixels, before it wraps: the canvas is 320, less the portrait, the gap and
/// the bar's padding, with a little to spare.
const BUTTON_ROW: f32 = 270.0;

/// The same, for a unit that has the needs panel down the right edge
/// ([`needs`]): the canvas less that panel's [`needs::GUTTER`], less the
/// portrait, the gap and the padding. Still two rows of buttons.
const BUTTON_ROW_BESIDE_NEEDS: f32 = 190.0;

/// Which menu is open under the bar, if any.
///
/// One at a time: these are views of one unit, and two
/// of them open at once would cover the map they are about.
#[derive(Resource, Default, PartialEq, Eq)]
struct OpenMenu(Option<Menu>);

/// The views of a selected unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    /// What can be done to it: despawn, freeze.
    Life,
    /// Everything the simulation holds about it, as it changes.
    Debug,
    /// Empty for now. Every stat is already listed, live, in the debug menu;
    /// this is for the history of them, which nothing keeps yet.
    Stats,
    /// What its mind is doing: the goal in charge, the priority list, the task
    /// and the action carrying it out — live, like the debug menu.
    Brains,
    /// What it is carrying: its hand, what it has stowed, and how close that
    /// is to the two limits — live, like the debug menu, because a unit picks
    /// things up and eats them while you are reading.
    Items,
    /// Lasting aptitudes, shown live so explicit changes appear immediately.
    Talents,
    Clothing,
    /// What it does for a living: the job, where it is done, the hours, and
    /// what it is doing about them right now — live — with a button to let it
    /// go or take it on.
    Job,
    /// Who it knows and what it thinks of them: family, colleagues, friends
    /// and whoever else it has chatted with, best first, and the chat it is
    /// in — live, since a chat can end while you read.
    Relationship,
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
            Menu::Items => "items",
            Menu::Talents => "talents",
            Menu::Clothing => "clothing",
            Menu::Job => "job",
            Menu::Relationship => "relationship",
        }
    }
}

/// A button on the bar or in a menu, and what pressing it asks for.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Action {
    ToggleFollow,
    /// Open this menu, or close it if it is the one already open.
    Open(Menu),
    /// Stop looking at this unit.
    Close,
    /// Take it out of the world.
    Despawn,
    /// Hold it still, or let it go.
    ToggleFreeze,
    Wear(Clothing),
    /// Let it go from its job, or give it one.
    ToggleJob,
}

/// The root of everything this module puts on screen. One at a time, and
/// despawned whole when the panel is rebuilt.
#[derive(Component)]
struct UnitPanel;

/// The portrait's worn layer, also inspected by the QA harness.
#[derive(Component)]
pub(crate) struct ClothingPortrait;

/// A piece of text that follows the simulation rather than the player.
#[derive(Component, Clone, Copy)]
enum Readout {
    /// Every field of the debug menu, rewritten as a block.
    Debug,
    /// Every field of the brains menu, rewritten as a block.
    Brains,
    /// Every field of the items menu, rewritten as a block.
    Items,
    /// Every talent, rewritten when its value changes.
    Talents,
    /// The freeze button's own label, which flips to `unfreeze`.
    Freeze,
    /// Every field of the job menu, rewritten as a block.
    Job,
    /// The job menu's button, which says `dismiss` to somebody with a job
    /// and `hire` to somebody without.
    Employ,
    /// Every line of the relationship menu, rewritten as a block.
    Relationship,
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
                    follow::highlight_button,
                    draw_path,
                )
                    .chain()
                    .after(NavSystems)
                    .before(super::move_camera)
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
    mut follow: ResMut<CameraFollow>,
    time: Res<Time<Real>>,
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
            Action::ToggleFollow => {
                if let Some(entity) = sim.as_ref().and_then(|sim| sim.0.entities().get(uid)) {
                    follow.toggle(uid, entity.center_position(), time.elapsed_secs_f64());
                }
            }
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
            Action::Wear(clothing) => {
                input.0.set_clothing(uid, clothing);
            }
            Action::ToggleJob => {
                if is_employed(sim.as_deref(), uid) {
                    input.0.dismiss(uid);
                } else {
                    input.0.hire(uid);
                }
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
    mut shown: Local<Option<(Uid, Option<Clothing>, Option<Clothing>)>>,
) {
    // Its own clothes and what it has on: a uniform going on with a shift
    // redraws the portrait as a change of clothes does.
    let appearance = selected.get().and_then(|uid| {
        let sim = sim.as_ref()?;
        sim.0.entities().get(uid).map(|entity| (uid, entity.clothing(), entity.worn(sim.0.clock())))
    });
    if !selected.is_changed() && !menu.is_changed() && *shown == appearance {
        return;
    }
    *shown = appearance;
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
    // A human has its needs down the right edge, and the bar keeps to the left
    // of them: its margin is the panel's width, so the bar centres in what is
    // left rather than under the bars.
    let (button_row, gutter) = if needs::shows(entity) {
        (BUTTON_ROW_BESIDE_NEEDS, needs::GUTTER)
    } else {
        (BUTTON_ROW, 0.0)
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
                    BackgroundColor(if open == Menu::Clothing { crate::ui::MODAL } else { PANEL }),
                    GlobalZIndex(1),
                    bevy::ui::FocusPolicy::Block,
                    // So a press on the panel's own background is not a press
                    // on the map behind it.
                    hud::absorbs_clicks(),
                    children![label(open.label(), FONT_BODY, TEXT_DIM)],
                ))
                .with_children(|panel| menu_contents(panel, open, &sim, entity, &assets));
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
                    margin: UiRect::right(px(gutter)),
                    ..default()
                },
                BackgroundColor(PANEL),
                hud::absorbs_clicks(),
            ))
            .with_children(|bar| {
                portrait(bar, &assets, sheets.as_deref(), entity, sim.0.clock());
                bar.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(3),
                        ..default()
                    },
                    children![
                        (
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(3),
                                ..default()
                            },
                            children![
                                (Readout::Title, label(title(entity), FONT_BODY, TEXT_ACCENT)),
                                (Action::ToggleFollow, follow::button()),
                            ],
                        ),
                        (
                            // Wrapped onto a second row rather than run off
                            // the right of the canvas: the portrait and the
                            // padding leave this much of its 320 pixels.
                            Node {
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(3),
                                row_gap: px(3),
                                max_width: px(button_row),
                                ..default()
                            },
                            children![
                                (Action::Open(Menu::Life), plain_button("life", px(20))),
                                (Action::Open(Menu::Debug), plain_button("debug", px(26))),
                                (Action::Open(Menu::Stats), plain_button("stats", px(24))),
                                (Action::Open(Menu::Brains), plain_button("brains", px(28))),
                                (Action::Open(Menu::Items), plain_button("items", px(24))),
                                (Action::Open(Menu::Talents), plain_button("talents", px(32))),
                                (Action::Open(Menu::Clothing), plain_button("clothing", px(32))),
                                (Action::Open(Menu::Job), plain_button("job", px(18))),
                                (Action::Open(Menu::Relationship), plain_button("relationship", px(50))),
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
/// Spawned into the menu rather than returned as a bundle: the menus have
/// different shapes.
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
    assets: &AssetServer,
) {
    match menu {
        Menu::Clothing => clothing_menu(parent, entity, assets, sim.0.clock()),
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
        Menu::Brains => {
            parent.spawn((
                Readout::Brains,
                label(brains_block(entity), FONT_BODY, TEXT),
            ));
        }
        Menu::Items => {
            parent.spawn((
                Readout::Items,
                label(items_block(entity), FONT_BODY, TEXT),
            ));
        }
        Menu::Talents => {
            parent.spawn((
                Readout::Talents,
                label(talents_block(entity), FONT_BODY, TEXT),
            ));
        }
        Menu::Job => {
            parent.spawn((
                Readout::Job,
                label(job_and_cash(&sim, entity), FONT_BODY, TEXT),
            ));
            // Only for a kind that can work: a dog has no job to lose and
            // nobody would give it one.
            if entity.kind() == Some(EntityType::Human) {
                parent.spawn((
                    Action::ToggleJob,
                    labelled_button((Readout::Employ, label(employ_label(entity), FONT_BODY, TEXT)), px(34)),
                ));
            }
        }
        Menu::Relationship => {
            parent.spawn((
                Readout::Relationship,
                label(relationship_block(sim, entity), FONT_BODY, TEXT),
            ));
        }
        // Deliberately empty, and saying so: a menu that opened onto nothing
        // at all would read as a menu that failed to load.
        Menu::Stats => {
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
    clock: Clock,
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
            let look = look_from(entity, clock);
            portrait.with_children(|stack| {
                let paths = [Some(human::BASE), Some(look.eyes), look.clothes, Some(look.hair), look.hat];
                for (slot, path) in paths.into_iter().enumerate() {
                    let mut layer = stack.spawn((
                        layer_node(),
                        match path {
                            Some(path) => ImageNode::new(assets.load(path)),
                            None => ImageNode::default(),
                        },
                        if path.is_some() { Visibility::Inherited } else { Visibility::Hidden },
                    ));
                    if slot == 2 {
                        layer.insert(ClothingPortrait);
                    }
                }
            });
        }
        Some(kind @ (EntityType::Dog | EntityType::Deathdog)) => {
            let Some(sheets) = sheets else {
                return;
            };
            let (image, atlas) = sheets.portrait(super::actors::breed_of(kind), facing_of(entity));
            portrait.with_children(|stack| {
                stack.spawn((layer_node(), ImageNode::from_atlas_image(image, atlas)));
            });
        }
        // An id tagged with a type this build has no art for. The rest of the
        // panel still works, which is the point of not guessing a picture.
        None => {}
    }
}

fn clothing_menu(parent: &mut ChildSpawnerCommands, entity: &dyn GameEntity, assets: &AssetServer, clock: Clock) {
    let (Some(worn), Some(on)) = (entity.clothing(), entity.worn(clock)) else {
        parent.spawn(label("no clothing", FONT_BODY, TEXT_DIM));
        return;
    };
    // At work in its uniform, the buttons still change the clothes it goes
    // home in.
    if on.is_uniform() {
        parent.spawn(label(format!("wearing: {} (own: {})", on.name(), worn.name()), FONT_BODY, TEXT_ACCENT));
    } else {
        parent.spawn(label(format!("wearing: {}", worn.name()), FONT_BODY, TEXT_ACCENT));
    }
    let look = look_from(entity, clock);
    for row in Clothing::ALL.chunks(5) {
        parent.spawn(Node { column_gap: px(2), ..default() }).with_children(|buttons| {
            for &outfit in row {
                buttons.spawn((
                    Action::Wear(outfit),
                    Button,
                    Node {
                        width: px(54),
                        height: px(19),
                        align_items: AlignItems::Center,
                        column_gap: px(2), padding: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(crate::ui::FIELD),
                )).with_children(|button| {
                    button.spawn(Node {
                        width: px(16), height: px(16), flex_shrink: 0.0, ..default()
                    })
                        .with_children(|preview| {
                            let preview_look = human::Look {
                                clothes: human::clothing_path(outfit),
                                hat: human::hat_path(outfit),
                                ..look.clone()
                            };
                            for path in human::portrait_layers(&preview_look) {
                                preview.spawn((layer_node(), ImageNode::new(assets.load(path))));
                            }
                        });
                    button.spawn(label(
                        outfit.name(), 5.0, if outfit == worn { TEXT_ACCENT } else { TEXT },
                    ));
                });
            }
        });
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
            Readout::Brains => brains_block(entity),
            Readout::Items => items_block(entity),
            Readout::Talents => talents_block(entity),
            Readout::Freeze => freeze_label(entity).to_string(),
            Readout::Job => job_and_cash(&sim, entity),
            Readout::Employ => employ_label(entity).to_string(),
            Readout::Relationship => relationship_block(&sim, entity),
            Readout::Title => title(entity),
        };
        if **text != wanted {
            **text = wanted;
        }
    }
}

/// Radius of the marker drawn on the selected unit's goal, in canvas pixels.
const GOAL_MARKER: f32 = 3.0;

/// Draw the selected unit's planned route over the map.
///
/// A visual reading of what [`debug_block`]'s `goal` and `path` fields already
/// say in words, for whoever is selected rather than only while their debug
/// menu happens to be open — a route worth glancing at is not a reason to
/// have the text panel up too. World-space, through [`world_pos`], the same
/// conversion the actor it is drawn over is placed with.
fn draw_path(selected: Res<Selected>, sim: Option<Res<Sim>>, mut gizmos: Gizmos) {
    let Some((uid, sim)) = selected.get().zip(sim) else {
        return;
    };
    let Some(entity) = sim.0.entities().get(uid) else {
        return;
    };
    let cells = entity.planned_path();
    if cells.is_empty() {
        return;
    }

    let mut points = Vec::with_capacity(cells.len() + 1);
    points.push(world_pos(entity.position()));
    points.extend(
        cells
            .iter()
            .map(|cell| world_pos((cell.x as f32 + 0.5, cell.y as f32 + 0.5))),
    );

    gizmos.linestrip_2d(points.iter().copied(), TEXT_ACCENT);
    if let Some(&goal) = points.last() {
        gizmos.circle_2d(goal, GOAL_MARKER, TEXT_ACCENT);
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

/// What the job menu's button says, which is what pressing it will do.
fn employ_label(entity: &dyn GameEntity) -> &'static str {
    if entity.job().is_some() {
        "dismiss"
    } else {
        "hire"
    }
}

/// What it does for a living, as a block of lines: the job, where it is done
/// (`computer at 12, 4`, `scrap bin at 30, 2`), the hours and days, and —
/// `now` — what it is doing about them at `clock`, which is the line worth
/// watching: what the job's own row says while its goal is in charge (on the
/// way in, at the desk, out collecting scrap or storing it, on watch at a
/// gate, seeing to the dead — [`JobKind::now`](crate::sim::JobKind::now)),
/// getting ready for a trip out (`getting ready: eat`), away from it for
/// something more pressing, or off.
fn job_and_cash(sim: &Sim, entity: &dyn GameEntity) -> String {
    let mut text = job_block(entity, sim.0.clock());
    let economy = sim.0.economy();
    if let Some(account) = economy.account(entity.uid()) {
        text.push_str(&format!("\ncash    {}", account.cash));
        if let Some(job) = entity.job() {
            text.push_str(&format!("\nwage    {} a shift", economy.tariffs().wage(job.kind)));
        }
        text.push_str(&format!("\nowed    {} shifts, {} care days", account.shifts, account.care_days));
    }
    text
}

fn job_block(entity: &dyn GameEntity, clock: Clock) -> String {
    let Some(job) = entity.job() else {
        return if entity.kind() == Some(EntityType::Human) { "unemployed" } else { "no job" }.to_string();
    };
    let shift = job.shift;
    let at_work = entity.interacting_with() == Some(job.workplace);
    let now = if entity.is_on_sick_leave(clock) {
        "sick leave".to_string()
    } else if job.is_due(clock) {
        match entity.current_goal() {
            Some(goal) if goal == job.kind.goal() => job.kind.now(at_work, entity.inventory()).to_string(),
            Some(goal) if entity.is_getting_ready() && READY_NEEDS.iter().any(|ready| ready.need.goal == goal) => {
                format!("getting ready: {}", goal.name())
            }
            Some(goal) => format!("on a break: {}", goal.name()),
            None => "off".to_string(),
        }
    } else if !shift.works_on(clock.weekday()) {
        if clock.weekday().is_weekend() { "weekend" } else { "day off" }.to_string()
    } else if clock.hours() < shift.start() as f32 {
        format!("off until {:02}:00", shift.start())
    } else if shift.start() == 0 && shift.works_on(clock.weekday().next()) {
        // The night watch: its next start is tonight's midnight.
        "off until 00:00".to_string()
    } else {
        "done for the day".to_string()
    };
    field_block(vec![
        ("job", job.kind.name().to_string()),
        ("at", format!("{} at {}, {}", job.kind.workplace_name(), job.workplace.x, job.workplace.y)),
        ("hours", shift.hours()),
        ("days", shift.days()),
        ("now", now),
    ])
}

/// How many people the relationship menu lists before it says how many more.
const RELATIONSHIP_ROWS: usize = 14;

/// Who it knows, as a block of lines: the chat it is in, if any; how many of
/// each it has; then one line each — name, level, what they are to it — best
/// first ([`GameState::acquaintances`](crate::sim::GameState::acquaintances)),
/// as many as fit.
fn relationship_block(sim: &Sim, entity: &dyn GameEntity) -> String {
    use crate::sim::relationships::Phase;
    if entity.relations().is_none() {
        return "knows nobody".to_string();
    }
    let state = &sim.0;
    // A name, or what became of somebody who is no longer about.
    let name = |uid: Uid| match state.entities().get(uid) {
        Some(other) if other.is_dead() => format!("{} (dead)", other.display_name().unwrap_or_else(|| uid.to_string())),
        Some(other) => other.display_name().unwrap_or_else(|| uid.to_string()),
        None => format!("{uid} (gone)"),
    };
    let mut lines = Vec::new();
    if let Some(chat) = entity.chat() {
        let doing = match chat.phase {
            Phase::Inviting => "asking for a chat:",
            Phase::Talking => "chatting with",
            Phase::Done => "saying goodbye to",
        };
        lines.push(format!("{doing} {}", name(chat.with)));
    }
    if let Some(smalltalk) = entity.smalltalk().filter(|smalltalk| smalltalk.answered) {
        lines.push(format!("making small talk with {}", name(smalltalk.with)));
    }
    let known = state.acquaintances(entity.uid());
    if known.is_empty() {
        lines.push("knows nobody yet".to_string());
        return lines.join("\n");
    }
    let count = |kind: &str| known.iter().filter(|known| known.kind() == kind).count();
    lines.push(format!(
        "family {}  colleagues {}  friends {}  acquaintances {}",
        count("family"),
        count("colleague"),
        count("friend"),
        count("acquaintance")
    ));
    let width = known.iter().take(RELATIONSHIP_ROWS).map(|known| name(known.who).chars().count()).max().unwrap_or(0) + 1;
    for known in known.iter().take(RELATIONSHIP_ROWS) {
        lines.push(format!("{:<width$}{:>3.0}  {}", name(known.who), known.level, known.kind()));
    }
    if known.len() > RELATIONSHIP_ROWS {
        lines.push(format!("...and {} more", known.len() - RELATIONSHIP_ROWS));
    }
    lines.join("\n")
}

/// The line beside the portrait: this person's name, or the kind's for one
/// that has none.
fn title(entity: &dyn GameEntity) -> String {
    let name = entity
        .display_name()
        .unwrap_or_else(|| name_of(entity.kind()).to_string());
    if entity.is_frozen() {
        format!("{name}  (frozen)")
    } else {
        name
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
    field_block(fields)
}

/// What the unit is carrying, as a block of lines.
///
/// The hand first, because it is the slot the simulation actually uses, then
/// one line per kind stowed, then the two totals against the two limits. The
/// hand is in the mass line and not in the space line — `Inventory` says why —
/// and showing both totals side by side is what makes that legible: put
/// something in a hand and `kg` moves while `L` does not.
///
/// A kind that carries nothing at all says so rather than showing a set of
/// zeroes it can never move off.
fn items_block(entity: &dyn GameEntity) -> String {
    let Some(inventory) = entity.inventory() else {
        return "carries nothing".to_string();
    };
    let mut fields: Vec<(&'static str, String)> = vec![(
        "hand",
        inventory.hand().map_or("empty", ItemKind::name).to_string(),
    )];
    if inventory.stowed_items() == 0 {
        fields.push(("stowed", "nothing".to_string()));
    } else {
        fields.extend(
            inventory
                .stowed()
                .map(|(kind, count)| (kind.name(), count.to_string())),
        );
    }
    let capacity = inventory.capacity();
    fields.push(("mass", format!("{:.1} / {:.1} kg", inventory.mass(), capacity.mass)));
    fields.push((
        "space",
        format!("{:.1} / {:.1} L", inventory.volume(), capacity.volume),
    ));
    field_block(fields)
}

/// All nine aptitudes, with their shared range visible next to each value.
fn talents_block(entity: &dyn GameEntity) -> String {
    let Some(talents) = entity.talents() else {
        return "no talents".to_string();
    };
    field_block(
        Talent::ALL.into_iter()
            .map(|talent| (talent.name(), format!("{} / 100", talents.get(talent))))
            .collect(),
    )
}

/// What the unit's mind is doing, as a block of lines — or a plain `empty` for
/// a kind with no brain, which is more honest than a blank menu.
fn brains_block(entity: &dyn GameEntity) -> String {
    let fields = entity.brain_fields();
    if fields.is_empty() {
        return "empty".to_string();
    }
    field_block(fields)
}

/// Name and value pairs as aligned lines. One layout for both live menus, so
/// the two cannot drift apart.
///
/// The names are padded to the longest one in the block, plus a space — not
/// to a fixed width, which a name as long as it (`priority`) runs straight
/// into its value, and any longer one (`mental_health`) pushes out of line.
fn field_block(fields: Vec<(&'static str, String)>) -> String {
    let width = fields.iter().map(|(name, _)| name.len()).max().unwrap_or(0) + 1;
    fields
        .into_iter()
        .map(|(name, value)| format!("{name:<width$}{value}"))
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

/// Whether an entity has a job, for a caller that has an id and a world
/// rather than the entity itself.
fn is_employed(sim: Option<&Sim>, uid: Uid) -> bool {
    sim.and_then(|sim| sim.0.entities().get(uid))
        .is_some_and(|entity| entity.job().is_some())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Point;
    use crate::sim::Human;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn human() -> Human {
        let mut rng = SmallRng::seed_from_u64(1);
        Human::new(Uid::new(EntityType::Human, 5), Point::new(1, 1), &mut rng)
    }

    /// What a block says for one field. The names are padded to align, and by
    /// how much depends on the longest name in the block, so a test reads the
    /// value rather than counting spaces.
    fn field(block: &str, name: &str) -> String {
        block
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or_else(|| panic!("no {name:?} line in:\n{block}"))
            .trim()
            .to_string()
    }

    #[test]
    fn the_items_menu_says_what_is_carried_and_how_close_the_limits_are() {
        let mut person = human();
        let empty = items_block(&person);
        assert_eq!(field(&empty, "hand"), "empty");
        assert_eq!(field(&empty, "stowed"), "nothing");
        assert_eq!(field(&empty, "mass"), "0.0 / 10.0 kg");

        person.set_carried(Some(ItemKind::Water));
        let inventory = person.inventory_mut();
        assert!(inventory.stow(ItemKind::Food));
        assert!(inventory.stow(ItemKind::Food));

        let block = items_block(&person);
        assert_eq!(field(&block, "hand"), "water");
        assert_eq!(field(&block, "food"), "2");
        // The water in its hand is in the kilos and not in the litres: 0.5 of
        // water plus 1.0 of food by weight, and 2.0 of food alone by volume.
        assert_eq!(field(&block, "mass"), "1.5 / 10.0 kg");
        assert_eq!(field(&block, "space"), "2.0 / 12.0 L");
    }

    #[test]
    fn the_talents_menu_shows_all_nine_scores_and_follows_explicit_changes() {
        let mut person = human();
        let block = talents_block(&person);
        assert_eq!(block.lines().count(), Talent::ALL.len());
        for talent in Talent::ALL {
            assert_eq!(field(&block, talent.name()), format!("{} / 100", person.talents().get(talent)));
        }
        GameEntity::talents_mut(&mut person).unwrap().set(Talent::Precision, 100);
        assert_eq!(field(&talents_block(&person), "precision"), "100 / 100");
    }

    #[test]
    fn a_dog_has_no_human_talent_scores_to_show() {
        let mut rng = SmallRng::seed_from_u64(1);
        let dog = crate::sim::Dog::new(
            Uid::new(EntityType::Dog, 1), Point::new(0, 0), crate::sim::Facing::Left, &mut rng,
        );
        assert_eq!(talents_block(&dog), "no talents");
    }

    #[test]
    fn a_kind_that_carries_nothing_says_so_rather_than_showing_empty_limits() {
        // A dog has no inventory at all, and a row of zeroes it can never move
        // off would read as a bug rather than as a fact about dogs.
        struct Nobody(crate::sim::Body);
        impl GameEntity for Nobody {
            fn save(&self) -> crate::save::pb::Kind {
                unreachable!("a test's stand-in is never saved")
            }

            fn body(&self) -> &crate::sim::Body {
                &self.0
            }
            fn body_mut(&mut self) -> &mut crate::sim::Body {
                &mut self.0
            }
            fn think(&self, _: &crate::sim::Think<'_>) -> crate::sim::Intent {
                crate::sim::Intent::Idle
            }
        }
        let nobody = Nobody(crate::sim::Body::at_cell(
            Uid::new(EntityType::Dog, 1),
            Point::new(0, 0),
        ));
        assert_eq!(items_block(&nobody), "carries nothing");
    }

    /// `hours` o'clock on day `day`; day 1 is a Monday.
    fn on(day: u32, hours: f32) -> Clock {
        use crate::sim::clock::{DAY, HOUR, TIME_SCALE};
        let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    #[test]
    fn the_job_menu_says_what_the_job_is_and_when() {
        let mut person = human();
        assert_eq!(job_block(&person, on(1, 10.0)), "unemployed");
        assert_eq!(employ_label(&person), "hire");

        assert!(GameEntity::hire(&mut person, crate::sim::Job::office(Point::new(12, 4))));
        assert_eq!(employ_label(&person), "dismiss");
        let block = job_block(&person, on(2, 7.0));
        assert_eq!(field(&block, "job"), "office worker");
        assert_eq!(field(&block, "at"), "computer at 12, 4");
        assert_eq!(field(&block, "hours"), "09:00-17:00");
        assert_eq!(field(&block, "days"), "Mon-Fri");
        assert_eq!(field(&block, "now"), "off until 09:00");
        assert_eq!(field(&job_block(&person, on(2, 18.0)), "now"), "done for the day");
        assert_eq!(field(&job_block(&person, on(6, 11.0)), "now"), "weekend");
        // On shift, and not yet thinking about it — a brain that has not run
        // is idle: away from the desk for something else.
        assert_eq!(field(&job_block(&person, on(2, 11.0)), "now"), "on a break: idle");
    }

    #[test]
    fn a_scrap_collectors_job_menu_names_its_bin() {
        let mut person = human();
        assert!(GameEntity::hire(&mut person, crate::sim::Job::collector(Point::new(30, 2))));
        let block = job_block(&person, on(2, 7.0));
        assert_eq!(field(&block, "job"), "scrap collector");
        assert_eq!(field(&block, "at"), "scrap bin at 30, 2");
        assert_eq!(field(&block, "hours"), "09:00-17:00");
        assert_eq!(field(&block, "now"), "off until 09:00");
    }

    /// Beside its bin and thirsty: seeing to it before the trip is part of
    /// the job, not a break from it.
    #[test]
    fn a_collector_getting_ready_for_a_trip_says_so_rather_than_on_a_break() {
        use crate::sim::testing::{needy_human, prop_at, World};
        let bin = Point::new(4, 3);
        let mut map = crate::map::Map::new(crate::map::Size::new(9, 7), crate::map::FLOOR);
        prop_at(&mut map, "scrap bin", bin);
        prop_at(&mut map, "fridge", Point::new(7, 1));
        let mut world = World::new(map);
        world.clock = on(2, 10.0);
        let mut person = needy_human(Point::new(3, 3), 0.0, 40.0, 0.0);
        assert!(GameEntity::hire(&mut person, crate::sim::Job::collector(bin)));
        world.step(&mut person);
        assert_eq!(person.current_goal(), Some(crate::sim::brain::GoalId::Drink));
        assert_eq!(field(&job_block(&person, world.clock), "now"), "getting ready: drink");
    }

    #[test]
    fn a_guards_job_menu_names_its_post_and_its_watch() {
        use crate::sim::job::WATCHES;
        let mut person = human();
        assert!(GameEntity::hire(&mut person, crate::sim::Job::guard(Point::new(0, 20), WATCHES[1])));
        let block = job_block(&person, on(6, 10.0));
        assert_eq!(field(&block, "job"), "guard");
        assert_eq!(field(&block, "at"), "guard post at 0, 20");
        assert_eq!(field(&block, "hours"), "16:00-24:00");
        assert_eq!(field(&block, "days"), "Mon-Sun");
        assert_eq!(field(&block, "now"), "off until 16:00", "a Saturday is a day like any other");

        let mut person = human();
        assert!(GameEntity::hire(&mut person, crate::sim::Job::guard(Point::new(0, 20), WATCHES[2])));
        assert_eq!(field(&job_block(&person, on(2, 10.0)), "now"), "off until 00:00", "the night watch, after it");
        assert_eq!(field(&job_block(&person, on(2, 23.75)), "now"), "on a break: idle", "due, and not set off");
    }

    /// A playzone administrator's menu names the computer, the shift and the
    /// days of its group, and a day of the other group's is a day off.
    #[test]
    fn a_playzone_administrators_job_menu_names_its_shift_and_its_groups_days() {
        use crate::sim::job::{JobKind, PLAYZONE_SHIFTS};
        let mut person = human();
        assert!(GameEntity::hire(&mut person, crate::sim::Job::new(JobKind::PlayzoneAdministrator, Point::new(40, 22), PLAYZONE_SHIFTS[2])));
        let block = job_block(&person, on(1, 7.0));
        assert_eq!(field(&block, "job"), "playzone administrator");
        assert_eq!(field(&block, "at"), "playzone computer at 40, 22");
        assert_eq!(field(&block, "hours"), "16:00-22:00");
        assert_eq!(field(&block, "days"), "Mon, Wed, Fri, Sun");
        assert_eq!(field(&block, "now"), "off until 16:00", "a Monday: the first group's");
        assert_eq!(field(&job_block(&person, on(2, 17.0)), "now"), "day off", "a Tuesday: the second's");
        assert_eq!(field(&job_block(&person, on(1, 23.0)), "now"), "done for the day");
    }

    #[test]
    fn a_dog_has_no_job_and_no_button_to_give_it_one() {
        let mut rng = SmallRng::seed_from_u64(1);
        let dog = crate::sim::Dog::new(
            Uid::new(EntityType::Dog, 1), Point::new(0, 0), crate::sim::Facing::Left, &mut rng,
        );
        assert_eq!(job_block(&dog, on(1, 10.0)), "no job");
    }

    #[test]
    fn every_value_in_a_block_starts_in_the_same_column_after_a_space() {
        let block = field_block(vec![
            ("goal", "wander".to_string()),
            ("priority", "wander 0.10".to_string()),
            ("mental_health", "50.0".to_string()),
        ]);
        let columns: Vec<usize> = block
            .lines()
            .map(|line| {
                let name = line.split(' ').next().unwrap();
                assert!(line[name.len()..].starts_with(' '), "no gap after {name:?} in {line:?}");
                line.len() - line.trim_start_matches(|c: char| c != ' ').trim_start().len()
            })
            .collect();
        assert!(columns.windows(2).all(|pair| pair[0] == pair[1]), "{block}");
    }
}
