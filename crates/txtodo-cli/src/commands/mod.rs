//! One module per todo.sh command family. Pure line logic lives beside its unit tests; the `run`
//! functions do the file I/O and printing.

pub mod add;
pub mod archive;
pub mod conflicts;
pub mod doctor;
pub mod edit;
pub mod fileops;
pub mod history;
pub mod hygiene;
pub mod list;
pub mod mcp;
pub mod service;
pub mod text;
