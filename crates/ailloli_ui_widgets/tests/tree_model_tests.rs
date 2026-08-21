use std::rc::Rc;

use ailloli_ui_widgets::controls::{
    TreeItem, TreeModel, TreeModelError, TreeModelHandle, TreeMutation,
};

fn insert(id: u64, parent: Option<u64>, index: usize, branch: bool) -> TreeMutation<u64> {
    let item = if branch {
        TreeItem::branch(id, format!("node-{id}"))
    } else {
        TreeItem::leaf(id, format!("node-{id}"))
    };
    TreeMutation::Insert {
        parent,
        index,
        item,
    }
}

#[test]
fn batches_are_atomic_and_revisions_are_monotone() {
    let mut model = TreeModel::new();
    let first = model
        .apply_batch([insert(1, None, 0, true), insert(2, Some(1), 0, false)])
        .unwrap();
    assert_eq!(first.revision(), 1);
    assert_eq!(model.len(), 2);

    let before = model.clone();
    let error = model
        .apply_batch([insert(3, Some(1), 1, false), insert(2, None, 1, false)])
        .unwrap_err();
    assert_eq!(error, TreeModelError::DuplicateId { id: 2 });
    assert_eq!(model.revision(), before.revision());
    assert_eq!(model.len(), before.len());
    assert!(model.item(&3).is_none());
}

#[test]
fn flat_index_changes_on_mutation_but_not_on_reads() {
    let mut model = TreeModel::new();
    model
        .apply_batch([
            insert(1, None, 0, true),
            insert(2, Some(1), 0, false),
            insert(3, Some(1), 1, true),
            insert(4, Some(3), 0, false),
        ])
        .unwrap();
    assert_eq!(model.visible_len(), 1);
    let rebuilds = model.flat_index().rebuilds();
    for _ in 0..100 {
        assert_eq!(model.flat_index().rows().len(), 1);
    }
    assert_eq!(model.flat_index().rebuilds(), rebuilds);

    model
        .apply(TreeMutation::SetExpanded {
            id: 1,
            expanded: true,
        })
        .unwrap();
    assert_eq!(model.visible_len(), 3);
    assert_eq!(model.flat_index().row_of(&3), Some(2));
    model
        .apply(TreeMutation::SetExpanded {
            id: 3,
            expanded: true,
        })
        .unwrap();
    assert_eq!(model.visible_len(), 4);
    assert_eq!(model.flat_index().rows()[3].depth(), 2);
}

#[test]
fn update_preserves_structure_and_rejects_nonempty_branch_to_leaf() {
    let mut model = TreeModel::new();
    model
        .apply_batch([insert(1, None, 0, true), insert(2, Some(1), 0, false)])
        .unwrap();
    model
        .apply(TreeMutation::SetExpanded {
            id: 1,
            expanded: true,
        })
        .unwrap();
    model
        .apply(TreeMutation::Update {
            item: TreeItem::branch(1, "renamed"),
        })
        .unwrap();
    assert_eq!(model.item(&1).unwrap().label(), "renamed");
    assert_eq!(model.children(&1), Some([2].as_slice()));
    assert!(model.is_expanded(&1));

    assert_eq!(
        model
            .apply(TreeMutation::Update {
                item: TreeItem::leaf(1, "invalid"),
            })
            .unwrap_err(),
        TreeModelError::NonEmptyBranchToLeaf { id: 1 }
    );
}

#[test]
fn moves_reject_cycles_and_removals_retire_identifiers() {
    let mut model = TreeModel::new();
    model
        .apply_batch([
            insert(1, None, 0, true),
            insert(2, Some(1), 0, true),
            insert(3, Some(2), 0, false),
        ])
        .unwrap();
    assert_eq!(
        model
            .apply(TreeMutation::Move {
                id: 1,
                new_parent: Some(2),
                index: 0,
            })
            .unwrap_err(),
        TreeModelError::Cycle {
            id: 1,
            new_parent: 2,
        }
    );
    model.apply(TreeMutation::Remove { id: 2 }).unwrap();
    assert!(model.item(&2).is_none());
    assert!(model.item(&3).is_none());
    assert_eq!(
        model.apply(insert(2, Some(1), 0, false)).unwrap_err(),
        TreeModelError::ReusedId { id: 2 }
    );
}

#[test]
fn subscriptions_are_weak_and_raii_scoped() {
    let handle = TreeModelHandle::new(TreeModel::<u64>::new());
    let calls = Rc::new(std::cell::Cell::new(0_u64));
    let calls_for_callback = calls.clone();
    let callback: Rc<dyn Fn(u64)> = Rc::new(move |revision| calls_for_callback.set(revision));
    let guard = handle.subscribe(&callback);
    handle.apply(insert(1, None, 0, false)).unwrap();
    assert_eq!(calls.get(), 1);

    drop(guard);
    handle.apply(insert(2, None, 1, false)).unwrap();
    assert_eq!(calls.get(), 1);

    let guard = handle.subscribe(&callback);
    drop(callback);
    handle.apply(insert(3, None, 2, false)).unwrap();
    assert_eq!(calls.get(), 1);
    drop(guard);
}
