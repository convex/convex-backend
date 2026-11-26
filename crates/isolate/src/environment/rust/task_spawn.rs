//! Spawns the shared [`TaskExecutor`] for a `"use rust"` action. Mirrors the
//! construction in `ActionEnvironment::new` (V8) without touching it.

use common::{
    runtime::{
        Runtime,
        SpawnHandle,
    },
    sync::spsc,
};
use tokio::sync::mpsc;

use crate::environment::action::{
    task::{
        TaskRequest,
        TaskResponse,
    },
    task_executor::TaskExecutor,
};

/// A running [`TaskExecutor`] plus the channels the wasm backend drives it by.
pub(super) struct SpawnedTaskExecutor {
    pub pending_task_sender: spsc::UnboundedSender<TaskRequest>,
    pub task_responses: mpsc::UnboundedReceiver<TaskResponse>,
    pub running_tasks: Box<dyn SpawnHandle>,
}

/// Builds an executor (given the sender for its task responses) and spawns
/// its request loop on `rt`.
pub(super) fn spawn_task_executor<RT: Runtime>(
    rt: &RT,
    build: impl FnOnce(mpsc::UnboundedSender<TaskResponse>) -> TaskExecutor<RT>,
) -> SpawnedTaskExecutor {
    let (task_retval_sender, task_responses) = mpsc::unbounded_channel();
    let task_executor = build(task_retval_sender);
    let (pending_task_sender, pending_task_receiver) = spsc::unbounded_channel();
    let running_tasks = rt.spawn("task_executor", task_executor.go(pending_task_receiver));
    SpawnedTaskExecutor {
        pending_task_sender,
        task_responses,
        running_tasks,
    }
}
