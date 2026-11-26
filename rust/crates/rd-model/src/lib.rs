//! Data types shared by every ReleaseDock crate. Field names, enum raw values and
//! encodings match the Swift app's JSON in SQLite exactly, so both apps read the same
//! database: camelCase keys, dates as seconds since 2001-01-01 ([`SwiftDate`]),
//! uppercase UUID strings ([`Id`]), and absent optionals omitted.

pub mod inspection;
pub mod iso8601;
pub mod product;
pub mod svn_status;
pub mod swift_json;
pub mod time;
pub mod validation;

pub use inspection::*;
pub use product::*;
pub use svn_status::SvnItemStatus;
pub use time::{Id, SwiftDate};
pub use validation::*;
