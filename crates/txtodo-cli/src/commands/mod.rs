//! One module per todo.sh command family. Pure line logic lives beside its unit tests; the `run`
//! functions do the file I/O and printing.

pub mod add;
pub mod archive;
pub mod conflicts;
pub mod device;
pub mod doctor;
pub mod edit;
pub mod env;
pub mod fileops;
pub mod history;
pub mod hygiene;
pub mod list;
pub mod service;
pub mod text;
