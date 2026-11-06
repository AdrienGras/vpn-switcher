//! Library target of `vpn-switcher`, the crate behind the `vpns` binary.
//!
//! This library is an implementation detail of the `vpns` binary. It has no
//! semver guarantee and its API may change in any release. For usage, see the
//! README: <https://github.com/AdrienGras/vpn-switcher#readme>.

#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod detect;
#[doc(hidden)]
pub mod interrupt;
#[doc(hidden)]
pub mod runner;
#[doc(hidden)]
pub mod state;
#[doc(hidden)]
pub mod switch;
#[doc(hidden)]
pub mod ui;
