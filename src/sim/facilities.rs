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
use super::economy::Economy;
use super::feature::Features;
use super::fields::Fields;
use super::fridge::Fridges;
use super::homes::Homes;
use super::job::Workplaces;
use super::panics::Panics;
use super::playzone::Playzones;
use super::property::Properties;
use super::refuge::WaysIn;
use super::relationships::{Chats, Smalltalks, Society};
use super::statistics::Tally;
use super::uid::Uid;
use super::wounds::Wounds;

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
    pub hospital: super::hospital::Hospital,
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
    pub flowers: super::flowers::Flowers,
    pub dirt: super::dirt::DirtLayer,
    /// Which arcade cabinets work: those whose playzone computer an
    /// administrator was running as the last tick ended (`Effect::Operate`,
    /// [`super::playzone`]).
    pub playzones: Playzones,
    /// The district's money: the budget, every unit's account, the books
    /// ([`super::economy`]). Changed by `Effect::Record`, `Payroll`, `Buy`
    /// and `CloseBooks`, and by the spawn pass as units come and go.
    pub economy: Economy,
    /// Who was seen running in a panic this tick: written by the move step,
    /// read by react ([`super::panics`]). Rebuilt every tick, never saved.
    pub panics: Panics,
    /// The bites the world step dealt, each felt by the one bitten in its
    /// own react next tick ([`super::wounds`]). Saved: it is held between
    /// ticks.
    pub wounds: Wounds,
    /// The ways in from the wasteland, and the refuge behind each: where
    /// somebody panicking runs ([`super::refuge`]). Derived from the map.
    pub ways_in: WaysIn,
    /// Everybody's family and job — what the floors of their relationships
    /// are made of ([`super::relationships`]). Kept by the spawn pass as
    /// people arrive, are hired and let go, die and leave; worked out again
    /// from the entities on load, so never saved.
    pub society: Society,
    /// Who is chatting with whom this tick: gathered from the crowd by the
    /// think step, read by react ([`super::relationships::Chats`]). Rebuilt
    /// every tick, never saved.
    pub chats: Chats,
    /// Who is making small talk with whom this tick, gathered like `chats`
    /// ([`super::relationships::Smalltalks`]). Rebuilt every tick, never
    /// saved.
    pub smalltalks: Smalltalks,
    /// Who is bonded to whom — a person to its pet dog — gathered from the
    /// crowd by the think step like `chats` ([`super::pets::Bonds`]).
    /// Rebuilt every tick, never saved.
    pub bonds: super::pets::Bonds,
    /// The dog foods given as the last tick ended, each for its dog to feel
    /// in its own react ([`super::pets::Feedings`]). Saved: it is held
    /// between ticks.
    pub feedings: super::pets::Feedings,
    /// Who leaves the world this tick: the bodies undertakers took up
    /// (`Effect::TakeBody`) and the deathdogs gone back into the wasteland
    /// (`Effect::Depart`). Despawned at the end of the processing pass
    /// (`sim::comings_and_goings`), once the table it may not change is let
    /// go. Empty between ticks, so never saved.
    pub leaving: Vec<Uid>,
    /// The jobs the dead left this tick (`Effect::Died`), each with who left
    /// it: given to somebody jobless at the end of the processing pass
    /// (`sim::comings_and_goings`, `GameState::fill_vacancy`). Empty between
    /// ticks, so never saved.
    pub vacancies: Vec<(Uid, super::Job)>,
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
            hospital: super::hospital::Hospital::from_map(map),
            workplaces: Workplaces::new(),
            tally: Tally::default(),
            fields: Fields::new(),
            flowers: Default::default(),
            dirt: super::dirt::DirtLayer::new(map),
            playzones: Playzones::from_map(map),
            economy: Economy::default(),
            panics: Panics::default(),
            wounds: Wounds::default(),
            ways_in: WaysIn::from_map(map),
            society: Society::new(),
            chats: Chats::default(),
            smalltalks: Smalltalks::default(),
            bonds: Default::default(),
            feedings: Default::default(),
            leaving: Vec::new(),
            vacancies: Vec::new(),
        }
    }
}
