//! The district's money: [`Economy`] — every unit's [`Account`], the
//! district's budget, the books kept a day at a time ([`DayBook`]) — and the
//! [`Tariffs`] it is run by, read from `economy.yaml`.
//!
//! **Where money comes from and goes.** Every arrival is granted
//! [`Tariffs::budget_per_arrival`] into the budget and arrives with a little
//! cash of its own ([`Tariffs::starting_cash`]). Every shift a worker is on
//! duty for is recorded on its account (`brain::timesheet`,
//! [`Effect::Record`](super::Effect::Record)), and so is every day gone by
//! out of work; **office workers pay them, at their desks, an account at a
//! time** ([`Economy::pay_next`], the `Payroll` task): the wage times the
//! shifts, less tax, and a care package for every day without a job, out of
//! the budget. Food out of a fridge costs its price ([`Economy::buy`]), and a
//! unit may go as far into debt as it likes. What the dead had goes back to
//! the budget when the body leaves the world ([`Economy::close`]).
//!
//! **The books.** Every day has a [`DayBook`]: the budget it opened with and
//! what went in and out. The accountant closes them a day at a time
//! ([`Economy::unclosed_day`], the `CloseBooks` task) and says how the day,
//! the week and the budget stand.
//!
//! Plain Rust, in cents ([`Money`]), with no rounding but the tax's: two runs
//! keep the same books to the cent. A field of `Facilities`, read by every
//! brain through `Think::economy`, changed only by the world step's effects.

use std::collections::BTreeMap;
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};
use std::sync::LazyLock;

use rand::RngExt;
use serde::Deserialize;

use crate::save::{invalid, pb, Invalid};

use super::clock::Weekday;
use super::item::ItemKind;
use super::job::JobKind;
use super::log::Log;
use super::uid::Uid;

/// An amount of money, in whole **cents**. May be below zero: a debt, or a
/// budget overspent.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn cents(cents: i64) -> Money {
        Money(cents)
    }

    pub const fn dollars(dollars: i64) -> Money {
        Money(dollars * 100)
    }

    pub const fn as_cents(self) -> i64 {
        self.0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// `pct` percent of it, to the nearest cent, half a cent away from zero.
    pub fn percent(self, pct: u32) -> Money {
        let scaled = self.0 as i128 * pct as i128;
        let (whole, rest) = (scaled / 100, scaled % 100);
        let rounded = if rest.abs() * 2 >= 100 { whole + scaled.signum() } else { whole };
        Money(rounded as i64)
    }

    /// With its sign, whichever it is: `+$192.00`, `-$3.00`.
    pub fn signed(self) -> String {
        if self.is_negative() { self.to_string() } else { format!("+{self}") }
    }

    /// Dollars as a file writes them (`1.5`), to the cent; `None` for a
    /// negative or nonsense amount.
    fn from_dollars(dollars: f64) -> Option<Money> {
        (dollars.is_finite() && dollars >= 0.0 && dollars * 100.0 < i64::MAX as f64).then(|| Money((dollars * 100.0).round() as i64))
    }
}

/// `$1,234.56`, `-$3.00`. Pads like a string, so `{:>12}` lines them up.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cents = self.0.unsigned_abs();
        let digits = (cents / 100).to_string();
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, digit) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        let sign = if self.0 < 0 { "-" } else { "" };
        f.pad(&format!("{sign}${grouped}.{:02}", cents % 100))
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, other: Money) -> Money {
        Money(self.0 + other.0)
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, other: Money) -> Money {
        Money(self.0 - other.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, other: Money) {
        self.0 += other.0;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, other: Money) {
        self.0 -= other.0;
    }
}

impl Mul<i64> for Money {
    type Output = Money;
    fn mul(self, times: i64) -> Money {
        Money(self.0 * times)
    }
}

impl Neg for Money {
    type Output = Money;
    fn neg(self) -> Money {
        Money(-self.0)
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, Add::add)
    }
}

/// The items a fridge sells: what the economy menu prices.
pub const SOLD: [ItemKind; 3] = [ItemKind::Food, ItemKind::Water, ItemKind::DogFood];

/// The numbers an economy is run by. Every one is a default in
/// `economy.yaml` ([`Tariffs::default`]), and every one can be changed while
/// playing ([`Tariffs::set`], `Command::SetTariff`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tariffs {
    /// The share of a wage paid back into the budget, 0-100.
    pub tax_percent: u32,
    /// What a day out of work is paid, untaxed.
    pub care_package: Money,
    /// A shift's wage, before tax, by [`JobKind`] number.
    pub wages: [Money; JobKind::COUNT],
    /// What a thing costs out of a fridge, by [`ItemKind`] number.
    pub prices: [Money; ItemKind::COUNT],
    /// What the budget is granted for every arrival.
    pub budget_per_arrival: Money,
    /// What an arrival has on it: whole dollars between the two, inclusive.
    pub starting_cash: (Money, Money),
    /// How long a clerk spends on an account, in world minutes. At least one.
    pub minutes_per_account: u32,
}

/// The file the defaults are, built into the game.
pub const DEFAULTS_YAML: &str = include_str!("../../economy.yaml");

static DEFAULTS: LazyLock<Tariffs> =
    LazyLock::new(|| Tariffs::from_yaml(DEFAULTS_YAML).unwrap_or_else(|error| panic!("economy.yaml: {error}")));

/// The checked-in `economy.yaml`: the one place a default is written.
impl Default for Tariffs {
    fn default() -> Tariffs {
        DEFAULTS.clone()
    }
}

/// `economy.yaml` as written: dollars, jobs and items by name.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TariffFile {
    tax: u32,
    #[serde(rename = "care package")]
    care_package: f64,
    #[serde(rename = "budget per arrival")]
    budget_per_arrival: f64,
    #[serde(rename = "starting cash")]
    starting_cash: (f64, f64),
    #[serde(rename = "minutes per account")]
    minutes_per_account: u32,
    wages: BTreeMap<String, f64>,
    prices: BTreeMap<String, f64>,
}

/// One number of the [`Tariffs`], for setting it on its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tariff {
    Tax,
    CarePackage,
    Wage(JobKind),
    Price(ItemKind),
    BudgetPerArrival,
    CashMin,
    CashMax,
    MinutesPerAccount,
}

impl Tariff {
    /// Every one the economy menu lists, in its order: the wages, then the
    /// rest.
    pub fn every() -> Vec<Tariff> {
        let mut every: Vec<Tariff> = JobKind::ALL.into_iter().map(Tariff::Wage).collect();
        every.push(Tariff::Tax);
        every.push(Tariff::CarePackage);
        every.extend(SOLD.into_iter().map(Tariff::Price));
        every.extend([Tariff::BudgetPerArrival, Tariff::CashMin, Tariff::CashMax, Tariff::MinutesPerAccount]);
        every
    }

    /// What the economy menu calls it: unique among [`Tariff::every`].
    pub fn label(self) -> &'static str {
        match self {
            Tariff::Tax => "tax",
            Tariff::CarePackage => "care package",
            Tariff::Wage(job) => job.name(),
            Tariff::Price(item) => item.name(),
            Tariff::BudgetPerArrival => "budget per arrival",
            Tariff::CashMin => "lowest starting cash",
            Tariff::CashMax => "highest starting cash",
            Tariff::MinutesPerAccount => "minutes per account",
        }
    }

    /// How much the menu's `lower` and `raise` move it, in its own units
    /// ([`Tariffs::get`]).
    pub fn step(self) -> i64 {
        match self {
            Tariff::Wage(_) | Tariff::CarePackage => 500,
            Tariff::Price(_) => 50,
            Tariff::Tax | Tariff::MinutesPerAccount => 1,
            Tariff::BudgetPerArrival => 10_000,
            Tariff::CashMin | Tariff::CashMax => 500,
        }
    }
}

impl Tariffs {
    /// The tariffs `economy.yaml` written as `text` says. Every job and every
    /// item named, nothing else; no amount below zero, no tax above 100%, the
    /// lowest starting cash no more than the highest, an account at least a
    /// minute.
    pub fn from_yaml(text: &str) -> Result<Tariffs, String> {
        let file: TariffFile = serde_yaml::from_str(text).map_err(|error| error.to_string())?;
        let money = |what: &str, dollars: f64| Money::from_dollars(dollars).ok_or_else(|| format!("{what}: {dollars} is not an amount"));
        if file.tax > 100 {
            return Err(format!("tax: {}% is more than all of it", file.tax));
        }
        if file.minutes_per_account == 0 {
            return Err("minutes per account: an account takes a minute at least".to_string());
        }
        let wages = by_name(&file.wages, "wages", &JobKind::ALL.map(JobKind::name), money)?;
        let prices = by_name(&file.prices, "prices", &ItemKind::ALL.map(ItemKind::name), money)?;
        let starting_cash = (money("starting cash", file.starting_cash.0)?, money("starting cash", file.starting_cash.1)?);
        if starting_cash.0 > starting_cash.1 {
            return Err(format!("starting cash: {} is more than {}", starting_cash.0, starting_cash.1));
        }
        Ok(Tariffs {
            tax_percent: file.tax,
            care_package: money("care package", file.care_package)?,
            wages: wages.try_into().expect("one a job"),
            prices: prices.try_into().expect("one an item"),
            budget_per_arrival: money("budget per arrival", file.budget_per_arrival)?,
            starting_cash,
            minutes_per_account: file.minutes_per_account,
        })
    }

    /// A shift's wage as `job`, before tax.
    pub fn wage(&self, job: JobKind) -> Money {
        self.wages[job as usize]
    }

    /// What `item` costs out of a fridge.
    pub fn price(&self, item: ItemKind) -> Money {
        self.prices[item as usize]
    }

    /// An arrival's cash: whole dollars, evenly between the lowest and the
    /// highest. One draw of `rng`.
    pub fn roll_cash(&self, rng: &mut impl RngExt) -> Money {
        let low = (self.starting_cash.0.0 + 99).div_euclid(100);
        let high = self.starting_cash.1.0.div_euclid(100).max(low);
        Money::dollars(rng.random_range(low..=high))
    }

    /// `tariff` in its own units: cents for money, percent for the tax,
    /// minutes for an account.
    pub fn get(&self, tariff: Tariff) -> i64 {
        match tariff {
            Tariff::Tax => self.tax_percent as i64,
            Tariff::CarePackage => self.care_package.0,
            Tariff::Wage(job) => self.wage(job).0,
            Tariff::Price(item) => self.price(item).0,
            Tariff::BudgetPerArrival => self.budget_per_arrival.0,
            Tariff::CashMin => self.starting_cash.0.0,
            Tariff::CashMax => self.starting_cash.1.0,
            Tariff::MinutesPerAccount => self.minutes_per_account as i64,
        }
    }

    /// Set `tariff` to `value` in its own units ([`Tariffs::get`]), kept in
    /// bounds: nothing below zero, the tax no more than 100%, an account a
    /// minute at least, the lowest starting cash no more than the highest.
    /// What it was set to.
    pub fn set(&mut self, tariff: Tariff, value: i64) -> i64 {
        let value = value.max(0);
        match tariff {
            Tariff::Tax => self.tax_percent = value.min(100) as u32,
            Tariff::CarePackage => self.care_package = Money(value),
            Tariff::Wage(job) => self.wages[job as usize] = Money(value),
            Tariff::Price(item) => self.prices[item as usize] = Money(value),
            Tariff::BudgetPerArrival => self.budget_per_arrival = Money(value),
            Tariff::CashMin => self.starting_cash.0 = Money(value).min(self.starting_cash.1),
            Tariff::CashMax => self.starting_cash.1 = Money(value).max(self.starting_cash.0),
            Tariff::MinutesPerAccount => self.minutes_per_account = value.clamp(1, u32::MAX as i64) as u32,
        }
        self.get(tariff)
    }

    /// `tariff` as the menu reads it: `$60.00 a shift`, `20%`, `5 minutes`.
    pub fn value_text(&self, tariff: Tariff) -> String {
        let value = self.get(tariff);
        match tariff {
            Tariff::Tax => format!("{value}%"),
            Tariff::Wage(_) => format!("{} a shift", Money(value)),
            Tariff::CarePackage => format!("{} a day", Money(value)),
            Tariff::MinutesPerAccount if value == 1 => "1 minute".to_string(),
            Tariff::MinutesPerAccount => format!("{value} minutes"),
            _ => Money(value).to_string(),
        }
    }

    /// What the log says when `tariff` has been set: `the wage of a guard is
    /// now $45.00 a shift`.
    pub fn said(&self, tariff: Tariff) -> String {
        let value = self.value_text(tariff);
        match tariff {
            Tariff::Tax => format!("the tax is now {value}"),
            Tariff::CarePackage => format!("the care package is now {value}"),
            Tariff::Wage(job) => format!("the wage of {} is now {value}", job.with_article()),
            Tariff::Price(item) => format!("the price of {} is now {value}", item.name()),
            Tariff::BudgetPerArrival => format!("the budget per arrival is now {value}"),
            Tariff::CashMin => format!("the lowest starting cash is now {value}"),
            Tariff::CashMax => format!("the highest starting cash is now {value}"),
            Tariff::MinutesPerAccount => format!("an account now takes {value}"),
        }
    }

    // --- saving ---

    fn save(&self) -> pb::Tariffs {
        let Tariffs { tax_percent, care_package, wages, prices, budget_per_arrival, starting_cash, minutes_per_account } = self;
        let priced = |name: &str, money: &Money| pb::Priced { name: name.to_string(), cents: money.0 };
        pb::Tariffs {
            tax_percent: *tax_percent,
            care_package: care_package.0,
            wages: JobKind::ALL.iter().zip(wages).map(|(job, wage)| priced(job.name(), wage)).collect(),
            prices: ItemKind::ALL.iter().zip(prices).map(|(item, price)| priced(item.name(), price)).collect(),
            budget_per_arrival: budget_per_arrival.0,
            cash_min: starting_cash.0.0,
            cash_max: starting_cash.1.0,
            minutes_per_account: *minutes_per_account,
        }
    }

    /// The tariffs a save was played with. A job or an item the save does not
    /// name — one added since — has its default; one this build does not
    /// know is refused.
    fn restore(saved: &pb::Tariffs) -> Result<Tariffs, Invalid> {
        let mut tariffs = Tariffs::default();
        let mut wage_names = std::collections::BTreeSet::new();
        for wage in &saved.wages {
            if wage.cents < 0 || !wage_names.insert(&wage.name) { return invalid("negative or duplicate wage"); }
            let Some(job) = JobKind::ALL.into_iter().find(|job| job.name() == wage.name) else {
                return invalid(format!("a wage for no such job {:?}", wage.name));
            };
            tariffs.wages[job as usize] = Money(wage.cents);
        }
        let mut price_names = std::collections::BTreeSet::new();
        for price in &saved.prices {
            if price.cents < 0 || !price_names.insert(&price.name) { return invalid("negative or duplicate price"); }
            let Some(item) = ItemKind::from_name(&price.name) else {
                return invalid(format!("a price for no such item {:?}", price.name));
            };
            tariffs.prices[item as usize] = Money(price.cents);
        }
        if saved.tax_percent > 100 || saved.minutes_per_account == 0 || saved.cash_min > saved.cash_max
            || saved.cash_min < 0 || saved.care_package < 0 || saved.budget_per_arrival < 0 {
            return invalid(format!(
                "tariffs of {}% tax, {} minutes an account and cash from {} to {}",
                saved.tax_percent, saved.minutes_per_account, saved.cash_min, saved.cash_max
            ));
        }
        tariffs.tax_percent = saved.tax_percent;
        tariffs.care_package = Money(saved.care_package);
        tariffs.budget_per_arrival = Money(saved.budget_per_arrival);
        tariffs.starting_cash = (Money(saved.cash_min), Money(saved.cash_max));
        tariffs.minutes_per_account = saved.minutes_per_account;
        Ok(tariffs)
    }
}

/// `names`, one each, read out of `section` by `read`: refused if one is
/// missing or the section names something else.
fn by_name<T>(
    section: &BTreeMap<String, f64>,
    what: &str,
    names: &[&str],
    read: impl Fn(&str, f64) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    if let Some(stranger) = section.keys().find(|key| !names.contains(&key.as_str())) {
        return Err(format!("{what}: there is no {stranger:?}"));
    }
    names
        .iter()
        .map(|&name| match section.get(name) {
            Some(&value) => read(&format!("{what}: {name}"), value),
            None => Err(format!("{what}: {name:?} is missing")),
        })
        .collect()
}

/// One unit's money, and what it is owed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Account {
    pub uid: Uid,
    /// What it has. Below zero is a debt, and a debt has no limit.
    pub cash: Money,
    /// Shifts worked and not paid for yet.
    pub shifts: u16,
    /// The job those shifts were worked as. A unit hired as something else
    /// before a clerk gets to it is paid for them all at the wage of the
    /// job it worked last.
    pub job: Option<JobKind>,
    /// Days gone by out of work, and not paid a care package for yet.
    pub care_days: u16,
    /// Dead: owed nothing, paid nothing, until the body leaves the world.
    pub dead: bool,
}

impl Account {
    fn new(uid: Uid, cash: Money) -> Account {
        Account { uid, cash, shifts: 0, job: None, care_days: 0, dead: false }
    }

    /// Whether a clerk has something to pay it.
    pub fn is_owed(&self) -> bool {
        !self.dead && (self.shifts > 0 || self.care_days > 0)
    }
}

/// One day's money.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayBook {
    pub day: u32,
    /// The budget as the day began.
    pub opening: Money,
    /// Paid out in wages, before tax.
    pub wages: Money,
    /// Paid back in tax on them.
    pub taxes: Money,
    /// Paid out in care packages.
    pub care: Money,
    /// Taken for what fridges sold.
    pub sales: Money,
    /// What the dead had, when their bodies left the world — a debt too.
    pub estates: Money,
    /// Granted for the day's arrivals: not earned, so not in the balance.
    pub grants: Money,
}

impl DayBook {
    fn open(day: u32, opening: Money) -> DayBook {
        let zero = Money::ZERO;
        DayBook { day, opening, wages: zero, taxes: zero, care: zero, sales: zero, estates: zero, grants: zero }
    }

    /// How the day went for the budget, the grants left out: what came in
    /// less what went out.
    pub fn balance(&self) -> Money {
        self.taxes + self.sales + self.estates - self.wages - self.care
    }

    /// The day of the week it was: day one is a Monday.
    pub fn weekday(&self) -> Weekday {
        Weekday::ALL[(self.day.max(1) - 1) as usize % 7]
    }
}

/// How many days' books are kept: four weeks. The accountant reads a week
/// back at most.
pub const BOOKS_KEPT: usize = 28;

/// How far back the accountant closes the books: a week. A day older that was
/// never closed — the accountant was off, or there was none — is left.
pub const CLOSES_BACK: u32 = 7;

/// The district's money: the budget, every unit's account and the books.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Economy {
    tariffs: Tariffs,
    /// What the district has to pay with. May go below zero: nothing stops a
    /// wage being paid.
    budget: Money,
    /// Every unit's, sorted by uid: humans only, from arrival until the body
    /// leaves the world.
    accounts: Vec<Account>,
    /// Where the clerks have got to: the next payroll starts at the first
    /// account at or after it, round to the start. `None`, the start.
    next: Option<Uid>,
    /// How many accounts are owed something ([`Account::is_owed`]). Kept in
    /// step, so a brain asks it for nothing; derived on load.
    owing: u32,
    /// A day each, oldest first, [`BOOKS_KEPT`] at most.
    books: Vec<DayBook>,
    /// The last day the accountant closed the books for.
    closed_through: u32,
}

impl Default for Economy {
    fn default() -> Economy {
        Economy::new(Tariffs::default())
    }
}

impl Economy {
    /// No money but the budget's nothing, nobody's account, no books.
    pub fn new(tariffs: Tariffs) -> Economy {
        Economy { tariffs, budget: Money::ZERO, accounts: Vec::new(), next: None, owing: 0, books: Vec::new(), closed_through: 0 }
    }

    pub fn tariffs(&self) -> &Tariffs {
        &self.tariffs
    }

    /// The tariffs to run by from now on: what is owed is paid at them.
    pub fn set_tariffs(&mut self, tariffs: Tariffs) {
        self.tariffs = tariffs;
    }

    /// One tariff, kept in bounds ([`Tariffs::set`]); what it was set to.
    pub fn set_tariff(&mut self, tariff: Tariff, value: i64) -> i64 {
        self.tariffs.set(tariff, value)
    }

    pub fn budget(&self) -> Money {
        self.budget
    }

    /// `uid`'s account, if it has one.
    pub fn account(&self, uid: Uid) -> Option<&Account> {
        self.find(uid).map(|at| &self.accounts[at])
    }

    /// Every account, by uid.
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    /// How many accounts a clerk has something to pay.
    pub fn owing(&self) -> u32 {
        self.owing
    }

    /// The books, oldest first.
    pub fn books(&self) -> &[DayBook] {
        &self.books
    }

    /// The book of `day`, if it is kept.
    pub fn book(&self, day: u32) -> Option<&DayBook> {
        self.books.iter().find(|book| book.day == day)
    }

    /// Today's book: the last opened.
    pub fn today(&self) -> Option<&DayBook> {
        self.books.last()
    }

    /// The last day the accountant has closed the books for.
    pub fn closed_through(&self) -> u32 {
        self.closed_through
    }

    /// The first day before `today` whose books are kept and not closed, of
    /// the last [`CLOSES_BACK`]: what the accountant closes next.
    pub fn unclosed_day(&self, today: u32) -> Option<u32> {
        self.books
            .iter()
            .map(|book| book.day)
            .find(|&day| day > self.closed_through && day < today && day + CLOSES_BACK >= today)
    }

    fn find(&self, uid: Uid) -> Option<usize> {
        self.accounts.binary_search_by_key(&uid, |account| account.uid).ok()
    }

    fn book_mut(&mut self) -> Option<&mut DayBook> {
        self.books.last_mut()
    }

    /// It is `day`: open its book, with the budget as it stands, if it is not
    /// open already. Every tick, first thing in the world step; idempotent.
    pub fn roll(&mut self, day: u32) {
        if self.books.last().is_some_and(|book| book.day >= day) {
            return;
        }
        self.books.push(DayBook::open(day, self.budget));
        if self.books.len() > BOOKS_KEPT {
            self.books.remove(0);
        }
    }

    /// `uid` has arrived with `cash`: its account opened, and the budget
    /// granted [`Tariffs::budget_per_arrival`] for it.
    pub fn open(&mut self, uid: Uid, cash: Money) {
        let Err(at) = self.accounts.binary_search_by_key(&uid, |account| account.uid) else {
            return;
        };
        self.accounts.insert(at, Account::new(uid, cash));
        let grant = self.tariffs.budget_per_arrival;
        self.budget += grant;
        if let Some(book) = self.book_mut() {
            book.grants += grant;
        }
    }

    /// `uid` has died: whatever it was owed is forfeit, and it is paid no
    /// more. Its cash stays with the body.
    pub fn died(&mut self, uid: Uid) {
        let Some(at) = self.find(uid) else { return };
        let account = &mut self.accounts[at];
        if account.is_owed() {
            self.owing -= 1;
        }
        *account = Account { dead: true, shifts: 0, job: None, care_days: 0, ..*account };
    }

    /// `uid` has left the world: its account is closed and what it had goes
    /// to the budget, a debt as much as cash. What that was.
    pub fn close(&mut self, uid: Uid) -> Option<Money> {
        let at = self.find(uid)?;
        let account = self.accounts.remove(at);
        if account.is_owed() {
            self.owing -= 1;
        }
        self.budget += account.cash;
        if let Some(book) = self.book_mut() {
            book.estates += account.cash;
        }
        Some(account.cash)
    }

    /// `uid` worked `shifts` more shifts as `job`, and went `care_days` more
    /// days without one: its own timesheet's say (`Effect::Record`). The
    /// dead, and anybody with no account, are not owed anything.
    pub fn record(&mut self, uid: Uid, shifts: u16, job: Option<JobKind>, care_days: u16) {
        let Some(at) = self.find(uid) else { return };
        let account = &mut self.accounts[at];
        if account.dead {
            return;
        }
        let was_owed = account.is_owed();
        account.shifts = account.shifts.saturating_add(shifts);
        if shifts > 0 && job.is_some() {
            account.job = job;
        }
        account.care_days = account.care_days.saturating_add(care_days);
        if !was_owed && account.is_owed() {
            self.owing += 1;
        }
    }

    /// A clerk, `clerk`, has finished an account: the first one at or after
    /// where the clerks have got to that is owed anything is paid — the wage
    /// times its shifts, less tax, and a care package for each day out of
    /// work — out of the budget, which has the tax back. The round moves past
    /// it. Who was paid, and what.
    pub fn pay_next(&mut self, clerk: Uid, log: &Log) -> Option<(Uid, Money)> {
        let len = self.accounts.len();
        if self.owing == 0 || len == 0 {
            return None;
        }
        let start = self.next.map_or(0, |next| self.accounts.partition_point(|account| account.uid < next));
        let at = (0..len).map(|offset| (start + offset) % len).find(|&at| self.accounts[at].is_owed())?;
        let tariffs = &self.tariffs;
        let account = &mut self.accounts[at];
        let (payee, shifts, job, days) = (account.uid, account.shifts, account.job, account.care_days);
        let gross = job.map_or(Money::ZERO, |job| tariffs.wage(job)) * shifts as i64;
        let tax = gross.percent(tariffs.tax_percent);
        let care = tariffs.care_package * days as i64;
        let paid = gross - tax + care;
        account.cash += paid;
        *account = Account { shifts: 0, job: None, care_days: 0, ..*account };
        self.owing -= 1;
        self.budget = self.budget - gross - care + tax;
        if let Some(book) = self.book_mut() {
            book.wages += gross;
            book.taxes += tax;
            book.care += care;
        }
        self.next = self.accounts.get(at + 1).map(|account| account.uid);

        let mut parts = Vec::with_capacity(2);
        if let Some(job) = job.filter(|_| shifts > 0) {
            let plural = if shifts == 1 { "" } else { "s" };
            parts.push(format!("{shifts} shift{plural} as {}, {gross} less {tax} tax", job.with_article()));
        }
        if days > 0 {
            let plural = if days == 1 { "" } else { "s" };
            parts.push(format!("a care package for {days} day{plural}, {care}"));
        }
        log.related(clerk, payee, format!("{clerk} paid {payee} {paid}: {}", parts.join(", and ")));
        Some((payee, paid))
    }

    /// `uid` took `item` out of a fridge: its price off its cash, debt or
    /// not, and into the budget. What it cost.
    pub fn buy(&mut self, uid: Uid, item: ItemKind) -> Option<Money> {
        let at = self.find(uid)?;
        let price = self.tariffs.price(item);
        self.accounts[at].cash -= price;
        self.budget += price;
        if let Some(book) = self.book_mut() {
            book.sales += price;
        }
        Some(price)
    }

    /// The accountant has closed the books for `day`, and every day before.
    pub fn close_books(&mut self, day: u32) {
        self.closed_through = self.closed_through.max(day);
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Economy {
        let Economy { tariffs, budget, accounts, next, owing: _, books, closed_through } = self;
        pb::Economy {
            budget: budget.0,
            accounts: accounts
                .iter()
                .map(|&Account { uid, cash, shifts, job, care_days, dead }| pb::Account {
                    uid: uid.raw(),
                    cash: cash.0,
                    shifts: shifts as u32,
                    job: job.map(JobKind::number),
                    care_days: care_days as u32,
                    dead,
                })
                .collect(),
            next: next.map(Uid::raw),
            books: books
                .iter()
                .map(|&DayBook { day, opening, wages, taxes, care, sales, estates, grants }| pb::DayBook {
                    day,
                    opening: opening.0,
                    wages: wages.0,
                    taxes: taxes.0,
                    care: care.0,
                    sales: sales.0,
                    estates: estates.0,
                    grants: grants.0,
                })
                .collect(),
            closed_through: *closed_through,
            tariffs: Some(tariffs.save()),
        }
    }

    /// The economy a save kept, for a world whose humans are `humans` —
    /// `(uid, dead)`, by uid. **One account to every human and none to
    /// anybody else**, dead as the human is.
    pub(crate) fn restore(saved: &pb::Economy, humans: &[(Uid, bool)]) -> Result<Economy, Invalid> {
        let tariffs = Tariffs::restore(pb::required(&saved.tariffs, "tariffs")?)?;
        let mut accounts = Vec::with_capacity(saved.accounts.len());
        for account in &saved.accounts {
            let Some(uid) = Uid::from_raw(account.uid) else {
                return invalid(format!("an account for nobody, {:#x}", account.uid));
            };
            let job = match account.job {
                None => None,
                Some(number) => Some(JobKind::from_number(number).ok_or_else(|| Invalid::new(format!("{uid} owed for no job numbered {number}")))?),
            };
            let (Ok(shifts), Ok(care_days)) = (u16::try_from(account.shifts), u16::try_from(account.care_days)) else {
                return invalid(format!("{uid} owed {} shifts and {} days", account.shifts, account.care_days));
            };
            if accounts.last().is_some_and(|last: &Account| last.uid >= uid) {
                return invalid(format!("the accounts out of order at {uid}"));
            }
            if (shifts > 0 && job.is_none()) || (account.dead && (shifts > 0 || care_days > 0)) {
                return invalid(format!("{uid} has an invalid unpaid timesheet"));
            }
            accounts.push(Account { uid, cash: Money(account.cash), shifts, job, care_days, dead: account.dead });
        }
        if accounts.len() != humans.len() {
            return invalid(format!("{} accounts for {} humans", accounts.len(), humans.len()));
        }
        for (account, &(uid, dead)) in accounts.iter().zip(humans) {
            if account.uid != uid {
                return invalid(format!("an account for {}, who is no human here, or none for {uid}", account.uid));
            }
            if account.dead != dead {
                return invalid(format!("{uid}'s account says {} and the body {}", life(account.dead), life(dead)));
            }
        }
        let mut books: Vec<DayBook> = Vec::with_capacity(saved.books.len());
        for book in &saved.books {
            if books.last().is_some_and(|last| last.day >= book.day) {
                return invalid(format!("the books out of order at day {}", book.day));
            }
            let pb::DayBook { day, opening, wages, taxes, care, sales, estates, grants } = *book;
            books.push(DayBook {
                day,
                opening: Money(opening),
                wages: Money(wages),
                taxes: Money(taxes),
                care: Money(care),
                sales: Money(sales),
                estates: Money(estates),
                grants: Money(grants),
            });
        }
        let next = match saved.next {
            None => None,
            Some(raw) => Some(Uid::from_raw(raw).ok_or_else(|| Invalid::new("the payroll's round at nobody"))?),
        };
        let owing = accounts.iter().filter(|account| account.is_owed()).count() as u32;
        Ok(Economy { tariffs, budget: Money(saved.budget), accounts, next, owing, books, closed_through: saved.closed_through })
    }

    /// An economy for a world saved before there was one, whose humans are
    /// `humans` — `(uid, dead)`, by uid: an account each, with cash worked
    /// out from the uid alone (`10 + raw % 81` dollars, so the same save loads
    /// the same), the dead marked so, and the budget granted for every one of
    /// them.
    pub(crate) fn fresh(tariffs: Tariffs, humans: &[(Uid, bool)]) -> Economy {
        let mut economy = Economy::new(tariffs);
        for &(uid, dead) in humans {
            economy.accounts.push(Account { dead, ..Account::new(uid, Money::dollars(10 + (uid.raw() % 81) as i64)) });
        }
        economy.accounts.sort_unstable_by_key(|account| account.uid);
        economy.budget = economy.tariffs.budget_per_arrival * economy.accounts.len() as i64;
        economy
    }
}

fn life(dead: bool) -> &'static str {
    if dead { "dead" } else { "alive" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    fn human(n: u64) -> Uid {
        Uid::new(EntityType::Human, n)
    }

    #[test]
    fn money_adds_up_rounds_half_away_from_zero_and_reads_in_dollars() {
        let a = Money::dollars(12) + Money::cents(34);
        assert_eq!(a, Money(1234));
        assert_eq!(a - Money::dollars(20), Money(-766));
        assert_eq!(-a, Money(-1234));
        assert_eq!(a * 3, Money(3702));
        assert_eq!([a, a, Money(-68)].into_iter().sum::<Money>(), Money(2400));
        assert_eq!(Money::dollars(80).percent(20), Money::dollars(16));
        assert_eq!(Money(250).percent(1), Money(3), "2.5 cents is 3");
        assert_eq!(Money(249).percent(1), Money(2));
        assert_eq!(Money(-250).percent(1), Money(-3), "away from zero");
        assert_eq!(Money(123_456).to_string(), "$1,234.56");
        assert_eq!(Money(-300).to_string(), "-$3.00");
        assert_eq!(Money(5).to_string(), "$0.05");
        assert_eq!(Money::dollars(131_240).to_string(), "$131,240.00");
        assert_eq!(Money::dollars(1_000_000).to_string(), "$1,000,000.00");
        assert_eq!(format!("{:>8}", Money(100)), "   $1.00", "pads like a string");
        assert_eq!((Money(19_200).signed(), Money(-300).signed()), ("+$192.00".to_string(), "-$3.00".to_string()));
    }

    /// `economy.yaml` is the defaults: every job and every item named, at
    /// the wages and prices agreed.
    #[test]
    fn the_economy_built_in_names_every_job_and_item_at_the_agreed_numbers() {
        let tariffs = Tariffs::default();
        assert_eq!(tariffs.tax_percent, 20);
        assert_eq!(tariffs.care_package, Money::dollars(20));
        assert_eq!(tariffs.budget_per_arrival, Money::dollars(1000));
        assert_eq!(tariffs.starting_cash, (Money::dollars(10), Money::dollars(90)));
        assert_eq!(tariffs.minutes_per_account, 5);
        assert_eq!(tariffs.price(ItemKind::Food), Money::dollars(1));
        assert_eq!(tariffs.price(ItemKind::Water), Money::ZERO);
        let mid = [JobKind::OfficeWorker, JobKind::Accountant, JobKind::ScrapCollector, JobKind::Farmer, JobKind::CityDecorator];
        for job in JobKind::ALL {
            let wage = if job == JobKind::King {
                100
            } else if mid.contains(&job) {
                60
            } else {
                40
            };
            assert_eq!(tariffs.wage(job), Money::dollars(wage), "{}", job.name());
        }
    }

    #[test]
    fn a_tariff_file_missing_a_job_or_naming_a_stranger_is_refused() {
        assert!(Tariffs::from_yaml(DEFAULTS_YAML).is_ok());
        let without = DEFAULTS_YAML.replace("  accountant: 60              # mid\n", "");
        assert!(Tariffs::from_yaml(&without).unwrap_err().contains("\"accountant\" is missing"));
        let stranger = DEFAULTS_YAML.replace("  guard: 40", "  guardian: 40");
        assert!(Tariffs::from_yaml(&stranger).is_err());
        let item = DEFAULTS_YAML.replace("  water: 0  ", "  wine: 0  ");
        assert!(Tariffs::from_yaml(&item).unwrap_err().contains("wine"));
        let unknown = format!("{DEFAULTS_YAML}\nbonus: 3\n");
        assert!(Tariffs::from_yaml(&unknown).is_err(), "a key nobody reads");
        let backwards = DEFAULTS_YAML.replace("[10, 90]", "[90, 10]");
        assert!(Tariffs::from_yaml(&backwards).is_err());
        let tax = DEFAULTS_YAML.replace("tax: 20", "tax: 120");
        assert!(Tariffs::from_yaml(&tax).is_err());
        let cheap = DEFAULTS_YAML.replace("food: 1 ", "food: 1.5 ");
        assert_eq!(Tariffs::from_yaml(&cheap).unwrap().price(ItemKind::Food), Money(150));
    }

    #[test]
    fn a_tariff_set_stays_in_bounds_and_says_what_it_is_now() {
        let mut tariffs = Tariffs::default();
        assert_eq!(tariffs.set(Tariff::Wage(JobKind::Guard), 4500), 4500);
        assert_eq!(tariffs.said(Tariff::Wage(JobKind::Guard)), "the wage of a guard is now $45.00 a shift");
        assert_eq!(tariffs.set(Tariff::Tax, 140), 100);
        assert_eq!(tariffs.set(Tariff::Price(ItemKind::Food), -50), 0, "nothing below zero");
        assert_eq!(tariffs.set(Tariff::CashMin, 20_000), 9000, "the lowest no more than the highest");
        assert_eq!(tariffs.set(Tariff::CashMax, 100), 9000, "and the highest no less than the lowest");
        assert_eq!(tariffs.set(Tariff::MinutesPerAccount, 0), 1);
        assert_eq!(tariffs.value_text(Tariff::Tax), "100%");
        assert_eq!(tariffs.said(Tariff::Price(ItemKind::Food)), "the price of food is now $0.00");
        let labels: Vec<&str> = Tariff::every().into_iter().map(Tariff::label).collect();
        for (i, label) in labels.iter().enumerate() {
            assert!(!labels[..i].contains(label), "{label} twice");
        }
    }

    #[test]
    fn an_arrival_brings_a_grant_and_leaves_its_estate_debt_and_all() {
        let mut economy = Economy::default();
        economy.roll(1);
        economy.open(human(1), Money::dollars(42));
        economy.open(human(2), Money::dollars(10));
        assert_eq!(economy.budget(), Money::dollars(2000));
        assert_eq!(economy.today().unwrap().grants, Money::dollars(2000));
        assert_eq!(economy.today().unwrap().balance(), Money::ZERO, "a grant is not earned");
        for _ in 0..12 {
            economy.buy(human(2), ItemKind::Food);
        }
        assert_eq!(economy.account(human(2)).unwrap().cash, Money::dollars(-2), "into debt, no limit");
        assert_eq!(economy.budget(), Money::dollars(2012));
        assert_eq!(economy.close(human(2)), Some(Money::dollars(-2)));
        assert_eq!(economy.budget(), Money::dollars(2010), "the debt is the budget's loss");
        assert_eq!(economy.today().unwrap().estates, Money::dollars(-2));
        assert_eq!(economy.today().unwrap().balance(), Money::dollars(10));
        assert!(economy.account(human(2)).is_none());
        economy.buy(human(1), ItemKind::Water);
        assert_eq!(economy.account(human(1)).unwrap().cash, Money::dollars(42), "water is free");
    }

    #[test]
    fn a_recorded_shift_is_paid_its_wage_less_tax_out_of_the_budget() {
        let mut economy = Economy::default();
        let log = Log::new();
        economy.roll(1);
        economy.open(human(1), Money::dollars(50));
        economy.record(human(1), 2, Some(JobKind::Guard), 1);
        assert_eq!(economy.owing(), 1);
        let (payee, paid) = economy.pay_next(human(9), &log).unwrap();
        assert_eq!(payee, human(1));
        assert_eq!(paid, Money::dollars(64 + 20), "$80 less $16 tax, and $20 care");
        let account = economy.account(human(1)).unwrap();
        assert_eq!(account.cash, Money::dollars(134));
        assert_eq!((account.shifts, account.job, account.care_days), (0, None, 0), "paid off");
        assert_eq!(economy.owing(), 0);
        assert_eq!(economy.budget(), Money::dollars(1000 - 80 - 20 + 16));
        let book = economy.today().unwrap();
        assert_eq!((book.wages, book.taxes, book.care), (Money::dollars(80), Money::dollars(16), Money::dollars(20)));
        assert_eq!(book.balance(), Money::dollars(-84));
        let lines = log.drain();
        assert_eq!(
            lines,
            [format!("{} paid {} $84.00: 2 shifts as a guard, $80.00 less $16.00 tax, and a care package for 1 day, $20.00", human(9), human(1))]
        );
        assert!(economy.pay_next(human(9), &log).is_none(), "nobody owed");
    }

    #[test]
    fn the_clerks_go_round_the_accounts_in_turn_and_pass_over_the_dead() {
        let mut economy = Economy::default();
        let log = Log::new();
        economy.roll(1);
        for n in 1..=4 {
            economy.open(human(n), Money::ZERO);
            economy.record(human(n), 1, Some(JobKind::OfficeWorker), 0);
        }
        economy.died(human(2));
        assert_eq!(economy.owing(), 3, "the dead are owed nothing");
        assert_eq!(economy.pay_next(human(9), &log).map(|(uid, _)| uid), Some(human(1)));
        economy.record(human(1), 1, Some(JobKind::OfficeWorker), 0);
        assert_eq!(economy.pay_next(human(9), &log).map(|(uid, _)| uid), Some(human(3)), "past the dead");
        assert_eq!(economy.pay_next(human(9), &log).map(|(uid, _)| uid), Some(human(4)));
        assert_eq!(economy.pay_next(human(9), &log).map(|(uid, _)| uid), Some(human(1)), "and round again");
        assert_eq!(economy.account(human(2)).unwrap().cash, Money::ZERO);
        economy.record(human(2), 3, Some(JobKind::Guard), 2);
        assert_eq!(economy.owing(), 0, "nor recorded for");
    }

    #[test]
    fn the_books_open_a_day_at_a_time_and_the_accountant_closes_a_week_back_at_most() {
        let mut economy = Economy::default();
        economy.roll(1);
        economy.open(human(1), Money::dollars(5));
        economy.roll(1);
        assert_eq!(economy.books().len(), 1, "once a day");
        economy.buy(human(1), ItemKind::Food);
        economy.roll(2);
        assert_eq!(economy.book(2).unwrap().opening, Money::dollars(1001));
        assert_eq!(economy.unclosed_day(2), Some(1));
        economy.close_books(1);
        assert_eq!(economy.unclosed_day(2), None);
        for day in 3..=12 {
            economy.roll(day);
        }
        assert_eq!(economy.unclosed_day(12), Some(5), "seven days back, no further");
        economy.close_books(9);
        assert_eq!(economy.unclosed_day(12), Some(10));
        for day in 13..=60 {
            economy.roll(day);
        }
        assert_eq!(economy.books().len(), BOOKS_KEPT);
        assert_eq!(economy.books()[0].weekday(), Weekday::ALL[(33 - 1) % 7]);
    }

    #[test]
    fn an_economy_comes_back_from_a_save_as_it_was_and_one_for_strangers_is_refused() {
        let mut economy = Economy::default();
        economy.roll(3);
        economy.set_tariff(Tariff::Price(ItemKind::Food), 150);
        for n in 1..=3 {
            economy.open(human(n), Money::dollars(n as i64));
        }
        economy.record(human(3), 2, Some(JobKind::Farmer), 0);
        economy.record(human(1), 0, None, 4);
        economy.died(human(2));
        economy.pay_next(human(1), &Log::new());
        economy.close_books(2);
        let humans = [(human(1), false), (human(2), true), (human(3), false)];
        let saved = economy.save();
        assert_eq!(Economy::restore(&saved, &humans), Ok(economy.clone()));

        assert!(Economy::restore(&saved, &humans[..2]).is_err(), "an account for nobody here");
        let more = [(human(1), false), (human(2), true), (human(3), false), (human(4), false)];
        assert!(Economy::restore(&saved, &more).is_err(), "a human without one");
        let alive = [(human(1), false), (human(2), false), (human(3), false)];
        assert!(Economy::restore(&saved, &alive).is_err(), "dead in the books, alive in the world");
        let mut stranger = saved.clone();
        stranger.tariffs.as_mut().unwrap().wages[0].name = "jester".to_string();
        assert!(Economy::restore(&stranger, &humans).is_err());
        let mut older = saved.clone();
        older.tariffs.as_mut().unwrap().wages.pop();
        let restored = Economy::restore(&older, &humans).unwrap();
        assert_eq!(restored.tariffs().wage(JobKind::Accountant), Money::dollars(60), "a job added since is at its default");
    }

    #[test]
    fn an_economy_for_a_save_from_before_money_opens_an_account_for_every_human() {
        let humans = [(human(5), false), (human(90), true)];
        let economy = Economy::fresh(Tariffs::default(), &humans);
        assert_eq!(economy.accounts().len(), 2);
        assert_eq!(economy.budget(), Money::dollars(2000));
        let cash = |uid: Uid| Money::dollars(10 + (uid.raw() % 81) as i64);
        assert_eq!(economy.account(human(5)).unwrap().cash, cash(human(5)));
        assert!(economy.account(human(90)).unwrap().dead);
        assert_eq!(economy, Economy::fresh(Tariffs::default(), &humans), "the same save, the same accounts");
    }
}
