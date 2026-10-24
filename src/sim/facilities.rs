//! [`Facilities`]: the world's parts beside the map and the crowd — where
//! the useful props are and what is in them, the doors, whose rooms, beds and
//! workplaces are whose, the dead, the fields, and what has been counted.
//!
//! One struct, held by [`GameState`](super::GameState) and by the tests'
//! one-unit world (`testing::World`) alike, so **a new part of the world is
//! one field here**: built in [`Facilities::new`], read by a brain through
//! [`Think::new`](super::Think) if it needs to be, written by an
//! [`Effect`](super::Effect) in [`effect::apply`](super::effect::apply) — and
//! both worlds, and every test that hands a brain a world to look at, have it.
//! Saved by `sim::saving`, like everything else in a `GameState`.

use crate::map::utilities::Supply;
use crate::map::Map;

use super::bins::Bins;
use super::corpses::Corpses;
use super::door::Doors;
use super::feature::Features;
use super::fields::Fields;
use super::fridge::Fridges;
use super::homes::Homes;
use super::job::Workplaces;
use super::playzone::Playzones;
use super::property::Properties;
use super::statistics::Tally;
use super::uid::Uid;

#[derive(Default)]
pub struct Facilities {
    /// Where the props a brain can use are. Derived from the map, and again
    /// whenever the power changes (`GameState::refresh_supply`): a fridge or
    /// computer on a dead box is not somewhere to go.
    pub features: Features,
    pub electrical: super::electrical::Electrical,
    /// Every fridge's door and temperature. Derived from the map once; what
    /// is stored here changes every tick a door is open, through
    /// `Effect::Fridge`.
    pub fridges: Fridges,
    /// What every store holds ([`super::bins`]), filled and emptied through
    /// `Effect::Deposit` and `Effect::Withdraw`.
    pub bins: Bins,
    /// Every door's leaf and lock ([`super::door`]), moved through
    /// `Effect::Door`.
    pub doors: Doors,
    /// Which cells are whose property: the rooms behind the house doors.
    /// Derived from the map once ([`super::property`]).
    pub properties: Properties,
    /// Who lies dead where: filed by `Effect::Died`, taken off by
    /// `Effect::TakeBody`. Worked out again from the entities on load
    /// ([`super::corpses`]).
    pub corpses: Corpses,
    /// Which beds have been given to whom. Written by the spawn pass and read
    /// by nothing in a tick — a unit remembers its own bed ([`super::homes`]).
    pub homes: Homes,
    /// Which workplaces have been given to whom, the same way
    /// ([`super::job`]).
    pub workplaces: Workplaces,
    /// The dead, counted as they die and as they are cremated: what the world
    /// no longer shows once a body has gone ([`super::statistics`]).
    pub tally: Tally,
    /// Every crop in the fields ([`super::fields`]): sown, tended and
    /// harvested through `Effect::Sow`, `Tend` and `Harvest`, which plant and
    /// unplant the map with it.
    pub fields: Fields,
    /// Which arcade cabinets work: those whose playzone computer an
    /// administrator was running as the last tick ended (`Effect::Operate`,
    /// [`super::playzone`]).
    pub playzones: Playzones,
    /// The bodies taken up this tick, to leave the world when the processing
    /// pass is over (`process_game_state`) — the one pass that may not remove
    /// anybody. Empty between ticks, so never saved.
    pub burials: Vec<Uid>,
}

impl Facilities {
    /// Everything on `map`, working as far as `supply` reaches: nobody in a
    /// bed or at a desk yet, nothing stored and nobody dead.
    pub fn new(map: &Map, supply: &Supply) -> Facilities {
        let properties = Properties::from_map(map);
        Facilities {
            electrical: Default::default(),
            features: Features::with_properties(map, supply, &properties),
            fridges: Fridges::from_map(map, supply),
            bins: Bins::from_map(map),
            doors: Doors::from_map(map, &properties),
            properties,
            corpses: Corpses::new(),
            homes: Homes::new(),
            workplaces: Workplaces::new(),
            tally: Tally::default(),
            fields: Fields::new(),
            playzones: Playzones::from_map(map),
            burials: Vec::new(),
        }
    }
}
