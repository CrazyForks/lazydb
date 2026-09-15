use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use uuid::Uuid;

use crate::model::{editor::EditorViewport, editor_language::EditorLanguage};

use super::EditorWorkspace;

fn java_like_value(logical_lines: usize) -> String {
    let mut value = String::from(
        "{\n  \"annotations\": [],\n  \"class\": \"org.example.GeneratedAuthorization\",\n  \"fields\": {\n",
    );
    for index in 0..logical_lines.saturating_sub(7) {
        value.push_str(&format!(
            "    \"field_{index:05}\": {{\"class\": \"java.util.Collections$UnmodifiableMap\", \"value\": {index}}},\n"
        ));
    }
    value.push_str("    \"tail\": true\n  }\n}\n");
    value
}

fn open_preview(text: &str) -> (EditorWorkspace, Uuid) {
    let id = Uuid::new_v4();
    let mut workspace = EditorWorkspace::new();
    workspace.open_read_only(id, text);
    (workspace, id)
}

fn percentile(mut samples: Vec<Duration>, percentile: usize) -> Duration {
    samples.sort_unstable();
    let index = samples
        .len()
        .saturating_mul(percentile)
        .saturating_div(100)
        .min(samples.len().saturating_sub(1));
    samples[index]
}

#[test]
fn java_like_preview_fixture_has_stable_shape() {
    let text = java_like_value(100);
    assert!(text.contains("GeneratedAuthorization"));
    assert_eq!(text.lines().count(), 100);
    assert!(text.len() > 4_000);
}

#[test]
fn wrapped_preview_baseline_preserves_navigation_and_source_text() {
    let text = java_like_value(100);
    let (mut workspace, id) = open_preview(&text);
    let viewport = EditorViewport {
        width: 48,
        height: 12,
    };

    let first = workspace
        .render_wrapped_preview_snapshot(id, viewport, EditorLanguage::Json, true)
        .unwrap();
    assert!(first.total_lines > first.lines.len());
    assert_eq!(first.logical_line_count, 101);
    assert!(first.cursor_screen_cell.is_some());

    workspace
        .key(
            id,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyModifiers::NONE,
            ),
        )
        .unwrap();
    let after = workspace
        .render_wrapped_preview_snapshot(id, viewport, EditorLanguage::Json, true)
        .unwrap();
    assert!(after.cursor_screen_cell.is_some());
    assert_eq!(workspace.text(id).unwrap(), text);
}

#[test]
#[ignore = "performance baseline; run explicitly in release mode"]
fn preview_navigation_baseline() {
    let viewport = EditorViewport {
        width: 80,
        height: 24,
    };
    let language = EditorLanguage::Json;

    for logical_lines in [100, 1_000, 10_000] {
        let text = java_like_value(logical_lines);
        let (mut workspace, id) = open_preview(&text);
        let cold_start = Instant::now();
        black_box(
            workspace
                .render_wrapped_preview_snapshot(id, viewport, language, true)
                .unwrap(),
        );
        let cold = cold_start.elapsed();

        let mut snapshot_samples = Vec::with_capacity(30);
        let mut navigation_samples = Vec::with_capacity(30);
        for index in 0..30 {
            let key = if index % 2 == 0 { 'j' } else { 'k' };
            let navigation_start = Instant::now();
            workspace
                .key(
                    id,
                    crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char(key),
                        crossterm::event::KeyModifiers::NONE,
                    ),
                )
                .unwrap();
            navigation_samples.push(navigation_start.elapsed());

            let snapshot_start = Instant::now();
            black_box(
                workspace
                    .render_wrapped_preview_snapshot(id, viewport, language, true)
                    .unwrap(),
            );
            snapshot_samples.push(snapshot_start.elapsed());
        }

        println!(
            "lines={logical_lines} bytes={} cold={cold:?} navigation_p95={:?} snapshot_p95={:?}",
            text.len(),
            percentile(navigation_samples, 95),
            percentile(snapshot_samples, 95),
        );
    }
}
