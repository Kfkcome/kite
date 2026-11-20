//! GPUI terminal view driven by an arbitrary byte stream (an SSH channel; there
//! is no local PTY). `Terminal` is the model, `TerminalView` the interactive
//! view, `TerminalTheme` the palette.

mod box_drawing;
mod config;
mod element;
mod links;
mod local_pty;
mod mappings;
mod row_cache;
mod search;
mod snapshot;
mod terminal;
mod view;

pub use config::TerminalTheme;
pub use config::{CursorStyleSetting, TerminalOptions};
pub use links::Link;
pub use local_pty::LocalPty;
pub use search::SearchMark;
pub use snapshot::{CellColor, CellSnapshot, CursorSnapshot};
pub use terminal::{SCROLLBACK_LINES, Terminal, TerminalEvent};
pub use view::{TerminalView, paste_bytes};
