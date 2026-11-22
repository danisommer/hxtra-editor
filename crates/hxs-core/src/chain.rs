//! The signal chain's grid as an editor sees it: two rows of eight positions, inserting with
//! push, the moves that reach a layout, and the edits that turn one chain into another (undo).

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

/// `item` (now at `from`, or new) put at `target`. An empty target just takes it; on an
/// occupied one it lands right before the block that was there, which with the blocks after it
/// moves forward to the first free position; without room ahead, the blocks before it move back
/// instead. `None` when the row has no room at all.
pub fn insert(layout: &Layout, item: u64, from: Option<usize>, target: usize) -> Option<Layout> {
    let (row, i) = row_of(target)?;
    let mut out = layout.clone();
    if let Some(f) = from {
        out.remove(&f);
    }
    let mut slots: Vec<Option<u64>> = (0..ROW_LEN).map(|k| out.remove(&position(row, k))).collect();
    let ahead = (i + 1..ROW_LEN).find(|&j| slots[j].is_none());
    let behind = (0..i).rev().find(|&k| slots[k].is_none());
    match (slots[i], ahead, behind) {
        (None, _, _) => slots[i] = Some(item),
        (Some(_), Some(j), _) => {
            for k in (i + 1..=j).rev() {
                slots[k] = slots[k - 1];
            }
            slots[i] = Some(item);
        }
        (Some(_), None, Some(k)) => {
            for m in k..i - 1 {
                slots[m] = slots[m + 1];
            }
            slots[i - 1] = Some(item);
        }
        (Some(_), None, None) => return None,
    }
    out.extend(slots.into_iter().enumerate().filter_map(|(k, s)| s.map(|id| (position(row, k), id))));
    Some(out)
}

/// Moves to empty positions only (what the unit takes, editor 0x2b) that bring every item of
/// `target` found in `current` to its place; a cycle goes through a free position.
pub fn plan_moves(current: &Layout, target: &Layout) -> Vec<(usize, usize)> {
    let mut at: BTreeMap<u64, usize> = current.iter().map(|(p, id)| (*id, *p)).collect();
    let want: BTreeMap<u64, usize> = target.iter().filter(|(_, id)| at.contains_key(id)).map(|(p, id)| (*id, *p)).collect();
    let mut moves = Vec::new();
    for _ in 0..4 * all_positions().count() {
        let occupied: Vec<usize> = at.values().copied().collect();
        let misplaced: Vec<(u64, usize, usize)> = want.iter().filter(|(id, p)| at[id] != **p).map(|(id, p)| (*id, at[id], *p)).collect();
        let Some(&(first, from_first, _)) = misplaced.first() else { break };
        let (id, from, to) = match misplaced.iter().find(|(_, _, to)| !occupied.contains(to)) {
            Some(&m) => m,
            None => {
                let parking = all_positions().find(|p| !occupied.contains(p) && !want.values().any(|w| w == p));
                match parking {
                    Some(p) => (first, from_first, p),
                    None => break,
                }
            }
        };
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
    fn dropping_on_an_empty_position_just_moves() {
        assert_eq!(insert(&layout(&[(1, 10)]), 10, Some(1), 4), Some(layout(&[(4, 10)])));
    }

    #[test]
    fn dropping_on_a_block_inserts_before_it_and_pushes_forward() {
        let l = layout(&[(1, 10), (2, 20), (3, 30), (6, 60)]);
        assert_eq!(insert(&l, 99, None, 2), Some(layout(&[(1, 10), (2, 99), (3, 20), (4, 30), (6, 60)])));
        assert_eq!(insert(&l, 60, Some(6), 1), Some(layout(&[(1, 60), (2, 10), (3, 20), (4, 30)])), "the dragged block leaves its place first");
    }

    #[test]
    fn without_room_ahead_the_blocks_before_move_back() {
        let l = layout(&[(3, 30), (4, 40), (5, 50), (6, 60), (7, 70), (8, 80)]);
        assert_eq!(insert(&l, 99, None, 5), Some(layout(&[(2, 30), (3, 40), (4, 99), (5, 50), (6, 60), (7, 70), (8, 80)])));
        let full: Layout = (1..=8).map(|p| (p, p as u64)).collect();
        assert_eq!(insert(&full, 99, None, 4), None);
        assert_eq!(insert(&full, 3, Some(3), 6), Some(layout(&[(1, 1), (2, 2), (3, 4), (4, 5), (5, 3), (6, 6), (7, 7), (8, 8)])), "a block of the same row");
    }

    #[test]
    fn rows_are_independent() {
        let l = layout(&[(1, 10), (11, 110)]);
        assert_eq!(insert(&l, 10, Some(1), 11), Some(layout(&[(11, 10), (12, 110)])));
    }

    #[test]
    fn moves_go_through_free_positions() {
        let current = layout(&[(1, 10), (2, 20), (3, 30)]);
        let target = layout(&[(2, 10), (3, 20), (4, 30)]);
        let moves = plan_moves(&current, &target);
        assert_eq!(moves, vec![(3, 4), (2, 3), (1, 2)]);
        let swap = plan_moves(&layout(&[(1, 10), (2, 20)]), &layout(&[(1, 20), (2, 10)]));
        assert_eq!(swap.len(), 3, "a swap parks one block: {swap:?}");
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
