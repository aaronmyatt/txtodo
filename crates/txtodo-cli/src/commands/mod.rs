//! One module per todo.sh command family. Pure line logic lives beside its unit tests; the `run`
//! functions do the file I/O and printing.

pub mod add;
pub mod archive;
pub mod conflicts;
pub mod device;
pub mod doctor;
mod doctor_bind;
mod doctor_clock;
mod doctor_overlap;
mod doctor_sync;
mod doctor_transport;
mod doctor_version;
pub mod edit;
pub mod env;
pub mod fileops;
pub mod history;
pub mod hygiene;
pub mod identity;
pub mod layout;
pub mod list;
pub mod lists;
mod lists_import;
pub mod mcp;
pub mod merge_driver;
pub mod move_copy;
pub mod notices;
pub mod pair;
pub mod refdir;
pub mod service;
pub mod skill;
pub mod sub_slug;
pub mod text;
pub mod upgrade;
pub mod workspace;
pub mod workspace_bind;
pub mod workspace_offers;
pub mod workspace_rejoin;
