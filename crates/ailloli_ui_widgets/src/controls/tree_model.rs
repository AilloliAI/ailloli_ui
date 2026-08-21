use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::rc::{Rc, Weak};

use ailloli_ui_core::{Color, IconId};

use super::tree_view::TreeNodeTrailingAction;

/// Stable retained item stored by [`TreeModel`].
#[derive(Clone, PartialEq)]
pub struct TreeItem<T> {
    id: T,
    label: String,
    branch: bool,
    disabled: bool,
    leading_icon: Option<IconId>,
    leading_icon_tint: Option<Color>,
    trailing_action: Option<TreeNodeTrailingAction>,
    transient: bool,
}

impl<T: fmt::Debug> fmt::Debug for TreeItem<T> {
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
    pub fn branch(id: T, label: impl Into<String>) -> Self {
        Self::new(id, label, true)
    }

    pub fn leaf(id: T, label: impl Into<String>) -> Self {
        Self::new(id, label, false)
    }

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

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn leading_icon_tint(mut self, tint: Color) -> Self {
        self.leading_icon_tint = Some(tint);
        self
    }

    pub fn trailing_action(mut self, action: TreeNodeTrailingAction) -> Self {
        self.trailing_action = Some(action);
        self
    }

    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    pub fn id(&self) -> &T {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn is_branch(&self) -> bool {
        self.branch
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn leading_icon_ref(&self) -> Option<&IconId> {
        self.leading_icon.as_ref()
    }

    pub const fn leading_icon_tint_ref(&self) -> Option<Color> {
        self.leading_icon_tint
    }

    pub fn trailing_action_ref(&self) -> Option<&TreeNodeTrailingAction> {
        self.trailing_action.as_ref()
    }

    pub const fn is_transient(&self) -> bool {
        self.transient
    }
}

/// Atomic retained-tree mutation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum TreeMutation<T> {
    Insert {
        parent: Option<T>,
        index: usize,
        item: TreeItem<T>,
    },
    Remove {
        id: T,
    },
    Update {
        item: TreeItem<T>,
    },
    Move {
        id: T,
        new_parent: Option<T>,
        index: usize,
    },
    SetExpanded {
        id: T,
        expanded: bool,
    },
}

/// One successfully committed atomic batch.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeModelDelta<T> {
    revision: u64,
    mutations: Vec<TreeMutation<T>>,
}

impl<T> TreeModelDelta<T> {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn mutations(&self) -> &[TreeMutation<T>] {
        &self.mutations
    }
}

/// Validation failure for a retained tree mutation. The whole batch is rolled
/// back on error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeModelError<T: fmt::Debug> {
    #[error("tree item already exists: {id:?}")]
    DuplicateId { id: T },
    #[error("tree item does not exist: {id:?}")]
    MissingId { id: T },
    #[error("tree item identifier was retired and cannot be reused: {id:?}")]
    ReusedId { id: T },
    #[error("parent is not a branch: {id:?}")]
    ParentIsLeaf { id: T },
    #[error("child index {index} is outside 0..={len}")]
    InvalidIndex { index: usize, len: usize },
    #[error("moving {id:?} below {new_parent:?} would create a cycle")]
    Cycle { id: T, new_parent: T },
    #[error("a branch with children cannot become a leaf: {id:?}")]
    NonEmptyBranchToLeaf { id: T },
    #[error("only branch items can be expanded: {id:?}")]
    NotBranch { id: T },
    #[error("tree model revision space is exhausted")]
    RevisionExhausted,
    #[error("stale tree mutation: expected revision {expected}, current revision {actual}")]
    StaleRevision { expected: u64, actual: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatTreeRow<T> {
    node_id: T,
    depth: u16,
}

impl<T> FlatTreeRow<T> {
    pub fn node_id(&self) -> &T {
        &self.node_id
    }

    pub const fn depth(&self) -> u16 {
        self.depth
    }
}

/// Persistent visible-row index. It changes only when a model mutation is
/// committed, never during layout, paint, hit-test, or a pure scroll.
#[derive(Debug, Clone)]
pub struct FlatTreeIndex<T> {
    rows: Vec<FlatTreeRow<T>>,
    row_by_id: HashMap<T, usize>,
    revision: u64,
    rebuilds: u64,
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
    pub fn rows(&self) -> &[FlatTreeRow<T>] {
        &self.rows
    }

    pub fn row_of(&self, id: &T) -> Option<usize> {
        self.row_by_id.get(id).copied()
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn rebuilds(&self) -> u64 {
        self.rebuilds
    }

    pub const fn first_enabled_row(&self) -> Option<usize> {
        self.first_enabled_row
    }
}

#[derive(Debug, Clone)]
struct TreeRecord<T> {
    item: TreeItem<T>,
    parent: Option<T>,
    children: Vec<T>,
    expanded: bool,
}

/// Retained hierarchical model with stable identifiers and an incremental
/// presentation revision.
#[derive(Debug, Clone)]
pub struct TreeModel<T> {
    nodes: HashMap<T, TreeRecord<T>>,
    roots: Vec<T>,
    retired_ids: HashSet<T>,
    flat: FlatTreeIndex<T>,
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
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn visible_len(&self) -> usize {
        self.flat.rows.len()
    }

    pub fn flat_index(&self) -> &FlatTreeIndex<T> {
        &self.flat
    }

    pub fn item(&self, id: &T) -> Option<&TreeItem<T>> {
        self.nodes.get(id).map(|record| &record.item)
    }

    pub fn parent(&self, id: &T) -> Option<&T> {
        self.nodes.get(id).and_then(|record| record.parent.as_ref())
    }

    pub fn children(&self, id: &T) -> Option<&[T]> {
        self.nodes.get(id).map(|record| record.children.as_slice())
    }

    pub fn roots(&self) -> &[T] {
        &self.roots
    }

    pub fn is_expanded(&self, id: &T) -> bool {
        self.nodes.get(id).is_some_and(|record| record.expanded)
    }

    pub fn apply(
        &mut self,
        mutation: TreeMutation<T>,
    ) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        self.apply_batch([mutation])
    }

    /// Applies a batch only when the caller still observes `expected_revision`.
    /// This lets worker/UI bridges reject stale structural mutations without
    /// partially changing the retained model.
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

    fn rebuild_flat_index(&mut self) {
        let mut rows = Vec::with_capacity(self.nodes.len());
        for root in &self.roots {
            self.push_visible(root, 0, &mut rows);
        }
        self.flat.rows = rows;
        self.flat.rebuilds = self.flat.rebuilds.saturating_add(1);
    }

    fn remove_visible_subtree(&mut self, id: &T) {
        let Some(row) = self.flat.row_of(id) else {
            return;
        };
        let range = row..self.visible_descendant_range(row).end;
        self.flat.rows.drain(range);
        self.refresh_flat_metadata();
    }

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

type RevisionCallback = dyn Fn(u64);

#[derive(Default)]
struct SubscriberRegistry {
    next_id: u64,
    callbacks: HashMap<u64, Weak<RevisionCallback>>,
}

/// UI-local shared handle for a retained tree model.
#[derive(Clone)]
pub struct TreeModelHandle<T> {
    model: Rc<RefCell<TreeModel<T>>>,
    subscribers: Rc<RefCell<SubscriberRegistry>>,
}

impl<T> fmt::Debug for TreeModelHandle<T>
where
    T: Clone + Eq + Hash + fmt::Debug,
{
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
    pub fn new(model: TreeModel<T>) -> Self {
        Self {
            model: Rc::new(RefCell::new(model)),
            subscribers: Rc::new(RefCell::new(SubscriberRegistry::default())),
        }
    }

    pub fn revision(&self) -> u64 {
        self.model.borrow().revision()
    }

    pub fn read<R>(&self, read: impl FnOnce(&TreeModel<T>) -> R) -> R {
        read(&self.model.borrow())
    }

    pub fn apply(&self, mutation: TreeMutation<T>) -> Result<TreeModelDelta<T>, TreeModelError<T>> {
        self.apply_batch([mutation])
    }

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

    /// Registers a weak revision listener. The model never owns the target;
    /// dropping either the callback or returned guard removes the edge.
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
pub struct TreeModelSubscription {
    id: u64,
    subscribers: Weak<RefCell<SubscriberRegistry>>,
}

impl fmt::Debug for TreeModelSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeModelSubscription")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for TreeModelSubscription {
    fn drop(&mut self) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.borrow_mut().callbacks.remove(&self.id);
        }
    }
}
