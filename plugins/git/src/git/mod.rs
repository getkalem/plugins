//! Git's side: the command lines the plugin runs and the readers of what
//! git answers. Nothing here starts a program or holds state.

pub mod blame;
pub mod cmd;
pub mod diff;
pub mod log;
pub mod refs;
pub mod status;
