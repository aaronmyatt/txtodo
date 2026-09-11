//! One module per todo.sh command family. Pure line logic lives beside its unit tests; the `run`
//! functions do the file I/O and printing.

pub mod add;
pub mod archive;
pub mod edit;
pub mod fileops;
pub mod list;
pub mod text;
