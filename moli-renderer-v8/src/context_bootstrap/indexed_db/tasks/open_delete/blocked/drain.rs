use super::*;

pub(in crate::context_bootstrap::indexed_db) fn flush_drain_blocked_open_requests_task(
    scope: &mut v8::PinScope<'_, '_>,
) {
    let Some(queue) = indexed_db_runtime_array(scope, IndexedDbRuntimeArray::BlockedOpenQueue)
    else {
        return;
    };
    let next = v8::Array::new(scope, 0);
    let mut blocked_keys = std::collections::HashSet::new();
    for index in 0..queue.length() {
        let Some(value) = queue.get_index(scope, index) else {
            continue;
        };
        let Ok(task) = v8::Local::<v8::Object>::try_from(value) else {
            continue;
        };
        let database_key = indexed_db_blocked_task_payload(scope, task)
            .map(|payload| database_registry_key(&payload.origin, &payload.name));
        let waiting_for_older = database_key
            .as_ref()
            .is_some_and(|key| blocked_keys.contains(key));
        if waiting_for_older || !try_execute_unblocked_request(scope, task) {
            if let Some(key) = database_key {
                blocked_keys.insert(key);
            }
            let _ = next.set_index(scope, next.length(), task.into());
        } else {
            if let Some(database_key) = database_key.as_deref() {
                let owner = indexed_db_typed_task_execution_owner(scope, task)
                    .expect("drained blocked task must retain its IndexedDB execution owner");
                unregister_blocked_database_context(scope, database_key, owner);
            }
            unregister_indexed_db_task(scope, task);
        }
    }
    replace_indexed_db_runtime_array(scope, IndexedDbRuntimeArray::BlockedOpenQueue, next);
}

fn try_execute_unblocked_request<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    task: v8::Local<'s, v8::Object>,
) -> bool {
    let owner = indexed_db_typed_task_owner_scope(scope, task)
        .expect("IDB blocked open/delete task should have typed owner state");
    let owner_restore = owner.enter(scope);
    let executed = try_execute_unblocked_request_in_owner_scope(scope, task);
    owner.defer_restore(scope, owner_restore);
    executed
}

fn try_execute_unblocked_request_in_owner_scope<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    task: v8::Local<'s, v8::Object>,
) -> bool {
    let Some(payload) = indexed_db_blocked_task_payload(scope, task) else {
        return false;
    };
    let key = database_registry_key(&payload.origin, &payload.name);
    if has_open_database_connections_for_key(scope, &key) {
        if !object_bool_property(
            scope,
            payload.request,
            INDEXED_DB_REQUEST_BLOCKED_DISPATCHED_SLOT,
        )
        .unwrap_or(false)
        {
            dispatch_version_change_to_open_connections(
                scope,
                &key,
                payload.old_version,
                payload.new_version,
            );
        }
        if has_open_database_connections_for_key(scope, &key) {
            event::dispatch_blocked_once(
                scope,
                payload.request,
                payload.old_version,
                payload.new_version,
            );
            return false;
        }
    }
    let Some(kind) = indexed_db_typed_task_kind(scope, task) else {
        return false;
    };
    match kind {
        IndexedDbTaskKind::OpenBlocked => {
            let Some(storage_scope) = blocked_task_storage_scope(scope, task) else {
                return false;
            };
            open::execute_open_request(
                scope,
                payload.request,
                storage_scope,
                payload.name,
                payload.version,
            );
        }
        IndexedDbTaskKind::DeleteBlocked => {
            let Some(storage_scope) = blocked_task_storage_scope(scope, task) else {
                return false;
            };
            delete::execute_delete_database_request(
                scope,
                payload.request,
                storage_scope,
                payload.name,
            );
        }
        _ => {}
    }
    true
}
