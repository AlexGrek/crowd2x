//! A unit's mind, inside its entity in `entities/NNNN.pb`.
//!
//! Only what changes. Which routines and goal executors a kind's brain has is
//! fixed by its kind (`Brain::human`, `Brain::dog`), so loading builds that
//! brain fresh and fills its state in; a save holds the state, matched up by
//! routine name and by goal.

use super::Cell;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Brain {
    #[prost(message, optional, tag = "1")]
    pub perception: Option<Perception>,
    #[prost(message, optional, tag = "2")]
    pub attention: Option<Attention>,
    #[prost(message, optional, tag = "3")]
    pub memory: Option<Memory>,
    /// In the brain's own order, each checked against the routine there by
    /// name.
    #[prost(message, repeated, tag = "4")]
    pub routines: Vec<RoutineState>,
    #[prost(message, optional, tag = "5")]
    pub goals: Option<Goals>,
    /// One per executor that has any state.
    #[prost(message, repeated, tag = "6")]
    pub executors: Vec<ExecutorState>,
    /// The task queue, front first.
    #[prost(message, repeated, tag = "7")]
    pub queue: Vec<Task>,
    /// What the current task last wrote.
    #[prost(enumeration = "TaskResult", tag = "8")]
    pub result: i32,
    /// The task being carried out.
    #[prost(message, optional, tag = "9")]
    pub task: Option<Task>,
    #[prost(message, optional, tag = "10")]
    pub action: Option<Action>,
    /// The task that ended since the goal was last asked.
    #[prost(message, optional, tag = "11")]
    pub finished: Option<Task>,
    /// Who refused the move that ended it.
    #[prost(fixed64, optional, tag = "12")]
    pub blocked_by: Option<u64>,
    #[prost(enumeration = "TaskResult", tag = "13")]
    pub last_result: i32,
    /// Its job, if it has one. Absent — every save from before there were
    /// jobs — is somebody with none.
    #[prost(message, optional, tag = "14")]
    pub job: Option<Job>,
}

/// A job: what it is, where, and when (`crate::sim::job::Job`).
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Job {
    #[prost(enumeration = "JobKind", tag = "1")]
    pub kind: i32,
    /// The desk, or whatever the job is done at.
    #[prost(message, optional, tag = "2")]
    pub workplace: Option<Cell>,
    /// The hour the shift starts, 0-23...
    #[prost(uint32, tag = "3")]
    pub start: u32,
    /// ...and the hour it ends, after `start`, the same day.
    #[prost(uint32, tag = "4")]
    pub end: u32,
    /// Which days it is worked: bit 0 Monday to bit 6 Sunday.
    #[prost(uint32, tag = "5")]
    pub days: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum JobKind {
    OfficeWorker = 0,
    ScrapCollector = 1,
    Guard = 2,
    Undertaker = 3,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Perception {
    #[prost(enumeration = "Heading", tag = "1")]
    pub heading: i32,
    /// How many it saw near, and far, at its last look.
    #[prost(uint32, tag = "2")]
    pub near: u32,
    #[prost(uint32, tag = "3")]
    pub far: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Heading {
    East = 0,
    NorthEast = 1,
    North = 2,
    NorthWest = 3,
    West = 4,
    SouthWest = 5,
    South = 6,
    SouthEast = 7,
}

/// The faces in mind, slot for slot.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Attention {
    #[prost(uint32, repeated, tag = "1")]
    pub keys: Vec<u32>,
    #[prost(uint32, repeated, tag = "2")]
    pub unseen: Vec<u32>,
    #[prost(uint32, tag = "3")]
    pub met: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Memory {
    #[prost(message, repeated, tag = "1")]
    pub keyed: Vec<Remembered>,
    /// Places out of reach, slot for slot; empty slots left out.
    #[prost(message, repeated, tag = "2")]
    pub out_of_reach: Vec<OutOfReach>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Remembered {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(oneof = "Recall", tags = "2, 3, 4, 5, 6")]
    pub recall: Option<Recall>,
}

#[derive(Clone, PartialEq, ::prost::Oneof)]
pub enum Recall {
    #[prost(message, tag = "2")]
    Cell(Cell),
    #[prost(fixed64, tag = "3")]
    Entity(u64),
    #[prost(float, tag = "4")]
    Number(f32),
    #[prost(uint64, tag = "5")]
    Time(u64),
    #[prost(string, tag = "6")]
    Text(String),
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct OutOfReach {
    #[prost(uint32, tag = "1")]
    pub slot: u32,
    #[prost(message, optional, tag = "2")]
    pub cell: Option<Cell>,
    /// When it is forgiven, in world seconds.
    #[prost(double, tag = "3")]
    pub until: f64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RoutineState {
    /// `Routine::name`, checked against the routine in the same place.
    #[prost(string, tag = "1")]
    pub name: String,
    /// Whether a routine with hysteresis has committed to its goal.
    #[prost(bool, tag = "2")]
    pub committed: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Goals {
    #[prost(enumeration = "GoalId", tag = "1")]
    pub top: i32,
    #[prost(message, repeated, tag = "2")]
    pub slots: Vec<GoalSlot>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct GoalSlot {
    #[prost(enumeration = "GoalId", tag = "1")]
    pub goal: i32,
    #[prost(float, tag = "2")]
    pub priority: f32,
    #[prost(uint32, tag = "3")]
    pub cooldown: u32,
    #[prost(uint32, tag = "4")]
    pub strikes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum GoalId {
    Idle = 0,
    Wander = 1,
    Eat = 2,
    Drink = 3,
    Relieve = 4,
    Play = 5,
    Sleep = 6,
    Salvage = 7,
    Work = 8,
    Collect = 9,
    Bury = 10,
}

/// One goal executor's state.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ExecutorState {
    #[prost(enumeration = "GoalId", tag = "1")]
    pub goal: i32,
    #[prost(oneof = "Executor", tags = "2, 3, 4")]
    pub state: Option<Executor>,
}

#[derive(Clone, PartialEq, ::prost::Oneof)]
pub enum Executor {
    #[prost(message, tag = "2")]
    Wander(Wander),
    /// Eat and drink: a stage and a seek.
    #[prost(message, tag = "3")]
    Fetch(Fetch),
    /// Relieve, play, sleep and salvage: a seek and nothing else.
    #[prost(message, tag = "4")]
    Seek(Seek),
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Wander {
    #[prost(message, optional, tag = "1")]
    pub target: Option<Cell>,
    #[prost(uint32, tag = "2")]
    pub retries: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Fetch {
    #[prost(enumeration = "FetchStage", tag = "1")]
    pub stage: i32,
    #[prost(message, optional, tag = "2")]
    pub seek: Option<Seek>,
}

/// Where eating or drinking has got to. The two goals' stages, which are the
/// same but for the name of the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum FetchStage {
    Finding = 0,
    Clearing = 1,
    Walking = 2,
    Opening = 3,
    Taking = 4,
    Closing = 5,
    /// Eating, or drinking.
    Consuming = 6,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Seek {
    #[prost(message, optional, tag = "1")]
    pub target: Option<Cell>,
    #[prost(uint32, tag = "2")]
    pub retries: u32,
    #[prost(uint32, tag = "3")]
    pub reroutes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum TaskResult {
    InProgress = 0,
    Executing = 1,
    Failed = 2,
    Success = 3,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Task {
    #[prost(oneof = "TaskKind", tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16")]
    pub kind: Option<TaskKind>,
}

#[derive(Clone, PartialEq, ::prost::Oneof)]
pub enum TaskKind {
    #[prost(message, tag = "1")]
    MoveTo(MoveTo),
    #[prost(message, tag = "2")]
    TakeItem(TakeItem),
    #[prost(message, tag = "3")]
    ConsumeItem(ConsumeItem),
    #[prost(message, tag = "4")]
    UseToilet(Timed),
    #[prost(message, tag = "5")]
    UseComputer(Timed),
    #[prost(message, tag = "6")]
    Sleep(Timed),
    #[prost(message, tag = "7")]
    Wait(Timed),
    #[prost(message, tag = "8")]
    OpenFridge(Timed),
    #[prost(message, tag = "9")]
    CloseFridge(Timed),
    #[prost(message, tag = "10")]
    Salvage(Timed),
    #[prost(message, tag = "11")]
    Work(Timed),
    /// Emptying the pack into a scrap bin.
    #[prost(message, tag = "12")]
    Deposit(Timed),
    /// Salvaging a heap for a living rather than to an order: the same
    /// work, and it tires (`Salvage::for_work`).
    #[prost(message, tag = "13")]
    SalvageForWork(Timed),
    /// A stretch at a guard post: the office's `Work`, kept as a watch
    /// (`Work::watch`).
    #[prost(message, tag = "14")]
    KeepWatch(Timed),
    /// An undertaker taking up the body lying in a cell.
    #[prost(message, tag = "15")]
    TakeBody(Timed),
    /// ...and putting it in a crematorium.
    #[prost(message, tag = "16")]
    Cremate(Timed),
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct MoveTo {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(bool, tag = "2")]
    pub beside: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TakeItem {
    #[prost(message, optional, tag = "1")]
    pub from: Option<Cell>,
    #[prost(string, tag = "2")]
    pub item: String,
    #[prost(float, tag = "3")]
    pub seconds: f32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConsumeItem {
    #[prost(string, tag = "1")]
    pub item: String,
    #[prost(float, tag = "2")]
    pub seconds: f32,
}

/// A task that is a cell and a time: the toilet, the computer, the bed, the
/// fridge, the heap — or no cell, for a wait.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Timed {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(float, tag = "2")]
    pub seconds: f32,
}

/// What a task is doing with the body. Absent, or no kind, for nothing.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Action {
    #[prost(oneof = "ActionKind", tags = "1, 2, 3, 4")]
    pub kind: Option<ActionKind>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Oneof)]
pub enum ActionKind {
    #[prost(message, tag = "1")]
    Move(MoveAction),
    #[prost(message, tag = "2")]
    Interact(InteractAction),
    #[prost(message, tag = "3")]
    Consume(TimedAction),
    #[prost(message, tag = "4")]
    Wait(TimedAction),
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct MoveAction {
    #[prost(message, optional, tag = "1")]
    pub to: Option<Cell>,
    #[prost(float, tag = "2")]
    pub elapsed: f32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct InteractAction {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(float, tag = "2")]
    pub seconds: f32,
    #[prost(float, tag = "3")]
    pub elapsed: f32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct TimedAction {
    #[prost(float, tag = "1")]
    pub seconds: f32,
    #[prost(float, tag = "2")]
    pub elapsed: f32,
}
