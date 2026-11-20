use crownos_config::system::TaskStatus;

pub fn label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Done => "done",
        TaskStatus::Failed { .. } => "failed",
    }
}

pub fn message(status: &TaskStatus) -> Option<&str> {
    match status {
        TaskStatus::Failed { message } => Some(message),
        _ => None,
    }
}
