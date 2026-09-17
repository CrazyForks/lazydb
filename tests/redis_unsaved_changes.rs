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
    model::{
        redis_browser::{RedisBrowserTab, RedisValuePageState},
        redis_key_tree::KeyTreeNodeId,
        tab::WorkspaceTab,
        workspace::Overlay,
    },
};
use uuid::Uuid;

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
