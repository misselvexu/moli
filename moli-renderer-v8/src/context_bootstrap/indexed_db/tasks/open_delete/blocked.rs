use super::*;

mod drain;
mod event;

pub(in crate::context_bootstrap::indexed_db) fn flush_blocked_request_task<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    task: v8::Local<'s, v8::Object>,
) {
    let Some(payload) = indexed_db_blocked_task_payload(scope, task) else {
        return;
    };
    let key = database_registry_key(&payload.origin, &payload.name);
    push_unique_object_to_indexed_db_runtime_array(
        scope,
        IndexedDbRuntimeArray::BlockedOpenQueue,
        task,
    );
    let owner = indexed_db_typed_task_execution_owner(scope, task)
        .expect("blocked request must retain its IndexedDB execution owner");
    register_blocked_database_context(scope, key, owner);
    // Even if the last connection closed before this task ran, an older
    // request can still be waiting in the connection queue. Only its FIFO
    // drain may start work; a newly arrived task must not take a fast path
    // around that queue. This matters when upgrade and open success run in
    // separate tasks and both callbacks request a delete.
    enqueue_drain_blocked_open_requests_task(scope);
}

pub(super) fn blocked_task_storage_scope<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    task: v8::Local<'s, v8::Object>,
) -> Option<IndexedDbStorageScope> {
    indexed_db_typed_task_storage_scope(scope, task)
}

pub(in crate::context_bootstrap::indexed_db) use self::drain::flush_drain_blocked_open_requests_task;
