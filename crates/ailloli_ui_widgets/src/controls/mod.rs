//! Interactive controls implementing the runtime `Widget` trait.

pub mod accordion;
pub mod avatar;
pub mod badge;
pub mod button;
pub mod card;
pub mod charts;
pub mod chat_widget;
pub mod checkbox;
pub mod combo_box;
pub mod command_palette;
pub mod context_menu;
pub mod dialog;
pub mod disclosure_row;
pub mod divider;
pub mod icon_button;
pub mod link;
pub mod list;
pub mod list_view;
pub mod navigation;
pub mod pickers;
pub(crate) mod popup;
pub mod progress;
pub mod radio;
pub mod segmented;
pub mod select;
pub mod slider;
pub mod status_indicator;
pub mod switch;
pub mod table_view;
pub mod tabs;
pub mod terminal;
pub mod terminal_widget;
pub(crate) mod text_field_core;
pub mod text_input;
pub mod toast;
pub mod tree_view;
pub mod upload_dropzone;

pub use accordion::{Accordion, AccordionItem, AccordionMode, AccordionSize, AccordionStyle};
pub use avatar::{Avatar, AvatarStyle, AvatarTone};
pub use badge::{Badge, BadgeStyle, BadgeTone, BadgeVariant, Chip, Tag};
pub use button::{Button, ButtonStyle, ButtonVariant};
pub use card::{Card, CardStyle, CardVariant};
pub use charts::{BarChart, ChartSize, ChartStyle, ChartTone, LineChart, RadialGauge};
pub use chat_widget::{
    ChatComposerControls, ChatComposerOption, ChatMessageRenderer, ChatWidget, ChatWidgetAction,
    ChatWidgetActionHandler, ChatWidgetCopyHandler, ChatWidgetRetryHandler, ChatWidgetSendHandler,
    ChatWidgetStyle,
};
pub use checkbox::{draw_checkbox, CheckboxStyle};
pub use combo_box::{
    Autocomplete, AutocompleteItem, AutocompleteSize, AutocompleteStyle, ComboBox, ComboBoxOption,
    ComboBoxSize, ComboBoxStyle,
};
pub use command_palette::{CommandItem, CommandPalette, CommandPaletteSize, CommandPaletteStyle};
pub use context_menu::{ContextMenu, ContextMenuEntry, ContextMenuItem, ContextMenuStyle};
pub use dialog::{Dialog, DialogStyle, DialogTone};
pub use disclosure_row::{DisclosureRow, DisclosureRowStyle, DisclosureRowVariant};
pub use divider::{Divider, DividerOrientation, DividerStyle, DividerVariant};
#[allow(deprecated)]
pub use icon_button::{draw_icon_button, IconButtonStyle};
pub use link::{Link, LinkStyle};
pub use list_view::{ListItem, ListItemStyle, ListItemVariant, ListView, ListViewStyle};
pub use navigation::{NavItem, NavItemStyle, NavItemVariant, Sidebar, SidebarStyle};
pub use pickers::{
    ColorPicker, ColorPickerSize, ColorPickerStyle, DatePicker, DatePickerSize, DatePickerStyle,
    TimePicker, TimePickerSize, TimePickerStyle,
};
pub use popup::PopupPlacement;
pub use progress::{CircularProgress, ProgressBar, ProgressSize, ProgressStyle, ProgressVariant};
pub use radio::{RadioButton, RadioDirection, RadioGroup, RadioOption, RadioSize, RadioStyle};
pub use segmented::{SegmentedControl, SegmentedOption, SegmentedSize, SegmentedStyle};
pub use select::{
    Dropdown, DropdownItem, DropdownSize, DropdownStyle, Select, SelectOption, SelectSize,
    SelectStyle,
};
pub use slider::{RangeSlider, Slider, SliderOrientation, SliderSize, SliderStyle};
pub use status_indicator::{StatusIndicator, StatusStyle, StatusTone, StatusVariant};
pub use switch::{Switch, SwitchOrientation, SwitchSize, SwitchStyle};
pub use table_view::{
    TableAlign, TableCell, TableCellKind, TableColumn, TableColumnWidth, TableRow, TableView,
    TableViewSize, TableViewStyle,
};
pub use tabs::{draw_tabs_bar_with_options, TabsBarOptions};
pub use terminal::{
    terminal_search_matches, TerminalEvent, TerminalEventBuffer, TerminalEventKind,
    TerminalEventSource, TerminalLine, TerminalLineAttrs, TerminalLineKind, TerminalPosition,
    TerminalSearchMatch, TerminalSelection, TerminalView, TerminalViewStyle,
};
pub use terminal_widget::{
    terminal_key_bytes, terminal_key_bytes_with_modes, terminal_selection_text, Terminal,
    TerminalCellMetrics, TerminalGeometry, TerminalSelectionMode, TerminalViewportSize,
    TerminalWidgetStyle,
};
pub use text_input::{TextInput, TextInputStyle};
pub use toast::{Toast, ToastHost, ToastPosition, ToastStyle, ToastTone};
pub use tree_view::{
    TreeContextMenu, TreeCreate, TreeCreateKind, TreeCreateRequest, TreeDelete, TreeDropPosition,
    TreeMove, TreeMutationMode, TreeNode, TreeNodeTrailingAction, TreeRename, TreeShortcut,
    TreeView, TreeViewCommand, TreeViewSize, TreeViewStyle,
};
pub use upload_dropzone::{UploadDropzone, UploadDropzoneStyle, UploadDropzoneVariant};
