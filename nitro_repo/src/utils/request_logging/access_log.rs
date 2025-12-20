use std::sync::Arc;

use parking_lot::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct AccessLogContext(Arc<Mutex<AccessLogData>>);

#[derive(Debug, Default)]
struct AccessLogData {
    repository_id: Option<Uuid>,
    user: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AccessLogSnapshot {
    pub repository_id: Option<Uuid>,
    pub user: Option<String>,
}

impl AccessLogContext {
    pub fn set_repository_id(&self, repository_id: Uuid) {
        self.0.lock().repository_id = Some(repository_id);
    }

    pub fn set_user(&self, user: impl Into<String>) {
        self.0.lock().user = Some(user.into());
    }

    pub fn snapshot(&self) -> AccessLogSnapshot {
        let locked = self.0.lock();
        AccessLogSnapshot {
            repository_id: locked.repository_id,
            user: locked.user.clone(),
        }
    }
}
