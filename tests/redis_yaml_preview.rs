use lazydb::{
    action::Action,
    app::App,
    db::redis::{
        read::{
            RedisKeyMetadata, RedisPagePosition, RedisPageValue, RedisType, RedisValuePage,
            TtlState,
        },
        types::{RedisKeyId, RedisTarget},
    },
    model::{
        execution_target::ExecutionTarget,
        redis_browser::{RedisBrowserTab, RedisValuePageState},
        redis_key_tree::KeyTreeNodeId,
        tab::WorkspaceTab,
        workspace::{ConnectionStatus, Focus, Overlay},
    },
    value_preview::{PreviewFormat, ValueView},
};
use uuid::Uuid;

const YAML: &[u8] = b"name: lazydb\nversion: 1\nfeatures:\n  - redis\n  - preview";

fn fixture(format: PreviewFormat, automatic: bool) -> (App, Uuid) {
    let profile_id = Uuid::from_u128(1);
    let target = RedisTarget {
        profile_id,
        database: 0,
    };
    let key = RedisKeyId {
        target: target.clone(),
        key: b"lazydb:test:yaml".to_vec(),
    };
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 1;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "0".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    app.tabs.clear();
    let mut tab = RedisBrowserTab::new(Uuid::from_u128(2), target);
    let tab_id = tab.id;
    if automatic {
        tab.format.reset_auto();
    } else {
        tab.format.select(format);
    }
    tab.tree.rebuild(std::slice::from_ref(&key));
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = 0;
    app.focus = Focus::Results;
    app.open_redis_key(tab_id, KeyTreeNodeId::Key(key.key.clone()));
    if automatic {
        if let WorkspaceTab::RedisBrowser(tab) = &mut app.tabs[0] {
            tab.format.reset_auto();
        }
    } else if let WorkspaceTab::RedisBrowser(tab) = &mut app.tabs[0] {
        tab.format.select(format);
    }
    let preview_generation = match &app.tabs[0] {
        WorkspaceTab::RedisBrowser(tab) => tab.preview_generation,
        _ => unreachable!(),
    };
    app.update(Action::RedisValuePageLoaded {
        tab_id,
        connection: app.connection.active_identity().unwrap(),
        preview_generation,
        page: RedisValuePage {
            metadata: RedisKeyMetadata {
                key,
                value_type: RedisType::String,
                ttl: TtlState::Persistent,
                memory_usage_bytes: None,
                value_size: Some(YAML.len() as u64),
            },
            position: RedisPagePosition::Complete,
            value: RedisPageValue::String(YAML.to_vec()),
            truncated: false,
            complete: true,
            raw_bytes: YAML.len(),
            formatted_bytes: YAML.len(),
        },
    });
    (app, tab_id)
}

fn preview_text(app: &App, tab_id: Uuid) -> String {
    let WorkspaceTab::RedisBrowser(tab) = app.tabs.iter().find(|tab| tab.id() == tab_id).unwrap()
    else {
        unreachable!()
    };
    app.editor_text(tab.preview_editor_id).unwrap()
}

#[test]
fn yaml_is_detected_and_loaded_as_multiline_text() {
    let (app, tab_id) = fixture(PreviewFormat::RAW, true);
    let WorkspaceTab::RedisBrowser(tab) = app.tabs.iter().find(|tab| tab.id() == tab_id).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(tab.format.view(), ValueView::Yaml);
    assert!(preview_text(&app, tab_id).contains("features:\n"));
}

#[test]
fn manual_yaml_selection_updates_editable_preview_and_baseline() {
    let (mut app, tab_id) = fixture(PreviewFormat::RAW, false);
    app.update(Action::RedisPreviewCycleFormat);
    app.overlay = Some(Overlay::RedisPreviewFormat { selected: 3 });
    app.update(Action::RedisPreviewFormatAccept);

    let text = preview_text(&app, tab_id);
    assert!(text.contains("name: lazydb\n"));
    assert!(!text.contains("\\x0a"));
    let WorkspaceTab::RedisBrowser(tab) = app.tabs.iter().find(|tab| tab.id() == tab_id).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(tab.format.view(), ValueView::Yaml);
    assert!(!tab.format.automatic);
    assert_eq!(tab.value_edit_baseline.as_deref(), Some(text.as_str()));
    assert!(!tab.value_is_dirty(&text));
}

#[test]
fn auto_selection_re_detects_yaml_without_reloading_key() {
    let (mut app, tab_id) = fixture(PreviewFormat::YAML, false);
    app.overlay = Some(Overlay::RedisPreviewFormat { selected: 0 });
    app.update(Action::RedisPreviewFormatAccept);

    let WorkspaceTab::RedisBrowser(tab) = app.tabs.iter().find(|tab| tab.id() == tab_id).unwrap()
    else {
        unreachable!()
    };
    assert!(tab.format.automatic);
    assert_eq!(tab.format.view(), ValueView::Yaml);
    assert!(preview_text(&app, tab_id).contains("features:\n"));
}

#[test]
fn yaml_detection_does_not_replace_json_or_plain_text_defaults() {
    assert_eq!(
        lazydb::value_preview::detect::default_format(br#"{"ok":true}"#, false),
        PreviewFormat::JSON
    );
    assert_eq!(
        lazydb::value_preview::detect::default_format(b"plain text", false),
        PreviewFormat::RAW
    );
}

#[test]
fn dirty_yaml_preview_rejects_format_change_without_losing_draft() {
    let (mut app, _tab_id) = fixture(PreviewFormat::RAW, false);
    assert!(
        matches!(&app.tabs[0], WorkspaceTab::RedisBrowser(tab) if tab.format.view() == ValueView::Raw)
    );
    let editor_id = match &app.tabs[0] {
        WorkspaceTab::RedisBrowser(tab) => tab.preview_editor_id,
        _ => unreachable!(),
    };
    app.update(Action::RedisPreviewEdit);
    app.update(Action::EditorKey(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('i'),
        crossterm::event::KeyModifiers::NONE,
    )));
    app.update(Action::EditorPaste("draft".into()));
    let before = app.editor_text(editor_id).unwrap();
    assert!(matches!(&app.tabs[0], WorkspaceTab::RedisBrowser(tab) if tab.value_is_dirty(&before)));
    app.overlay = Some(Overlay::RedisPreviewFormat { selected: 2 });
    app.update(Action::RedisPreviewFormatAccept);
    assert_eq!(app.editor_text(editor_id).unwrap(), before);
    let WorkspaceTab::RedisBrowser(tab) = &app.tabs[0] else {
        unreachable!()
    };
    assert_eq!(tab.format.view(), ValueView::Raw);
}

#[test]
fn formatted_yaml_is_multiline_and_raw_preserves_lossless_display() {
    let page = match fixture(PreviewFormat::RAW, false).0.tabs[0].clone() {
        WorkspaceTab::RedisBrowser(tab) => match tab.value_page {
            RedisValuePageState::Ready(page) => page,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let yaml = lazydb::ui::redis_value::format_page(&page, PreviewFormat::YAML).unwrap();
    assert!(yaml.contains("features:\n"));
    let raw = lazydb::ui::redis_value::format_page(&page, PreviewFormat::RAW).unwrap();
    assert!(raw.contains("\\x0a"));
}
