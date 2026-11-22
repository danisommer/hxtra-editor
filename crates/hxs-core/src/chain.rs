//! The signal chain's grid as an editor sees it: eight columns, each holding at most one block,
//! on row A or row B ([fact, user's report] the unit drops a block when another one takes its
//! column); inserting with push along the columns, the moves that reach a layout, and the
//! edits that turn one chain into another (undo).

use std::collections::BTreeMap;

use rmpv::Value;

use crate::preset::{Block, ROW_A, ROW_B};
use crate::stomp::Stomp;
use crate::{Error, Result};

pub const ROW_LEN: usize = 8;

/// Occupied positions and what is on each (an id the caller chooses).
pub type Layout = BTreeMap<usize, u64>;

/// The row (0 = A, 1 = B) and index of a grid position.
pub fn row_of(position: usize) -> Option<(usize, usize)> {
    match position {
        p if ROW_A.contains(&p) => Some((0, p - ROW_A.start())),
        p if ROW_B.contains(&p) => Some((1, p - ROW_B.start())),
        _ => None,
    }
}

pub fn position(row: usize, index: usize) -> usize {
    match row {
        0 => ROW_A.start() + index,
        _ => ROW_B.start() + index,
    }
}

pub fn all_positions() -> impl Iterator<Item = usize> {
    ROW_A.chain(ROW_B)
}

pub fn column_of(position: usize) -> Option<usize> {
    row_of(position).map(|(_, c)| c)
}

/// The block of each column, with its row.
fn columns(layout: &Layout) -> Vec<Option<(usize, u64)>> {
    let mut cols = vec![None; ROW_LEN];
    for (p, id) in layout {
        if let Some((row, c)) = row_of(*p) {
            cols[c] = Some((row, *id));
        }
    }
    cols
}

fn from_columns(cols: &[Option<(usize, u64)>]) -> Layout {
    cols.iter().enumerate().filter_map(|(c, s)| s.map(|(row, id)| (position(row, c), id))).collect()
}

/// `item` (now at `from`, or new) put at `target`, one block per column:
/// - a free column just takes it;
/// - a block moved onto the other row of an occupied column swaps columns with the block there;
/// - otherwise it takes that column, right before the block that held it, which moves forward
///   with the blocks after it (each keeping its row) up to the first free column; without a free
///   column ahead, the blocks before it move back instead.
///
/// `None` when there is no free column to push into.
pub fn insert(layout: &Layout, item: u64, from: Option<usize>, target: usize) -> Option<Layout> {
    let (row, j) = row_of(target)?;
    let mut rest = layout.clone();
    if let Some(f) = from {
        rest.remove(&f);
    }
    let mut cols = columns(&rest);
    let ahead = (j + 1..ROW_LEN).find(|&k| cols[k].is_none());
    let behind = (0..j).rev().find(|&k| cols[k].is_none());
    let swap_to = from.and_then(column_of).filter(|_| !layout.contains_key(&target));
    match (cols[j], swap_to, ahead, behind) {
        (None, _, _, _) => cols[j] = Some((row, item)),
        (Some(other), Some(back), _, _) if back != j => {
            cols[back] = Some(other);
            cols[j] = Some((row, item));
        }
        (Some(_), _, Some(k), _) => {
            for m in (j + 1..=k).rev() {
                cols[m] = cols[m - 1];
            }
            cols[j] = Some((row, item));
        }
        (Some(_), _, None, Some(k)) => {
            for m in k..j - 1 {
                cols[m] = cols[m + 1];
            }
            cols[j - 1] = Some((row, item));
        }
        (Some(_), _, None, None) => return None,
    }
    Some(from_columns(&cols))
}

/// Moves (editor 0x2b) that bring every item of `target` found in `current` to its place, each
/// to a position whose column no other block holds at that moment; a cycle goes through a free
/// column.
pub fn plan_moves(current: &Layout, target: &Layout) -> Vec<(usize, usize)> {
    let mut at: BTreeMap<u64, usize> = current.iter().map(|(p, id)| (*id, *p)).collect();
    let want: BTreeMap<u64, usize> = target.iter().filter(|(_, id)| at.contains_key(id)).map(|(p, id)| (*id, *p)).collect();
    let free_for = |at: &BTreeMap<u64, usize>, id: u64, p: usize| at.iter().all(|(other, q)| *other == id || column_of(*q) != column_of(p));
    let mut moves = Vec::new();
    for _ in 0..4 * all_positions().count() {
        let misplaced: Vec<(u64, usize, usize)> = want.iter().filter(|(id, p)| at[id] != **p).map(|(id, p)| (*id, at[id], *p)).collect();
        let Some(&(first, from_first, _)) = misplaced.first() else { break };
        let step = match misplaced.iter().find(|(id, _, to)| free_for(&at, *id, *to)) {
            Some(&m) => Some(m),
            None => {
                let (row, _) = row_of(from_first).unwrap_or((0, 0));
                let wanted_cols: Vec<Option<usize>> = want.values().map(|p| column_of(*p)).collect();
                (0..ROW_LEN)
                    .map(|c| position(row, c))
                    .find(|p| free_for(&at, first, *p) && !wanted_cols.contains(&column_of(*p)))
                    .map(|p| (first, from_first, p))
            }
        };
        let Some((id, from, to)) = step else { break };
        at.insert(id, to);
        moves.push((from, to));
    }
    moves
}

/// One edit of the edit buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Remove { position: u64 },
    Add { position: u64, preset_id: u64, cab: Option<u64>, kind: u64 },
    ChangeModel { position: u64, preset_id: u64, cab: Option<u64> },
    Set { position: u64, index: u64, value: Value },
    Enable { position: u64, on: bool },
}

/// The edits that turn the chain `current` into `target`, position by position. A block that
/// gets a new model starts from that model's defaults (`defaults`), so only the values that
/// differ from them are sent; a kept model sends only what changed.
pub fn reconcile(current: &[Block], target: &[Block], defaults: impl Fn(u64) -> Option<Vec<Value>>) -> Result<Vec<Op>> {
    let at = |blocks: &[Block], p: usize| blocks.iter().find(|b| b.position == p).cloned();
    let (mut removes, mut changes, mut adds) = (Vec::new(), Vec::new(), Vec::new());
    for p in all_positions() {
        let position = p as u64;
        match (at(current, p), at(target, p)) {
            (None, None) => {}
            (Some(_), None) => removes.push(Op::Remove { position }),
            (None, Some(t)) => {
                let kind = t.kind.ok_or_else(|| Error::Preset(format!("the block at {p} has no kind to add it with")))?;
                adds.push(Op::Add { position, preset_id: t.preset_id, cab: t.cab, kind });
                adds.extend(values_from(&defaults(t.preset_id).unwrap_or_default(), &t, position));
                adds.extend((!t.enabled).then_some(Op::Enable { position, on: false }));
            }
            (Some(c), Some(t)) if c.preset_id != t.preset_id || c.cab != t.cab => {
                changes.push(Op::ChangeModel { position, preset_id: t.preset_id, cab: t.cab });
                changes.extend(values_from(&defaults(t.preset_id).unwrap_or_default(), &t, position));
                changes.extend((c.enabled != t.enabled).then_some(Op::Enable { position, on: t.enabled }));
            }
            (Some(c), Some(t)) => {
                changes.extend(values_from(&c.params, &t, position));
                changes.extend((c.enabled != t.enabled).then_some(Op::Enable { position, on: t.enabled }));
            }
        }
    }
    Ok(removes.into_iter().chain(changes).chain(adds).collect())
}

fn values_from(baseline: &[Value], target: &Block, position: u64) -> Vec<Op> {
    let set = target.params.iter().enumerate().filter(|(i, v)| baseline.get(*i) != Some(*v));
    set.map(|(i, v)| Op::Set { position, index: i as u64, value: v.clone() }).collect()
}

/// Send the edits, in order.
pub fn apply(stomp: &mut Stomp, ops: &[Op]) -> Result<()> {
    for op in ops {
        match op {
            Op::Remove { position } => stomp.remove_block(*position)?,
            Op::Add { position, preset_id, cab, kind } => stomp.add_block(*position, *preset_id, *cab, *kind)?,
            Op::ChangeModel { position, preset_id, cab } => stomp.change_model(*position, *preset_id, *cab)?,
            Op::Set { position, index, value } => stomp.set_parameter(*position, 0, *index, value.clone())?,
            Op::Enable { position, on } => stomp.set_block_enabled(*position, *on)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(items: &[(usize, u64)]) -> Layout {
        items.iter().copied().collect()
    }

    #[test]
    fn dropping_on_a_free_column_just_moves() {
        assert_eq!(insert(&layout(&[(1, 10)]), 10, Some(1), 4), Some(layout(&[(4, 10)])));
        assert_eq!(insert(&layout(&[(1, 10)]), 10, Some(1), 14), Some(layout(&[(14, 10)])), "to row B");
    }

    #[test]
    fn dropping_on_a_block_inserts_before_it_and_pushes_the_columns_forward() {
        let l = layout(&[(1, 10), (12, 20), (3, 30), (6, 60)]);
        assert_eq!(insert(&l, 99, None, 2), Some(layout(&[(1, 10), (2, 99), (13, 20), (4, 30), (6, 60)])), "each pushed block keeps its row");
        assert_eq!(insert(&l, 60, Some(6), 1), Some(layout(&[(1, 60), (2, 10), (13, 20), (4, 30)])), "the dragged block leaves its column first");
    }

    #[test]
    fn a_block_moved_onto_the_other_row_of_a_taken_column_swaps_columns() {
        let l = layout(&[(1, 10), (13, 30)]);
        assert_eq!(insert(&l, 10, Some(1), 3), Some(layout(&[(3, 10), (11, 30)])));
    }

    #[test]
    fn a_new_block_on_the_other_row_of_a_taken_column_inserts_and_pushes() {
        let l = layout(&[(13, 30)]);
        assert_eq!(insert(&l, 99, None, 3), Some(layout(&[(3, 99), (14, 30)])));
    }

    #[test]
    fn without_a_free_column_ahead_the_blocks_before_move_back() {
        let l = layout(&[(3, 30), (14, 40), (5, 50), (6, 60), (17, 70), (8, 80)]);
        assert_eq!(insert(&l, 99, None, 5), Some(layout(&[(2, 30), (13, 40), (4, 99), (5, 50), (6, 60), (17, 70), (8, 80)])));
        let full: Layout = (1..=8).map(|p| (p, p as u64)).collect();
        assert_eq!(insert(&full, 99, None, 14), None, "eight blocks: no free column for a new one");
        assert_eq!(insert(&full, 3, Some(3), 6), Some(layout(&[(1, 1), (2, 2), (3, 4), (4, 5), (5, 3), (6, 6), (7, 7), (8, 8)])));
    }

    #[test]
    fn moves_never_put_two_blocks_in_a_column() {
        let current = layout(&[(1, 10), (2, 20), (3, 30)]);
        let target = layout(&[(2, 10), (3, 20), (4, 30)]);
        assert_eq!(plan_moves(&current, &target), vec![(3, 4), (2, 3), (1, 2)]);
        let swap = plan_moves(&layout(&[(1, 10), (13, 30)]), &layout(&[(3, 10), (11, 30)]));
        let mut at = layout(&[(1, 10), (13, 30)]);
        for (from, to) in &swap {
            let id = at.remove(from).unwrap();
            assert!(at.keys().all(|p| column_of(*p) != column_of(*to)), "{to} shares a column after {swap:?}");
            at.insert(*to, id);
        }
        assert_eq!(at, layout(&[(3, 10), (11, 30)]));
    }

    fn block(position: usize, preset_id: u64, params: Vec<Value>, enabled: bool) -> Block {
        Block { position, preset_id, cab: None, kind: Some(8), enabled, params, extras: vec![], ir_hash: None }
    }

    #[test]
    fn reconcile_sends_only_what_differs() {
        let defaults = |_| Some(vec![Value::F32(0.5), Value::F32(0.5)]);
        let current = vec![block(1, 80, vec![Value::F32(0.9), Value::F32(0.5)], true), block(3, 92, vec![Value::F32(0.5), Value::F32(0.5)], true)];
        let target = vec![block(1, 80, vec![Value::F32(0.2), Value::F32(0.5)], false), block(4, 92, vec![Value::F32(0.5), Value::F32(0.7)], true)];
        let ops = reconcile(&current, &target, defaults).unwrap();
        assert_eq!(
            ops,
            vec![
                Op::Remove { position: 3 },
                Op::Set { position: 1, index: 0, value: Value::F32(0.2) },
                Op::Enable { position: 1, on: false },
                Op::Add { position: 4, preset_id: 92, cab: None, kind: 8 },
                Op::Set { position: 4, index: 1, value: Value::F32(0.7) },
            ]
        );
        let back = reconcile(&[], &[block(2, 80, vec![Value::F32(0.5), Value::F32(0.5)], false)], defaults).unwrap();
        assert_eq!(back, vec![Op::Add { position: 2, preset_id: 80, cab: None, kind: 8 }, Op::Enable { position: 2, on: false }], "a re-added block keeps its off state");
    }
}
