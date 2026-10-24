//! `economy.pb`: the district's money (`crate::sim::economy::Economy`) — the
//! budget, every unit's account, the books kept a day at a time, and the
//! tariffs the game is being played with.
//!
//! **Optional**: a save from before there was money has none, and loading it
//! opens an account for every human there is (`Economy::fresh`).

/// The whole economy.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Economy {
    /// What the district has, in cents. May be below zero.
    #[prost(sint64, tag = "1")]
    pub budget: i64,
    /// Every account, by uid.
    #[prost(message, repeated, tag = "2")]
    pub accounts: Vec<Account>,
    /// Where the clerks' round of the accounts has got to: the uid of the
    /// first account the next payroll looks at. Absent, the first account.
    #[prost(fixed64, optional, tag = "3")]
    pub next: Option<u64>,
    /// The books, a day each, oldest first.
    #[prost(message, repeated, tag = "4")]
    pub books: Vec<DayBook>,
    /// The last day the accountant has closed the books for.
    #[prost(uint32, tag = "5")]
    pub closed_through: u32,
    #[prost(message, optional, tag = "6")]
    pub tariffs: Option<Tariffs>,
}

/// One unit's account.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Account {
    #[prost(fixed64, tag = "1")]
    pub uid: u64,
    /// Cents. May be below zero: in debt.
    #[prost(sint64, tag = "2")]
    pub cash: i64,
    /// Shifts worked and not paid yet.
    #[prost(uint32, tag = "3")]
    pub shifts: u32,
    /// The job they were worked as: `JobKind::number`. Absent with none owed.
    #[prost(int32, optional, tag = "4")]
    pub job: Option<i32>,
    /// Days gone by out of work and not paid a care package for yet.
    #[prost(uint32, tag = "5")]
    pub care_days: u32,
    /// Dead: owed nothing, paid nothing.
    #[prost(bool, tag = "6")]
    pub dead: bool,
}

/// One day's money, in cents.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct DayBook {
    #[prost(uint32, tag = "1")]
    pub day: u32,
    /// The budget as the day began.
    #[prost(sint64, tag = "2")]
    pub opening: i64,
    #[prost(sint64, tag = "3")]
    pub wages: i64,
    #[prost(sint64, tag = "4")]
    pub taxes: i64,
    #[prost(sint64, tag = "5")]
    pub care: i64,
    #[prost(sint64, tag = "6")]
    pub sales: i64,
    #[prost(sint64, tag = "7")]
    pub estates: i64,
    #[prost(sint64, tag = "8")]
    pub grants: i64,
}

/// The numbers the economy is played with. Wages and prices **by name**, as
/// content always is in a save.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Tariffs {
    #[prost(uint32, tag = "1")]
    pub tax_percent: u32,
    #[prost(sint64, tag = "2")]
    pub care_package: i64,
    #[prost(message, repeated, tag = "3")]
    pub wages: Vec<Priced>,
    #[prost(message, repeated, tag = "4")]
    pub prices: Vec<Priced>,
    #[prost(sint64, tag = "5")]
    pub budget_per_arrival: i64,
    #[prost(sint64, tag = "6")]
    pub cash_min: i64,
    #[prost(sint64, tag = "7")]
    pub cash_max: i64,
    #[prost(uint32, tag = "8")]
    pub minutes_per_account: u32,
}

/// A job's wage or an item's price: its name and the cents.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Priced {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(sint64, tag = "2")]
    pub cents: i64,
}
