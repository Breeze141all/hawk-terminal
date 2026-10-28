//! Undo, redo and keyboard nudge for drawings.
//!
//! Split out of `screen/dashboard/pane.rs` (code unchanged apart from visibility).

use super::*;

impl State {
    pub(super) fn apply_undo_action(
        pane_id: uuid::Uuid,
        selected_drawing_show_settings: &mut bool,
        c: &mut crate::chart::kline::KlineChart,
        symbol: &str,
    ) -> Option<Effect> {
        if let Some(undone) = c.undo() {
            match undone {
                crate::chart::kline::DrawingHistoryAction::Add(d) => {
                    data::DrawingStore::remove(d.id);
                    *selected_drawing_show_settings = false;
                    if d.is_synced {
                        return Some(Effect::DeleteSyncedDrawing {
                            symbol: symbol.to_string(),
                            drawing_id: d.id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::Delete(d) => {
                    data::DrawingStore::add(symbol, d.clone());
                    if d.is_synced {
                        return Some(Effect::SyncDrawing {
                            symbol: symbol.to_string(),
                            drawing: d,
                            originating_pane_id: pane_id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::Update { old, new } => {
                    data::DrawingStore::update(symbol, old.clone());
                    if old.is_synced || new.is_synced {
                        return Some(Effect::SyncDrawing {
                            symbol: symbol.to_string(),
                            drawing: old,
                            originating_pane_id: pane_id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::BatchUpdate(pairs) => {
                    let mut synced = Vec::new();
                    for (old, new) in pairs {
                        data::DrawingStore::update(symbol, old.clone());
                        if old.is_synced || new.is_synced {
                            synced.push(old);
                        }
                    }
                    if !synced.is_empty() {
                        return Some(Effect::SyncDrawings {
                            symbol: symbol.to_string(),
                            drawings: synced,
                            originating_pane_id: pane_id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::Clear(drawings) => {
                    let mut synced = Vec::new();
                    for d in drawings {
                        data::DrawingStore::add(symbol, d.clone());
                        if d.is_synced {
                            synced.push(d);
                        }
                    }
                    if !synced.is_empty() {
                        return Some(Effect::SyncDrawings {
                            symbol: symbol.to_string(),
                            drawings: synced,
                            originating_pane_id: pane_id,
                        });
                    }
                }
            }
        }
        None
    }

    pub(super) fn apply_redo_action(
        pane_id: uuid::Uuid,
        selected_drawing_show_settings: &mut bool,
        c: &mut crate::chart::kline::KlineChart,
        symbol: &str,
    ) -> Option<Effect> {
        if let Some(redone) = c.redo() {
            match redone {
                crate::chart::kline::DrawingHistoryAction::Add(d) => {
                    data::DrawingStore::add(symbol, d.clone());
                    if d.is_synced {
                        return Some(Effect::SyncDrawing {
                            symbol: symbol.to_string(),
                            drawing: d,
                            originating_pane_id: pane_id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::Delete(d) => {
                    data::DrawingStore::remove(d.id);
                    *selected_drawing_show_settings = false;
                    if d.is_synced {
                        return Some(Effect::DeleteSyncedDrawing {
                            symbol: symbol.to_string(),
                            drawing_id: d.id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::Update { new, .. } => {
                    data::DrawingStore::update(symbol, new.clone());
                    if new.is_synced {
                        return Some(Effect::SyncDrawing {
                            symbol: symbol.to_string(),
                            drawing: new,
                            originating_pane_id: pane_id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::BatchUpdate(pairs) => {
                    let mut synced = Vec::new();
                    for (_, new) in pairs {
                        data::DrawingStore::update(symbol, new.clone());
                        if new.is_synced {
                            synced.push(new);
                        }
                    }
                    if !synced.is_empty() {
                        return Some(Effect::SyncDrawings {
                            symbol: symbol.to_string(),
                            drawings: synced,
                            originating_pane_id: pane_id,
                        });
                    }
                }
                crate::chart::kline::DrawingHistoryAction::Clear(drawings) => {
                    let mut synced_ids = Vec::new();
                    for d in &drawings {
                        data::DrawingStore::remove(d.id);
                        if d.is_synced {
                            synced_ids.push(d.id);
                        }
                    }
                    *selected_drawing_show_settings = false;
                    if !synced_ids.is_empty() {
                        return Some(Effect::ClearSyncedDrawings {
                            symbol: symbol.to_string(),
                            drawing_ids: synced_ids,
                        });
                    }
                }
            }
        }
        None
    }

    pub(super) fn apply_nudge_action(
        pane_id: uuid::Uuid,
        c: &mut crate::chart::kline::KlineChart,
        symbol: &str,
        time_delta: i64,
        price_delta: f32,
    ) -> Option<Effect> {
        let result = c.nudge_selected_drawings(time_delta, price_delta);
        if let Some((updated_drawings, synced_drawings)) = result {
            for d in updated_drawings {
                data::DrawingStore::update(symbol, d);
            }
            if !synced_drawings.is_empty() {
                return Some(Effect::SyncDrawings {
                    symbol: symbol.to_string(),
                    drawings: synced_drawings,
                    originating_pane_id: pane_id,
                });
            }
        }
        None
    }
}
