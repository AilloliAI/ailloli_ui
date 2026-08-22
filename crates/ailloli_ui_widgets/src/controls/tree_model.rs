//! Stable-ID retained tree model with atomic mutations and a persistent flat index.
//!
//! Removed IDs are retired permanently, batches commit all-or-nothing, and each
//! nonempty successful batch advances a checked `u64` revision exactly once.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::rc::{Rc, Weak};

use ailloli_ui_core::{Color, IconId};

use super::tree_view::TreeNodeTrailingAction;

/// Stable retained item stored by [`TreeModel`].
///
/// Branch/leaf identity belongs to the item rather than being inferred from
/// children. Builder methods replace optional presentation metadata.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeItem;
/// let item = TreeItem::branch(1_u64, "src").disabled(true);
/// assert!(item.is_branch());
/// assert!(item.is_disabled());
/// ```
#[derive(Clone, PartialEq)]
pub struct TreeItem<T> {
    /// Stable identifier, unique across active and retired IDs.
    id: T,
    /// Display label stored unchanged.
    label: String,
    /// Whether this item can own children and expansion state.
    branch: bool,
    /// Whether selection/navigation should skip the item.
    disabled: bool,
    /// Optional leading icon.
    leading_icon: Option<IconId>,
    /// Optional leading-icon tint override.
    leading_icon_tint: Option<Color>,
    /// Optional trailing action metadata.
    trailing_action: Option<TreeNodeTrailingAction>,
    /// Whether the item represents provisional UI state.
    transient: bool,
}

impl<T: fmt::Debug> fmt::Debug for TreeItem<T> {
    /// Formats core identity/state while deliberately omitting tint/action details.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeItem")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("branch", &self.branch)
            .field("disabled", &self.disabled)
            .field("leading_icon", &self.leading_icon)
            .field("transient", &self.transient)
            .finish_non_exhaustive()
    }
}

impl<T> TreeItem<T> {
    /// Creates an enabled branch with no presentation metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// let item = TreeItem::branch("root", "Root");
    /// assert!(item.is_branch());
    /// ```
    pub fn branch(id: T, label: impl Into<String>) -> Self {
        Self::new(id, label, true)
    }

    /// Creates an enabled leaf with no presentation metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// let item = TreeItem::leaf(7, "README.md");
    /// assert!(!item.is_branch());
    /// ```
    pub fn leaf(id: T, label: impl Into<String>) -> Self {
        Self::new(id, label, false)
    }

    /// Creates an item with the supplied branch flag and default metadata.
    fn new(id: T, label: impl Into<String>, branch: bool) -> Self {
        Self {
            id,
            label: label.into(),
            branch,
            disabled: false,
            leading_icon: None,
            leading_icon_tint: None,
            trailing_action: None,
            transient: false,
        }
    }

    /// Sets whether the item is unavailable to tree interaction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(TreeItem::leaf(1, "locked").disabled(true).is_disabled());
    /// ```
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the leading icon, replacing any previous icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// let item = TreeItem::leaf(1, "file").leading_icon(IconId::History);
    /// assert_eq!(item.leading_icon_ref(), Some(&IconId::History));
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Sets the leading-icon tint override.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// let item = TreeItem::leaf(1, "file").leading_icon_tint(Color::WHITE);
    /// assert_eq!(item.leading_icon_tint_ref(), Some(Color::WHITE));
    /// ```
    pub fn leading_icon_tint(mut self, tint: Color) -> Self {
        self.leading_icon_tint = Some(tint);
        self
    }

    /// Sets trailing action metadata, replacing any previous action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeNodeTrailingAction};
    /// let item = TreeItem::leaf(1, "file").trailing_action(TreeNodeTrailingAction::new(IconId::Close));
    /// assert!(item.trailing_action_ref().is_some());
    /// ```
    pub fn trailing_action(mut self, action: TreeNodeTrailingAction) -> Self {
        self.trailing_action = Some(action);
        self
    }

    /// Marks or unmarks provisional presentation state.
    ///
    /// The model does not otherwise treat transient items specially.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(TreeItem::leaf(1, "draft").transient(true).is_transient());
    /// ```
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Borrows the stable item identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert_eq!(TreeItem::leaf(42, "answer").id(), &42);
    /// ```
    pub fn id(&self) -> &T {
        &self.id
    }

    /// Borrows the stored display label.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert_eq!(TreeItem::leaf(1, "README").label(), "README");
    /// ```
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Reports whether the item can own children and expansion state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(TreeItem::branch(1, "dir").is_branch());
    /// ```
    pub const fn is_branch(&self) -> bool {
        self.branch
    }

    /// Reports whether tree interaction should skip this item.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(!TreeItem::leaf(1, "file").is_disabled());
    /// ```
    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Borrows the optional leading icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(TreeItem::leaf(1, "file").leading_icon_ref().is_none());
    /// ```
    pub fn leading_icon_ref(&self) -> Option<&IconId> {
        self.leading_icon.as_ref()
    }

    /// Returns the optional leading-icon tint by value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert_eq!(TreeItem::leaf(1, "file").leading_icon_tint_ref(), None);
    /// ```
    pub const fn leading_icon_tint_ref(&self) -> Option<Color> {
        self.leading_icon_tint
    }

    /// Borrows optional trailing action metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(TreeItem::leaf(1, "file").trailing_action_ref().is_none());
    /// ```
    pub fn trailing_action_ref(&self) -> Option<&TreeNodeTrailingAction> {
        self.trailing_action.as_ref()
    }

    /// Reports whether this is provisional presentation state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeItem;
    /// assert!(!TreeItem::leaf(1, "file").is_transient());
    /// ```
    pub const fn is_transient(&self) -> bool {
        self.transient
    }
}

/// Atomic retained-tree mutation.
///
/// Indices are zero-based insertion positions in the selected root/child list.
/// A batch validates against a clone and commits all mutations or none.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeItem, TreeMutation};
/// let mutation = TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") };
/// assert!(matches!(mutation, TreeMutation::Insert { index: 0, .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum TreeMutation<T> {
    /// Inserts a new, never-used ID at a root or child position.
    Insert {
        /// Parent branch ID, or `None` for a root.
        parent: Option<T>,
        /// Insertion position in `0..=sibling_count`.
        index: usize,
        /// New item; its children start empty and expansion starts false.
        item: TreeItem<T>,
    },
    /// Removes the identified item and its entire descendant subtree.
    Remove {
        /// Existing root or descendant ID to retire.
        id: T,
    },
    /// Replaces item metadata while preserving hierarchy and expansion.
    Update {
        /// Replacement item carrying an existing ID.
        item: TreeItem<T>,
    },
    /// Moves an existing subtree to a root or branch position.
    Move {
        /// Existing subtree root ID.
        id: T,
        /// New parent branch, or `None` for roots.
        new_parent: Option<T>,
        /// Final position in the target sibling list.
        index: usize,
    },
    /// Changes expansion on an existing branch.
    SetExpanded {
        /// Existing branch ID.
        id: T,
        /// New expansion state.
        expanded: bool,
    },
}

/// One successfully committed atomic batch.
///
/// Empty batches return the current revision and no mutations without advancing
/// the model. Nonempty batches advance exactly once.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
/// let mut model = TreeModel::new();
/// let delta = model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
/// assert_eq!(delta.revision(), 1);
/// assert_eq!(delta.mutations().len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TreeModelDelta<T> {
    /// Revision after the batch committed.
    revision: u64,
    /// Exact input mutations in application order.
    mutations: Vec<TreeMutation<T>>,
}

impl<T> TreeModelDelta<T> {
    /// Returns the model revision after commit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeMutation};
    /// let mut model = TreeModel::<u8>::new();
    /// assert_eq!(model.apply_batch(Vec::<TreeMutation<u8>>::new()).unwrap().revision(), 0);
    /// ```
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Borrows committed mutations in input order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeMutation};
    /// let mut model = TreeModel::<u8>::new();
    /// let delta = model.apply_batch(Vec::<TreeMutation<u8>>::new()).unwrap();
    /// assert!(delta.mutations().is_empty());
    /// ```
    pub fn mutations(&self) -> &[TreeMutation<T>] {
        &self.mutations
    }
}

/// Validation failure for a retained tree mutation. The whole batch is rolled
/// back on error.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TreeModelError;
/// let error = TreeModelError::DuplicateId { id: 7_u64 };
/// assert!(error.to_string().contains("already exists"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeModelError<T: fmt::Debug> {
    /// An inserted ID is already active.
    #[error("tree item already exists: {id:?}")]
    DuplicateId {
        /// Active identifier that the insertion attempted to duplicate.
        id: T,
    },
    /// A referenced item or parent does not exist.
    #[error("tree item does not exist: {id:?}")]
    MissingId {
        /// Identifier that could not be resolved in the active model.
        id: T,
    },
    /// An inserted ID was previously removed and permanently retired.
    #[error("tree item identifier was retired and cannot be reused: {id:?}")]
    ReusedId {
        /// Tombstoned identifier that must not be inserted again.
        id: T,
    },
    /// An insertion/move target cannot own children.
    #[error("parent is not a branch: {id:?}")]
    ParentIsLeaf {
        /// Leaf identifier supplied as the requested parent.
        id: T,
    },
    /// An insertion/move position exceeds its allowed inclusive upper bound.
    #[error("child index {index} is outside 0..={len}")]
    InvalidIndex {
        /// Requested zero-based insertion position.
        index: usize,
        /// Current child count and inclusive append position.
        len: usize,
    },
    /// A move would make an item its own ancestor.
    #[error("moving {id:?} below {new_parent:?} would create a cycle")]
    Cycle {
        /// Identifier of the item being moved.
        id: T,
        /// Descendant identifier proposed as the new parent.
        new_parent: T,
    },
    /// An update attempted to turn a populated branch into a leaf.
    #[error("a branch with children cannot become a leaf: {id:?}")]
    NonEmptyBranchToLeaf {
        /// Populated branch identifier whose kind change was rejected.
        id: T,
    },
    /// Expansion was requested for a leaf.
    #[error("only branch items can be expanded: {id:?}")]
    NotBranch {
        /// Leaf identifier supplied to a branch-only operation.
        id: T,
    },
    /// Incrementing the checked `u64` revision would overflow.
    #[error("tree model revision space is exhausted")]
    RevisionExhausted,
    /// Optimistic mutation expected a different current revision.
    #[error("stale tree mutation: expected revision {expected}, current revision {actual}")]
    StaleRevision {
        /// Revision required by the optimistic mutation.
        expected: u64,
        /// Revision held by the model when validation ran.
        actual: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One visible depth-first row in a [`FlatTreeIndex`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
/// let mut model = TreeModel::new();
/// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
/// let row = &model.flat_index().rows()[0];
/// assert_eq!((row.node_id(), row.depth()), (&1, 0));
/// ```
pub struct FlatTreeRow<T> {
    /// Stable retained item ID.
    node_id: T,
    /// Visible depth, saturating at `u16::MAX`.
    depth: u16,
}

impl<T> FlatTreeRow<T> {
    /// Borrows the row's stable item ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf("a", "A") }).unwrap();
    /// assert_eq!(model.flat_index().rows()[0].node_id(), &"a");
    /// ```
    pub fn node_id(&self) -> &T {
        &self.node_id
    }

    /// Returns the saturated visible hierarchy depth.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "A") }).unwrap();
    /// assert_eq!(model.flat_index().rows()[0].depth(), 0);
    /// ```
    pub const fn depth(&self) -> u16 {
        self.depth
    }
}

/// Persistent visible-row index. It changes only when a model mutation is
/// committed, never during layout, paint, hit-test, or a pure scroll.
///
/// Rows are depth-first in root/child order and include descendants only below
/// expanded branches.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
/// let mut model = TreeModel::new();
/// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
/// assert_eq!(model.flat_index().rows().len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct FlatTreeIndex<T> {
    /// Visible rows in depth-first presentation order.
    rows: Vec<FlatTreeRow<T>>,
    /// Constant-time visible ID-to-row lookup.
    row_by_id: HashMap<T, usize>,
    /// Model revision represented by this index.
    revision: u64,
    /// Saturating count of full index rebuilds.
    rebuilds: u64,
    /// First visible non-disabled row, if one exists.
    first_enabled_row: Option<usize>,
}

impl<T> Default for FlatTreeIndex<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            row_by_id: HashMap::new(),
            revision: 0,
            rebuilds: 0,
            first_enabled_row: None,
        }
    }
}

impl<T: Eq + Hash> FlatTreeIndex<T> {
    /// Borrows all visible rows in depth-first presentation order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert!(TreeModel::<u8>::new().flat_index().rows().is_empty());
    /// ```
    pub fn rows(&self) -> &[FlatTreeRow<T>] {
        &self.rows
    }

    /// Returns an item's visible zero-based row, or `None` when hidden/missing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(9, "file") }).unwrap();
    /// assert_eq!(model.flat_index().row_of(&9), Some(0));
    /// assert_eq!(model.flat_index().row_of(&8), None);
    /// ```
    pub fn row_of(&self, id: &T) -> Option<usize> {
        self.row_by_id.get(id).copied()
    }

    /// Returns the model revision represented by the flat index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert_eq!(TreeModel::<u8>::new().flat_index().revision(), 0);
    /// ```
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the saturating count of full index materializations.
    ///
    /// Incremental batches normally splice rows and leave this unchanged; batches
    /// of at least 1,024 mutations rebuild once at commit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert_eq!(TreeModel::<u8>::new().flat_index().rebuilds(), 0);
    /// ```
    pub const fn rebuilds(&self) -> u64 {
        self.rebuilds
    }

    /// Returns the first visible row whose item is not disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "off").disabled(true) }).unwrap();
    /// assert_eq!(model.flat_index().first_enabled_row(), None);
    /// ```
    pub const fn first_enabled_row(&self) -> Option<usize> {
        self.first_enabled_row
    }
}

#[derive(Debug, Clone)]
/// Internal hierarchy record preserving parent, ordered children, and expansion.
struct TreeRecord<T> {
    /// Public item metadata.
    item: TreeItem<T>,
    /// Parent ID, or `None` for a root.
    parent: Option<T>,
    /// Child IDs in presentation order.
    children: Vec<T>,
    /// Whether visible descendants are materialized in the flat index.
    expanded: bool,
}

/// Retained hierarchical model with stable identifiers and an incremental
/// presentation revision.
///
/// IDs must remain globally unique for the lifetime of a model: removal retires
/// an ID and its descendants. Mutations are atomic, and visible rows are cached
/// persistently for cheap layout/paint access.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
/// let mut model = TreeModel::new();
/// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::branch(1, "root") }).unwrap();
/// assert_eq!((model.len(), model.visible_len(), model.revision()), (1, 1, 1));
/// ```
#[derive(Debug, Clone)]
pub struct TreeModel<T> {
    /// Active records indexed by stable ID.
    nodes: HashMap<T, TreeRecord<T>>,
    /// Root IDs in presentation order.
    roots: Vec<T>,
    /// Permanently unavailable IDs removed from this model.
    retired_ids: HashSet<T>,
    /// Persistent visible-row index.
    flat: FlatTreeIndex<T>,
    /// Checked transaction revision.
    revision: u64,
}

impl<T> Default for TreeModel<T> {
    fn default() -> Self {
        Self {
            nodes: HashMap::new(),
            roots: Vec::new(),
            retired_ids: HashSet::new(),
            flat: FlatTreeIndex::default(),
            revision: 0,
        }
    }
}

impl<T> TreeModel<T>
where
    T: Clone + Eq + Hash + fmt::Debug,
{
    /// Creates an empty revision-zero model.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// let model = TreeModel::<u64>::new();
    /// assert!(model.is_empty());
    /// assert_eq!(model.revision(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current transaction revision.
    ///
    /// Successful nonempty batches increment once; empty batches and failed
    /// batches leave it unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert_eq!(TreeModel::<u8>::new().revision(), 0);
    /// ```
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns active item count, including currently hidden descendants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert_eq!(TreeModel::<u8>::new().len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Reports whether the model contains no active items.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert!(TreeModel::<u8>::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns cached visible row count under current expansion state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
    /// assert_eq!(model.visible_len(), 1);
    /// ```
    pub fn visible_len(&self) -> usize {
        self.flat.rows.len()
    }

    /// Borrows the persistent visible-row index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TreeModel;
    /// assert_eq!(TreeModel::<u8>::new().flat_index().revision(), 0);
    /// ```
    pub fn flat_index(&self) -> &FlatTreeIndex<T> {
        &self.flat
    }

    /// Borrows active item metadata by ID.
    ///
    /// Returns `None` for missing and retired IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
    /// assert_eq!(model.item(&1).unwrap().label(), "file");
    /// ```
    pub fn item(&self, id: &T) -> Option<&TreeItem<T>> {
        self.nodes.get(id).map(|record| &record.item)
    }

    /// Borrows an active item's parent ID.
    ///
    /// Returns `None` both for roots and missing IDs; use [`Self::item`] when the
    /// distinction matters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply_batch([
    ///   TreeMutation::Insert { parent: None, index: 0, item: TreeItem::branch(1, "root") },
    ///   TreeMutation::Insert { parent: Some(1), index: 0, item: TreeItem::leaf(2, "file") },
    /// ]).unwrap();
    /// assert_eq!(model.parent(&2), Some(&1));
    /// ```
    pub fn parent(&self, id: &T) -> Option<&T> {
        self.nodes.get(id).and_then(|record| record.parent.as_ref())
    }

    /// Borrows an active item's child IDs in presentation order.
    ///
    /// Leaves return `Some(empty)`; missing IDs return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
    /// assert_eq!(model.children(&1), Some([].as_slice()));
    /// ```
    pub fn children(&self, id: &T) -> Option<&[T]> {
        self.nodes.get(id).map(|record| record.children.as_slice())
    }

    /// Borrows root IDs in presentation order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(7, "root") }).unwrap();
    /// assert_eq!(model.roots(), &[7]);
    /// ```
    pub fn roots(&self) -> &[T] {
        &self.roots
    }

    /// Reports expansion for an active branch.
    ///
    /// Missing IDs and leaves return `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::branch(1, "root") }).unwrap();
    /// assert!(!model.is_expanded(&1));
    /// ```
    pub fn is_expanded(&self, id: &T) -> bool {
        self.nodes.get(id).is_some_and(|record| record.expanded)
    }

    /// Atomically applies one mutation.
    ///
    /// On validation failure the complete hierarchy, flat index, retired IDs,
    /// and revision remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`TreeModelError`] for invalid IDs, parents, indices, cycles,
    /// branch transitions, expansion, or revision overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// let delta = model.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
    /// assert_eq!(delta.revision(), 1);
    /// ```
    pub fn apply(
        &mut self,
        mutation: TreeMutation<T>,
    ) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        self.apply_batch([mutation])
    }

    /// Applies a batch only when the caller still observes `expected_revision`.
    ///
    /// This lets worker/UI bridges reject stale structural mutations without
    /// partially changing the retained model. Revision comparison precedes input
    /// collection and validation.
    ///
    /// # Errors
    ///
    /// Returns [`TreeModelError::StaleRevision`] on mismatch, otherwise the same
    /// validation errors as [`Self::apply_batch`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelError, TreeMutation};
    /// let mut model = TreeModel::<u8>::new();
    /// let error = model.apply_batch_if_revision(1, Vec::<TreeMutation<u8>>::new()).unwrap_err();
    /// assert_eq!(error, TreeModelError::StaleRevision { expected: 1, actual: 0 });
    /// ```
    pub fn apply_batch_if_revision(
        &mut self,
        expected_revision: u64,
        mutations: impl IntoIterator<Item = TreeMutation<T>>,
    ) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        if self.revision != expected_revision {
            return Err(TreeModelError::StaleRevision {
                expected: expected_revision,
                actual: self.revision,
            });
        }
        self.apply_batch(mutations)
    }

    /// Atomically applies mutations in iterator order as one transaction.
    ///
    /// Empty input succeeds without revision change. Nonempty success increments
    /// once. Batches of at least 1,024 mutations defer flat-index materialization
    /// to the transaction boundary; smaller batches splice it incrementally.
    ///
    /// # Errors
    ///
    /// Returns the first validation error and commits no candidate state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeMutation};
    /// let mut model = TreeModel::new();
    /// let delta = model.apply_batch([
    ///   TreeMutation::Insert { parent: None, index: 0, item: TreeItem::branch(1, "root") },
    ///   TreeMutation::Insert { parent: Some(1), index: 0, item: TreeItem::leaf(2, "file") },
    /// ]).unwrap();
    /// assert_eq!((delta.revision(), model.len()), (1, 2));
    /// ```
    pub fn apply_batch(
        &mut self,
        mutations: impl IntoIterator<Item = TreeMutation<T>>,
    ) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        let mutations: Vec<_> = mutations.into_iter().collect();
        if mutations.is_empty() {
            return Ok(TreeModelDelta {
                revision: self.revision,
                mutations,
            });
        }
        let mut candidate = self.clone();
        // A large provider result is one logical model transaction even when
        // its parent already exists. Updating the row-to-id map after every
        // inserted row would turn opening directories such as `/bin` into
        // O(N²) UI-thread work. Materialize the persistent index once at the
        // transaction boundary; ordinary interactive batches continue to use
        // the precise splice path below.
        let bulk_materialization = mutations.len() >= 1_024;
        if bulk_materialization {
            for mutation in &mutations {
                candidate.apply_one_hierarchy(mutation)?;
            }
        } else {
            for mutation in &mutations {
                candidate.apply_one_incremental(mutation)?;
            }
        }
        candidate.revision = self
            .revision
            .checked_add(1)
            .ok_or(TreeModelError::RevisionExhausted)?;
        if bulk_materialization {
            candidate.rebuild_flat_index();
        }
        candidate.flat.revision = candidate.revision;
        candidate.refresh_flat_metadata();
        let revision = candidate.revision;
        *self = candidate;
        Ok(TreeModelDelta {
            revision,
            mutations,
        })
    }

    /// Applies one mutation to hierarchy only, used by bulk transactions.
    fn apply_one_hierarchy(&mut self, mutation: &TreeMutation<T>) -> Result<(), TreeModelError<T>> {
        match mutation {
            TreeMutation::Insert {
                parent,
                index,
                item,
            } => self.insert(parent.clone(), *index, item.clone()),
            TreeMutation::Remove { id } => self.remove(id),
            TreeMutation::Update { item } => self.update(item.clone()),
            TreeMutation::Move {
                id,
                new_parent,
                index,
            } => self.move_item(id, new_parent.clone(), *index),
            TreeMutation::SetExpanded { id, expanded } => self.set_expanded(id, *expanded),
        }
    }

    /// Applies one mutation and splices the persistent visible-row index.
    fn apply_one_incremental(
        &mut self,
        mutation: &TreeMutation<T>,
    ) -> Result<(), TreeModelError<T>> {
        match mutation {
            TreeMutation::Insert {
                parent,
                index,
                item,
            } => {
                self.insert(parent.clone(), *index, item.clone())?;
                self.insert_visible_subtree(item.id())
            }
            TreeMutation::Remove { id } => {
                self.remove_visible_subtree(id);
                self.remove(id)
            }
            TreeMutation::Update { item } => self.update(item.clone()),
            TreeMutation::Move {
                id,
                new_parent,
                index,
            } => {
                self.remove_visible_subtree(id);
                self.move_item(id, new_parent.clone(), *index)?;
                self.insert_visible_subtree(id)
            }
            TreeMutation::SetExpanded { id, expanded } => {
                let was_expanded = self.is_expanded(id);
                self.set_expanded(id, *expanded)?;
                if was_expanded == *expanded {
                    return Ok(());
                }
                let Some(row) = self.flat.row_of(id) else {
                    return Ok(());
                };
                if *expanded {
                    let depth = self.flat.rows[row].depth.saturating_add(1);
                    let mut rows = Vec::new();
                    for child in self
                        .nodes
                        .get(id)
                        .expect("expanded node exists")
                        .children
                        .clone()
                    {
                        self.push_visible(&child, depth, &mut rows);
                    }
                    self.flat.rows.splice(row + 1..row + 1, rows);
                } else {
                    let range = self.visible_descendant_range(row);
                    self.flat.rows.drain(range);
                }
                self.refresh_flat_metadata();
                Ok(())
            }
        }
    }

    /// Validates and inserts an initially collapsed childless record.
    fn insert(
        &mut self,
        parent: Option<T>,
        index: usize,
        item: TreeItem<T>,
    ) -> Result<(), TreeModelError<T>> {
        let id = item.id.clone();
        if self.nodes.contains_key(&id) {
            return Err(TreeModelError::DuplicateId { id });
        }
        if self.retired_ids.contains(&id) {
            return Err(TreeModelError::ReusedId { id });
        }
        if let Some(parent_id) = &parent {
            let parent_record =
                self.nodes
                    .get(parent_id)
                    .ok_or_else(|| TreeModelError::MissingId {
                        id: parent_id.clone(),
                    })?;
            if !parent_record.item.branch {
                return Err(TreeModelError::ParentIsLeaf {
                    id: parent_id.clone(),
                });
            }
            if index > parent_record.children.len() {
                return Err(TreeModelError::InvalidIndex {
                    index,
                    len: parent_record.children.len(),
                });
            }
        } else if index > self.roots.len() {
            return Err(TreeModelError::InvalidIndex {
                index,
                len: self.roots.len(),
            });
        }

        self.nodes.insert(
            id.clone(),
            TreeRecord {
                item,
                parent: parent.clone(),
                children: Vec::new(),
                expanded: false,
            },
        );
        match parent {
            Some(parent) => self
                .nodes
                .get_mut(&parent)
                .expect("validated parent")
                .children
                .insert(index, id),
            None => self.roots.insert(index, id),
        }
        Ok(())
    }

    /// Detaches and iteratively retires an item and every descendant.
    fn remove(&mut self, id: &T) -> Result<(), TreeModelError<T>> {
        let parent = self
            .nodes
            .get(id)
            .ok_or_else(|| TreeModelError::MissingId { id: id.clone() })?
            .parent
            .clone();
        self.detach_from_parent(id, parent.as_ref());
        let mut pending = vec![id.clone()];
        while let Some(current) = pending.pop() {
            if let Some(record) = self.nodes.remove(&current) {
                pending.extend(record.children);
                self.retired_ids.insert(current);
            }
        }
        Ok(())
    }

    /// Replaces metadata, rejecting leaf conversion for a populated branch.
    fn update(&mut self, item: TreeItem<T>) -> Result<(), TreeModelError<T>> {
        let id = item.id.clone();
        let record = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| TreeModelError::MissingId { id: id.clone() })?;
        if !item.branch && !record.children.is_empty() {
            return Err(TreeModelError::NonEmptyBranchToLeaf { id });
        }
        record.item = item;
        if !record.item.branch {
            record.expanded = false;
        }
        Ok(())
    }

    /// Validates and moves one subtree to its final sibling position.
    fn move_item(
        &mut self,
        id: &T,
        new_parent: Option<T>,
        index: usize,
    ) -> Result<(), TreeModelError<T>> {
        let old_parent = self
            .nodes
            .get(id)
            .ok_or_else(|| TreeModelError::MissingId { id: id.clone() })?
            .parent
            .clone();
        if let Some(parent) = &new_parent {
            let parent_record = self
                .nodes
                .get(parent)
                .ok_or_else(|| TreeModelError::MissingId { id: parent.clone() })?;
            if !parent_record.item.branch {
                return Err(TreeModelError::ParentIsLeaf { id: parent.clone() });
            }
            if parent == id || self.is_descendant_of(parent, id) {
                return Err(TreeModelError::Cycle {
                    id: id.clone(),
                    new_parent: parent.clone(),
                });
            }
        }

        let target_len = new_parent
            .as_ref()
            .and_then(|parent| self.nodes.get(parent).map(|record| record.children.len()))
            .unwrap_or(self.roots.len());
        let same_container = old_parent == new_parent;
        let max_index = if same_container {
            target_len.saturating_sub(1)
        } else {
            target_len
        };
        if index > max_index {
            return Err(TreeModelError::InvalidIndex {
                index,
                len: max_index,
            });
        }

        self.detach_from_parent(id, old_parent.as_ref());
        match &new_parent {
            Some(parent) => self
                .nodes
                .get_mut(parent)
                .expect("validated parent")
                .children
                .insert(index, id.clone()),
            None => self.roots.insert(index, id.clone()),
        }
        self.nodes.get_mut(id).expect("validated item").parent = new_parent;
        Ok(())
    }

    /// Sets expansion only on an existing branch.
    fn set_expanded(&mut self, id: &T, expanded: bool) -> Result<(), TreeModelError<T>> {
        let record = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| TreeModelError::MissingId { id: id.clone() })?;
        if !record.item.branch {
            return Err(TreeModelError::NotBranch { id: id.clone() });
        }
        record.expanded = expanded;
        Ok(())
    }

    /// Removes an ID from its parent child list or root list when present.
    fn detach_from_parent(&mut self, id: &T, parent: Option<&T>) {
        let siblings = match parent {
            Some(parent) => {
                &mut self
                    .nodes
                    .get_mut(parent)
                    .expect("validated parent")
                    .children
            }
            None => &mut self.roots,
        };
        if let Some(index) = siblings.iter().position(|candidate| candidate == id) {
            siblings.remove(index);
        }
    }

    /// Walks parent links and reports whether candidate is at/below ancestor.
    fn is_descendant_of(&self, candidate: &T, ancestor: &T) -> bool {
        let mut current = Some(candidate);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.nodes.get(id).and_then(|record| record.parent.as_ref());
        }
        false
    }

    /// Rebuilds visible ID lookup and first-enabled row from cached rows.
    fn refresh_flat_metadata(&mut self) {
        self.flat.row_by_id = self
            .flat
            .rows
            .iter()
            .enumerate()
            .map(|(index, row): (usize, &FlatTreeRow<T>)| (row.node_id.clone(), index))
            .collect();
        self.flat.first_enabled_row = self.flat.rows.iter().position(|row| {
            self.nodes
                .get(&row.node_id)
                .is_some_and(|record| !record.item.disabled)
        });
    }

    /// Fully materializes visible rows and saturating-increments rebuild count.
    fn rebuild_flat_index(&mut self) {
        let mut rows = Vec::with_capacity(self.nodes.len());
        for root in &self.roots {
            self.push_visible(root, 0, &mut rows);
        }
        self.flat.rows = rows;
        self.flat.rebuilds = self.flat.rebuilds.saturating_add(1);
    }

    /// Removes a visible row and its visible descendants when materialized.
    fn remove_visible_subtree(&mut self, id: &T) {
        let Some(row) = self.flat.row_of(id) else {
            return;
        };
        let range = row..self.visible_descendant_range(row).end;
        self.flat.rows.drain(range);
        self.refresh_flat_metadata();
    }

    /// Inserts a subtree's visible rows when its parent path is visible/expanded.
    fn insert_visible_subtree(&mut self, id: &T) -> Result<(), TreeModelError<T>> {
        let record = self
            .nodes
            .get(id)
            .ok_or_else(|| TreeModelError::MissingId { id: id.clone() })?;
        let (insert_at, depth) = match record.parent.as_ref() {
            Some(parent) => {
                let Some(parent_row) = self.flat.row_of(parent) else {
                    return Ok(());
                };
                if !self.is_expanded(parent) {
                    return Ok(());
                }
                let siblings = &self.nodes.get(parent).expect("parent exists").children;
                let index = siblings
                    .iter()
                    .position(|sibling| sibling == id)
                    .expect("inserted child is linked");
                let insert_at = siblings
                    .get(index + 1)
                    .and_then(|next| self.flat.row_of(next))
                    .unwrap_or_else(|| self.visible_descendant_range(parent_row).end);
                (
                    insert_at,
                    self.flat.rows[parent_row].depth.saturating_add(1),
                )
            }
            None => {
                let index = self
                    .roots
                    .iter()
                    .position(|root| root == id)
                    .expect("inserted root is linked");
                let insert_at = self
                    .roots
                    .get(index + 1)
                    .and_then(|next| self.flat.row_of(next))
                    .unwrap_or(self.flat.rows.len());
                (insert_at, 0)
            }
        };
        let mut rows = Vec::new();
        self.push_visible(id, depth, &mut rows);
        self.flat.rows.splice(insert_at..insert_at, rows);
        self.refresh_flat_metadata();
        Ok(())
    }

    /// Returns visible descendants after `row`, excluding the row itself.
    fn visible_descendant_range(&self, row: usize) -> std::ops::Range<usize> {
        let depth = self.flat.rows[row].depth;
        let end = self
            .flat
            .rows
            .iter()
            .enumerate()
            .skip(row + 1)
            .find(|(_, candidate)| candidate.depth <= depth)
            .map_or(self.flat.rows.len(), |(index, _)| index);
        row + 1..end
    }

    /// Iteratively appends a depth-first expanded subtree with saturating depth.
    fn push_visible(&self, id: &T, depth: u16, rows: &mut Vec<FlatTreeRow<T>>) {
        // Model depth is user-controlled; never recurse on the process stack.
        let mut pending = vec![(id.clone(), depth)];
        while let Some((current, current_depth)) = pending.pop() {
            let Some(record) = self.nodes.get(&current) else {
                continue;
            };
            rows.push(FlatTreeRow {
                node_id: current,
                depth: current_depth,
            });
            if record.expanded {
                pending.extend(
                    record
                        .children
                        .iter()
                        .rev()
                        .cloned()
                        .map(|child| (child, current_depth.saturating_add(1))),
                );
            }
        }
    }
}

/// Revision listener stored weakly by the shared handle.
type RevisionCallback = dyn Fn(u64);

#[derive(Default)]
/// Weak subscriber slots and wrapping allocation cursor.
struct SubscriberRegistry {
    /// Next subscription slot ID; wraps after `u64::MAX`.
    next_id: u64,
    /// Weak callbacks keyed by their guard-owned slot ID.
    callbacks: HashMap<u64, Weak<RevisionCallback>>,
}

/// UI-local shared handle for a retained tree model.
///
/// Clones share one `Rc<RefCell<_>>` model and subscriber registry, so the handle
/// is single-threaded. Successful nonempty writes notify live weak subscribers
/// after releasing internal mutable borrows.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeModelHandle, TreeMutation};
/// let handle = TreeModelHandle::new(TreeModel::new());
/// handle.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
/// assert_eq!(handle.revision(), 1);
/// ```
#[derive(Clone)]
pub struct TreeModelHandle<T> {
    /// Shared single-threaded retained model.
    model: Rc<RefCell<TreeModel<T>>>,
    /// Shared weak revision subscribers.
    subscribers: Rc<RefCell<SubscriberRegistry>>,
}

impl<T> fmt::Debug for TreeModelHandle<T>
where
    T: Clone + Eq + Hash + fmt::Debug,
{
    /// Formats revision and active/visible counts without dumping hierarchy.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let model = self.model.borrow();
        formatter
            .debug_struct("TreeModelHandle")
            .field("revision", &model.revision())
            .field("nodes", &model.len())
            .field("visible", &model.visible_len())
            .finish_non_exhaustive()
    }
}

impl<T> Default for TreeModelHandle<T> {
    fn default() -> Self {
        Self {
            model: Rc::new(RefCell::new(TreeModel::default())),
            subscribers: Rc::new(RefCell::new(SubscriberRegistry::default())),
        }
    }
}

impl<T> TreeModelHandle<T>
where
    T: Clone + Eq + Hash + fmt::Debug,
{
    /// Wraps an existing model in a new unshared subscriber registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// let handle = TreeModelHandle::new(TreeModel::<u64>::new());
    /// assert_eq!(handle.revision(), 0);
    /// ```
    pub fn new(model: TreeModel<T>) -> Self {
        Self {
            model: Rc::new(RefCell::new(model)),
            subscribers: Rc::new(RefCell::new(SubscriberRegistry::default())),
        }
    }

    /// Returns the shared model's current revision.
    ///
    /// # Panics
    ///
    /// Panics on reentrant access while the model is mutably borrowed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// assert_eq!(TreeModelHandle::new(TreeModel::<u8>::new()).revision(), 0);
    /// ```
    pub fn revision(&self) -> u64 {
        self.model.borrow().revision()
    }

    /// Runs `read` under a shared borrow and returns its result.
    ///
    /// # Panics
    ///
    /// Panics on conflicting reentrant model access. Do not call handle mutation
    /// methods from inside the closure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// let handle = TreeModelHandle::new(TreeModel::<u8>::new());
    /// assert_eq!(handle.read(|model| model.len()), 0);
    /// ```
    pub fn read<R>(&self, read: impl FnOnce(&TreeModel<T>) -> R) -> R {
        read(&self.model.borrow())
    }

    /// Atomically applies one mutation and notifies on nonempty success.
    ///
    /// # Errors
    ///
    /// Returns model validation or revision-overflow errors.
    ///
    /// # Panics
    ///
    /// Panics on conflicting reentrant model/subscriber borrows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeModelHandle, TreeMutation};
    /// let handle = TreeModelHandle::new(TreeModel::new());
    /// handle.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
    /// assert_eq!(handle.read(|model| model.len()), 1);
    /// ```
    pub fn apply(&self, mutation: TreeMutation<T>) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        self.apply_batch([mutation])
    }

    /// Atomically applies a batch and notifies once on nonempty success.
    ///
    /// Empty batches do not notify or advance revision.
    ///
    /// # Errors
    ///
    /// Returns the first model validation error without committing candidate state.
    ///
    /// # Panics
    ///
    /// Panics on conflicting reentrant model/subscriber borrows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeModelHandle, TreeMutation};
    /// let handle = TreeModelHandle::new(TreeModel::new());
    /// let delta = handle.apply_batch([TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }]).unwrap();
    /// assert_eq!(delta.revision(), 1);
    /// ```
    pub fn apply_batch(
        &self,
        mutations: impl IntoIterator<Item = TreeMutation<T>>,
    ) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        let delta = self.model.borrow_mut().apply_batch(mutations)?;
        if !delta.mutations.is_empty() {
            self.notify(delta.revision);
        }
        Ok(delta)
    }

    /// Applies and notifies only when `expected_revision` is still current.
    ///
    /// # Errors
    ///
    /// Returns [`TreeModelError::StaleRevision`] on mismatch or another model
    /// validation error without committing candidate state.
    ///
    /// # Panics
    ///
    /// Panics on conflicting reentrant model/subscriber borrows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelError, TreeModelHandle, TreeMutation};
    /// let handle = TreeModelHandle::new(TreeModel::<u8>::new());
    /// let error = handle.apply_batch_if_revision(2, Vec::<TreeMutation<u8>>::new()).unwrap_err();
    /// assert_eq!(error, TreeModelError::StaleRevision { expected: 2, actual: 0 });
    /// ```
    pub fn apply_batch_if_revision(
        &self,
        expected_revision: u64,
        mutations: impl IntoIterator<Item = TreeMutation<T>>,
    ) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        let delta = self
            .model
            .borrow_mut()
            .apply_batch_if_revision(expected_revision, mutations)?;
        if !delta.mutations.is_empty() {
            self.notify(delta.revision);
        }
        Ok(delta)
    }

    /// Registers a weak revision listener.
    ///
    /// The model never owns the target; dropping either the callback or returned
    /// guard removes the edge. The callback runs after model/subscriber mutable
    /// borrows are released and receives the committed revision.
    ///
    /// # Panics
    ///
    /// Panics on conflicting reentrant subscriber-registry access.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::Cell;
    /// use std::rc::Rc;
    /// use ailloli_ui_widgets::controls::{TreeItem, TreeModel, TreeModelHandle, TreeMutation};
    /// let handle = TreeModelHandle::new(TreeModel::new());
    /// let seen = Rc::new(Cell::new(0));
    /// let sink = seen.clone();
    /// let callback: Rc<dyn Fn(u64)> = Rc::new(move |revision| sink.set(revision));
    /// let _guard = handle.subscribe(&callback);
    /// handle.apply(TreeMutation::Insert { parent: None, index: 0, item: TreeItem::leaf(1, "file") }).unwrap();
    /// assert_eq!(seen.get(), 1);
    /// ```
    pub fn subscribe(&self, callback: &Rc<RevisionCallback>) -> TreeModelSubscription {
        let mut subscribers = self.subscribers.borrow_mut();
        let id = subscribers.next_id;
        subscribers.next_id = subscribers.next_id.wrapping_add(1);
        subscribers.callbacks.insert(id, Rc::downgrade(callback));
        TreeModelSubscription {
            id,
            subscribers: Rc::downgrade(&self.subscribers),
        }
    }

    /// Prunes dead callbacks and invokes upgraded listeners outside registry borrow.
    fn notify(&self, revision: u64) {
        let callbacks: Vec<_> = {
            let mut subscribers = self.subscribers.borrow_mut();
            let callbacks = subscribers
                .callbacks
                .iter()
                .filter_map(|(id, callback)| callback.upgrade().map(|callback| (*id, callback)))
                .collect::<Vec<_>>();
            subscribers
                .callbacks
                .retain(|_, callback| callback.strong_count() != 0);
            callbacks
        };
        for (_, callback) in callbacks {
            callback(revision);
        }
    }
}

/// RAII subscription guard returned by [`TreeModelHandle::subscribe`].
///
/// Dropping the guard removes its callback slot when the registry still exists.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
/// let handle = TreeModelHandle::new(TreeModel::<u8>::new());
/// let callback: Rc<dyn Fn(u64)> = Rc::new(|_| {});
/// let guard = handle.subscribe(&callback);
/// drop(guard);
/// ```
pub struct TreeModelSubscription {
    /// Callback slot ID owned by this guard.
    id: u64,
    /// Weak registry reference so guards do not keep handles alive.
    subscribers: Weak<RefCell<SubscriberRegistry>>,
}

impl fmt::Debug for TreeModelSubscription {
    /// Formats the subscription ID without retaining registry state.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeModelSubscription")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for TreeModelSubscription {
    /// Removes the callback slot when its registry remains alive.
    fn drop(&mut self) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.borrow_mut().callbacks.remove(&self.id);
        }
    }
}
