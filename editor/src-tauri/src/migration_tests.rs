//! 迁移边界的回归检查：直接使用 Rust 画笔、真实 Engine 和 IPC 数据类型。
//! 不启动窗口，也不把这些检查视为原生对话框或 WebView 端到端测试。

use std::collections::BTreeSet;

use pixauro_engine::{Color, ColorId, Engine, ProjectOptions};
use serde_json::json;

use crate::{
    editor_commands::apply_stroke_coverage,
    editor_protocol::{DocumentGuard, DocumentSnapshot},
    editor_session::{AppState, Session},
    tools::BrushStroke,
};

fn engine(width: u16, height: u16) -> Engine {
    Engine::new(ProjectOptions {
        name: "迁移回归".into(),
        width,
        height,
        palette: vec![Color::rgba(120, 60, 30, 128)],
    })
    .unwrap()
}

fn color_id(engine: &Engine) -> ColorId {
    engine.project().palette().entries().next().unwrap().0
}

fn painted_positions(engine: &Engine) -> BTreeSet<(i32, i32)> {
    let canvas = engine.project().canvas();
    let width = usize::from(canvas.width());
    canvas
        .pixels()
        .iter()
        .enumerate()
        .filter_map(|(offset, pixel)| {
            pixel.map(|_| ((offset % width) as i32, (offset / width) as i32))
        })
        .collect()
}

#[test]
fn skipped_pointer_points_are_interpolated_in_both_directions() {
    let mut engine = engine(8, 8);
    let mut stroke =
        BrushStroke::new(engine.project(), (1, 1), 1, Some(color_id(&engine))).unwrap();
    stroke.append(Some((5, 5))).unwrap();
    stroke.append(Some((1, 5))).unwrap();
    stroke.append(Some((1, 1))).unwrap();

    assert!(engine.execute(stroke.into_command()).unwrap());
    let expected = (1..=5)
        .flat_map(|n| [(n, n), (n, 5), (1, n)])
        .collect::<BTreeSet<_>>();
    assert_eq!(painted_positions(&engine), expected);
}

#[test]
fn invalid_brush_inputs_leave_the_document_and_pending_coverage_unchanged() {
    let mut engine = engine(6, 6);
    let target = Some(color_id(&engine));
    let initial = engine.status();
    assert!(BrushStroke::new(engine.project(), (2, 2), 0, target).is_err());
    let mut stroke = BrushStroke::new(engine.project(), (2, 2), 1, target).unwrap();
    for outside in [(-1, 2), (2, -1), (6, 2), (2, 6), (i32::MAX, i32::MIN)] {
        assert!(BrushStroke::new(engine.project(), outside, 1, target).is_err());
        assert!(stroke.append(Some(outside)).is_err());
    }
    assert_eq!(engine.status(), initial);
    assert!(painted_positions(&engine).is_empty());
    assert!(!stroke.append(Some((2, 2))).unwrap());
    engine.execute(stroke.into_command()).unwrap();
    assert_eq!(painted_positions(&engine), BTreeSet::from([(2, 2)]));
}

#[test]
fn leaving_and_reentering_the_canvas_does_not_bridge_the_gap() {
    let mut engine = engine(9, 5);
    let mut stroke =
        BrushStroke::new(engine.project(), (1, 2), 1, Some(color_id(&engine))).unwrap();
    assert!(!stroke.append(None).unwrap());
    assert!(!stroke.append(None).unwrap());
    stroke.append(Some((5, 2))).unwrap();
    stroke.append(Some((7, 2))).unwrap();

    engine.execute(stroke.into_command()).unwrap();
    assert_eq!(
        painted_positions(&engine),
        BTreeSet::from([(1, 2), (5, 2), (6, 2), (7, 2)])
    );
}

#[test]
fn even_brush_sizes_keep_right_down_bias_and_clip_at_canvas_edges() {
    for (size, point, min, max) in [
        (2, (2, 2), 2, 3),
        (4, (2, 2), 1, 4),
        (4, (0, 0), 0, 2),
        (4, (5, 5), 4, 5),
        (8, (0, 0), 0, 4),
    ] {
        let mut engine = engine(6, 6);
        let stroke =
            BrushStroke::new(engine.project(), point, size, Some(color_id(&engine))).unwrap();
        engine.execute(stroke.into_command()).unwrap();
        let expected = (min..=max)
            .flat_map(|y| (min..=max).map(move |x| (x, y)))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            painted_positions(&engine),
            expected,
            "size={size}, {point:?}"
        );
    }
}

#[test]
fn a_whole_stroke_is_one_undo_step_and_redo_restores_exact_pixels() {
    let mut engine = engine(8, 8);
    let target = Some(color_id(&engine));
    let initial = engine.status();
    let mut stroke = BrushStroke::new(engine.project(), (1, 1), 2, target).unwrap();
    stroke.append(Some((5, 5))).unwrap();
    stroke.append(Some((1, 1))).unwrap();
    assert!(!stroke.append(Some((1, 1))).unwrap());
    assert_eq!(engine.status(), initial);
    assert!(painted_positions(&engine).is_empty());

    assert!(engine.execute(stroke.into_command()).unwrap());
    let painted = engine.project().canvas().pixels().to_vec();
    assert_eq!(engine.history().entries().len(), 1);
    assert_eq!(engine.history().position(), 1);
    assert_eq!(engine.status().revision, initial.revision + 1);
    assert_eq!(
        engine.project().color_at((3, 3)).unwrap(),
        Some(Color::rgba(120, 60, 30, 128))
    );
    assert!(engine.undo().unwrap());
    assert!(painted_positions(&engine).is_empty());
    assert_eq!(engine.history().position(), 0);
    assert!(!engine.undo().unwrap());
    assert!(engine.redo().unwrap());
    assert_eq!(engine.project().canvas().pixels(), painted);
    assert_eq!(engine.history().entries().len(), 1);
    assert!(!engine.redo().unwrap());

    let status = engine.status();
    let unchanged = BrushStroke::new(engine.project(), (1, 1), 2, target).unwrap();
    assert!(!engine.execute(unchanged.into_command()).unwrap());
    assert_eq!(engine.status(), status);
    assert_eq!(engine.history().entries().len(), 1);
}

#[test]
fn reopening_the_same_project_rejects_the_previous_session_guard() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.json");
    let mut original = engine(4, 4);
    original.save(&path).unwrap();
    let original = Session::new(original, Some(path.clone()));
    let guard = DocumentGuard {
        session_id: original.id.clone(),
        revision: original.engine.status().revision.to_string(),
    };
    let reopened = Session::new(Engine::load(&path).unwrap(), Some(path));
    assert_eq!(
        original.engine.project().id(),
        reopened.engine.project().id()
    );
    assert_eq!(
        original.engine.status().revision,
        reopened.engine.status().revision
    );
    assert_ne!(original.id, reopened.id);

    let state = AppState::new(directory.path().join("recent.json"));
    let mut editor = state.editor.lock().unwrap();
    editor.session = Some(reopened);
    assert!(editor.guarded_session(&guard).is_err());
    let current = editor.snapshot().document.unwrap();
    assert!(
        editor
            .guarded_session(&DocumentGuard {
                session_id: current.session_id,
                revision: current.revision,
            })
            .is_ok()
    );
}

#[test]
fn undoing_to_the_same_pixels_still_rejects_an_old_revision_guard() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(directory.path().join("recent.json"));
    let mut editor = state.editor.lock().unwrap();
    let mut session = Session::new(engine(4, 4), None);
    let guard = DocumentGuard {
        session_id: session.id.clone(),
        revision: session.engine.status().revision.to_string(),
    };
    assert!(editor.guarded_session(&guard).is_err());
    let stroke = BrushStroke::new(
        session.engine.project(),
        (1, 1),
        1,
        Some(color_id(&session.engine)),
    )
    .unwrap();
    session.engine.execute(stroke.into_command()).unwrap();
    session.engine.undo().unwrap();
    assert!(painted_positions(&session.engine).is_empty());
    editor.session = Some(session);
    assert!(editor.guarded_session(&guard).is_err());
    let current_guard = DocumentGuard {
        session_id: guard.session_id,
        revision: "2".into(),
    };
    assert!(editor.guarded_session(&current_guard).is_ok());
}

#[test]
fn ipc_keeps_large_color_ids_and_revisions_as_exact_decimal_strings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large-id.json");
    engine(2, 2).save(&path).unwrap();
    // 从真实存档构造超出 JavaScript 安全整数范围的有效颜色 ID。
    let large_id = 9_007_199_254_740_993_u64;
    let mut file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    file["palette"]["colors"][0]["id"] = json!(large_id);
    file["palette"]["next_id"] = json!(large_id + 1);
    file["canvas"]["pixels"][0] = json!(large_id);
    std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
    let session = Session::new(Engine::load(&path).unwrap(), None);
    let id = large_id.to_string();
    assert_eq!(session.color_id(&id).unwrap().to_string(), id);
    assert!(session.color_id("9007199254740992").is_err());

    let snapshot = DocumentSnapshot::new(&session.engine, &session.id, None);
    let wire = serde_json::to_value(snapshot).unwrap();
    assert_eq!(wire["palette"][0]["id"], json!(id));
    assert_eq!(wire["pixels"][0], json!(id));
    assert_eq!(wire["palette"][0]["usage"], json!(1));
    assert_eq!(wire["revision"], json!("0"));
    let guard = DocumentGuard {
        session_id: session.id,
        revision: u64::MAX.to_string(),
    };
    let wire = serde_json::to_value(&guard).unwrap();
    assert_eq!(wire["revision"], json!("18446744073709551615"));
    let round_trip: DocumentGuard = serde_json::from_value(wire).unwrap();
    assert_eq!(round_trip.revision, guard.revision);
}

#[test]
fn malformed_stroke_coverage_is_rejected_without_mutation() {
    let mut session = Session::new(engine(3, 3), None);
    let target = color_id(&session.engine).to_string();
    // Keep existing pixels/history to prove validation cannot partially alter either.
    assert!(apply_stroke_coverage(&mut session, Some(&target), &[1, 0]).unwrap());
    let initial = session.engine.status();
    let pixels = session.engine.project().canvas().pixels().to_vec();
    for coverage in [
        &[][..],
        &[1][..],
        &[1, 0, 0][..],
        &[1, 2][..],
        &[1, 128][..],
    ] {
        assert!(apply_stroke_coverage(&mut session, None, coverage).is_err());
        assert_eq!(session.engine.status(), initial);
        assert_eq!(session.engine.project().canvas().pixels(), pixels);
        assert_eq!(session.engine.history().entries().len(), 1);
    }
    assert!(apply_stroke_coverage(&mut session, Some("missing"), &[255, 1]).is_err());
    assert_eq!(session.engine.status(), initial);
    assert_eq!(session.engine.project().canvas().pixels(), pixels);
}

#[test]
fn coverage_bits_are_row_major_little_endian_and_commit_one_undo_step() {
    let mut session = Session::new(engine(3, 3), None);
    let target = color_id(&session.engine).to_string();
    let initial = session.engine.status();
    let coverage = [0b1001_0001, 0b0000_0001];
    assert!(apply_stroke_coverage(&mut session, Some(&target), &coverage).unwrap());
    assert_eq!(
        painted_positions(&session.engine),
        BTreeSet::from([(0, 0), (1, 1), (1, 2), (2, 2)])
    );
    let painted = session.engine.project().canvas().pixels().to_vec();
    assert_eq!(session.engine.history().entries().len(), 1);
    assert_eq!(session.engine.history().position(), 1);
    assert_eq!(session.engine.status().revision, initial.revision + 1);
    assert!(session.engine.undo().unwrap());
    assert!(painted_positions(&session.engine).is_empty());
    assert!(!session.engine.undo().unwrap());
    assert!(session.engine.redo().unwrap());
    assert_eq!(session.engine.project().canvas().pixels(), painted);
    assert_eq!(session.engine.history().entries().len(), 1);

    let unchanged = session.engine.status();
    assert!(!apply_stroke_coverage(&mut session, Some(&target), &coverage).unwrap());
    assert!(!apply_stroke_coverage(&mut session, None, &[0, 0]).unwrap());
    assert_eq!(session.engine.status(), unchanged);
    assert!(apply_stroke_coverage(&mut session, None, &coverage).unwrap());
    assert!(painted_positions(&session.engine).is_empty());
    assert_eq!(session.engine.history().entries().len(), 2);
    assert!(session.engine.undo().unwrap());
    assert_eq!(session.engine.project().canvas().pixels(), painted);
}

#[test]
fn byte_aligned_canvas_accepts_all_eight_coverage_bits() {
    let mut session = Session::new(engine(4, 2), None);
    let target = color_id(&session.engine).to_string();
    assert!(apply_stroke_coverage(&mut session, Some(&target), &[255]).unwrap());
    assert_eq!(painted_positions(&session.engine).len(), 8);
    assert_eq!(session.engine.history().entries().len(), 1);
    assert!(session.engine.undo().unwrap());
    assert!(painted_positions(&session.engine).is_empty());
}
