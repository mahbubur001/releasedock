//! Data types shared by every ReleaseDock crate. Field names, enum raw values and
//! encodings match the Swift app's JSON in SQLite exactly, so both apps read the same
//! database: camelCase keys, dates as seconds since 2001-01-01 ([`SwiftDate`]),
//! uppercase UUID strings ([`Id`]), and absent optionals omitted.

pub mod inspection;
pub mod product;
pub mod time;
pub mod validation;

pub use inspection::*;
pub use product::*;
pub use time::{Id, SwiftDate};
pub use validation::*;
