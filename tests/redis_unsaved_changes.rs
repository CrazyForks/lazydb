use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
    model::execution_target::ExecutionTarget,
    model::{
        redis_browser::{RedisBrowserTab, RedisValuePageState},
        redis_key_tree::KeyTreeNodeId,
        tab::WorkspaceTab,
        workspace::{ConnectionStatus, Focus, Overlay},
    },
};
use uuid::Uuid;

fn collection_cases() -> Vec<(RedisType, RedisPageValue)> {
    vec![
        (
            RedisType::Hash,
            RedisPageValue::Hash(vec![(b"field".to_vec(), b"value".to_vec())]),
        ),
        (
            RedisType::List,
            RedisPageValue::List(vec![(0, b"value".to_vec())]),
        ),
        (
            RedisType::Set,
            RedisPageValue::Set(vec![b"member".to_vec()]),
        ),
        (
            RedisType::SortedSet,
            RedisPageValue::SortedSet(vec![(b"member".to_vec(), b"1".to_vec())]),
        ),
        (
            RedisType::Stream,
            RedisPageValue::Stream(vec![(
                b"1-0".to_vec(),
                vec![(b"field".to_vec(), b"value".to_vec())],
            )]),
        ),
    ]
}

fn preview_fixture() -> (App, Uuid, RedisKeyId, RedisKeyId) {
    let profile_id = Uuid::from_u128(20);
    let target = RedisTarget {
        profile_id,
        database: 0,
    };
    let string_key = RedisKeyId {
        target: target.clone(),
        key: b"string".to_vec(),
    };
    let collection_key = RedisKeyId {
        target: target.clone(),
        key: b"collection".to_vec(),
    };
    let next_key = RedisKeyId {
        target: target.clone(),
        key: b"next".to_vec(),
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
    let mut tab = RedisBrowserTab::new(Uuid::from_u128(21), target);
    let tab_id = tab.id;
    tab.tree
        .rebuild(&[string_key.clone(), collection_key.clone(), next_key.clone()]);
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = 0;
    app.focus = Focus::Results;
    app.overlay = None;
    load_preview(
        &mut app,
        tab_id,
        string_key,
        RedisType::String,
        RedisPageValue::String(b"baseline".to_vec()),
    );
    (app, tab_id, collection_key, next_key)
}

fn load_preview(
    app: &mut App,
    tab_id: Uuid,
    key: RedisKeyId,
    value_type: RedisType,
    value: RedisPageValue,
) {
    app.open_redis_key(tab_id, KeyTreeNodeId::Key(key.key.clone()));
    let preview_generation = match app.tabs.iter().find(|tab| tab.id() == tab_id) {
        Some(WorkspaceTab::RedisBrowser(tab)) => tab.preview_generation,
        _ => unreachable!(),
    };
    app.update(Action::RedisValuePageLoaded {
        tab_id,
        connection: app.connection.active_identity().unwrap(),
        preview_generation,
        page: RedisValuePage {
            metadata: RedisKeyMetadata {
                key,
                value_type,
                ttl: TtlState::Persistent,
                memory_usage_bytes: None,
                value_size: None,
            },
            position: RedisPagePosition::Complete,
            value,
            truncated: false,
            complete: true,
            raw_bytes: 1,
            formatted_bytes: 1,
        },
    });
}

fn dirty_fixture() -> (App, Uuid, RedisKeyId) {
    let mut app = App::new(Vec::new());
    let target = RedisTarget {
        profile_id: Uuid::new_v4(),
        database: 0,
    };
    let old = RedisKeyId {
        target: target.clone(),
        key: b"old".to_vec(),
    };
    let next = RedisKeyId {
        target: target.clone(),
        key: b"next".to_vec(),
    };
    let mut tab = RedisBrowserTab::new(Uuid::new_v4(), target);
    let id = tab.id;
    tab.tree.rebuild(&[old.clone(), next.clone()]);
    tab.open_key(old.clone());
    tab.value_page = RedisValuePageState::Ready(RedisValuePage {
        metadata: RedisKeyMetadata {
            key: old,
            value_type: RedisType::String,
            ttl: TtlState::Persistent,
            memory_usage_bytes: None,
            value_size: Some(4),
        },
        position: RedisPagePosition::Complete,
        value: RedisPageValue::String(b"base".to_vec()),
        truncated: false,
        complete: true,
        raw_bytes: 4,
        formatted_bytes: 4,
    });
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = app.tabs.len() - 1;
    app.overlay = None;
    app.update(Action::RedisPreviewEdit);
    app.update(Action::EditorKey(KeyEvent::new(
        KeyCode::Char('i'),
        KeyModifiers::NONE,
    )));
    app.update(Action::EditorPaste("changed".into()));
    (app, id, next)
}

#[test]
fn discard_key_switch_opens_destination_without_prompting_again() {
    let (mut app, id, next) = dirty_fixture();
    app.open_redis_key(id, KeyTreeNodeId::Key(next.key.clone()));
    assert!(matches!(
        app.overlay,
        Some(Overlay::RedisUnsavedValueConfirm { .. })
    ));
    app.update(Action::RedisUnsavedValueDiscard);
    assert!(
        app.overlay.is_none(),
        "discard must resolve dirty state before replay"
    );
    assert!(
        matches!(&app.tabs[app.active_tab], WorkspaceTab::RedisBrowser(tab) if tab.opened_key.as_ref() == Some(&next))
    );
}

#[test]
fn cancel_key_switch_preserves_the_dirty_document() {
    let (mut app, id, next) = dirty_fixture();
    app.open_redis_key(id, KeyTreeNodeId::Key(next.key.clone()));
    app.update(Action::RedisUnsavedValueCancel);
    assert!(
        matches!(&app.tabs[app.active_tab], WorkspaceTab::RedisBrowser(tab) if tab.opened_key.as_ref().unwrap().key == b"old")
    );
    app.open_redis_key(id, KeyTreeNodeId::Key(next.key));
    assert!(matches!(
        app.overlay,
        Some(Overlay::RedisUnsavedValueConfirm { .. })
    ));
}

#[test]
fn collection_preview_after_string_allows_key_switch_without_unsaved_prompt() {
    for (value_type, value) in collection_cases() {
        let (mut app, tab_id, collection_key, next_key) = preview_fixture();
        load_preview(&mut app, tab_id, collection_key.clone(), value_type, value);
        let clean = match app.tabs.iter().find(|tab| tab.id() == tab_id) {
            Some(WorkspaceTab::RedisBrowser(tab)) => {
                assert!(tab.value_edit_baseline.is_none());
                assert_eq!(tab.format.view(), lazydb::value_preview::ValueView::Table);
                !tab.value_is_dirty("any collection preview text")
            }
            _ => unreachable!(),
        };
        assert!(clean, "{value_type:?} collection should be clean");

        app.open_redis_key(tab_id, KeyTreeNodeId::Key(next_key.key.clone()));
        assert!(!matches!(
            app.overlay,
            Some(Overlay::RedisUnsavedValueConfirm { .. })
        ));
        assert!(matches!(
            app.tabs.iter().find(|tab| tab.id() == tab_id),
            Some(WorkspaceTab::RedisBrowser(tab)) if tab.opened_key.as_ref() == Some(&next_key)
        ));
    }
}

#[test]
fn collection_preview_after_string_allows_quit_without_unsaved_prompt() {
    for (value_type, value) in collection_cases() {
        let (mut app, tab_id, collection_key, _) = preview_fixture();
        load_preview(&mut app, tab_id, collection_key, value_type, value);

        app.update(Action::Quit);
        assert!(!matches!(
            app.overlay,
            Some(Overlay::RedisUnsavedValueConfirm { .. })
        ));
    }
}
