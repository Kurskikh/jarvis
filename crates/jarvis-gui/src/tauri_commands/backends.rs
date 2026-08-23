use jarvis_core::models::{self, BackendOption, Task};

// options for one backend picker, straight from the model registry.
// BackendOption already derives Serialize, so it goes over IPC as-is.
//
// infallible by design: an Err here would reject the promise and, at the
// frontend's Promise.all, abort the whole settings load. an empty list is
// rendered as an explicit "no backends" alert instead.
#[tauri::command]
pub fn list_backend_options(task: Task) -> Vec<BackendOption> {
    models::get_options(task)
}
