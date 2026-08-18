//! Text display and low-level editable line drawing helpers.

pub mod editable_text;
pub mod label;
pub mod rich_text;

pub use ailloli_ui_text::WrapMode;
pub use editable_text::{draw_editable_mono_line, EditableTextStyle};
pub use label::Text;
pub use rich_text::{draw_rich_text, RichText, TextSpan};
