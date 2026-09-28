use crate::cas::copy_or_link;
use crate::constants::PARALLEL_STORE_MIN_FILES;
use crate::domain::JvmCacheError;
use std::path::PathBuf;
use std::thread::{available_parallelism, Builder, JoinHandle};

#[derive(Debug, Clone)]
pub struct RestorationTask {
    pub src: PathBuf,
    pub dst: PathBuf,
}

pub struct AsyncRestorationHandle {
    handles: Vec<JoinHandle<Result<(), JvmCacheError>>>,
}

impl AsyncRestorationHandle {
    pub fn empty() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    pub fn join(self) -> Result<(), JvmCacheError> {
        for handle in self.handles {
            match handle.join() {
                Ok(res) => res?,
                Err(_) => {
                    return Err(JvmCacheError::Execution(
                        "Background restoration worker thread panicked".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

pub struct AsyncRestorationQueue;

impl AsyncRestorationQueue {
    pub fn spawn_restoration(tasks: Vec<RestorationTask>) -> AsyncRestorationHandle {
        if tasks.is_empty() {
            return AsyncRestorationHandle::empty();
        }

        if tasks.len() <= PARALLEL_STORE_MIN_FILES {
            let handle = Builder::new()
                .name("jvmcache-bg-restore".to_string())
                .spawn(move || -> Result<(), JvmCacheError> {
                    for task in tasks {
                        copy_or_link(&task.src, &task.dst)?;
                    }
                    Ok(())
                })
                .expect("Failed to spawn background restoration thread");
            return AsyncRestorationHandle {
                handles: vec![handle],
            };
        }

        let num_threads = available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(tasks.len())
            .min(16);

        let chunk_size = tasks.len().div_ceil(num_threads);
        let mut handles = Vec::with_capacity(num_threads);

        for chunk in tasks.chunks(chunk_size) {
            let chunk_vec = chunk.to_vec();
            let handle = Builder::new()
                .name("jvmcache-bg-restore-worker".to_string())
                .spawn(move || -> Result<(), JvmCacheError> {
                    for task in chunk_vec {
                        copy_or_link(&task.src, &task.dst)?;
                    }
                    Ok(())
                })
                .expect("Failed to spawn background restoration thread");
            handles.push(handle);
        }

        AsyncRestorationHandle { handles }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_async_restoration_empty_tasks() {
        let handle = AsyncRestorationQueue::spawn_restoration(Vec::new());
        assert!(handle.join().is_ok());
    }

    #[test]
    fn test_async_restoration_propagates_worker_error() {
        let tmp = std::env::temp_dir().join(format!("jvmcache_async_err_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let tasks = vec![RestorationTask {
            src: tmp.join("non_existent_source.bin"),
            dst: tmp.join("dst.bin"),
        }];
        let handle = AsyncRestorationQueue::spawn_restoration(tasks);
        assert!(handle.join().is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_async_restoration_concurrent_safety_and_zero_collisions() {
        let tmp = std::env::temp_dir().join(format!("jvmcache_async_concurr_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let count = 64;
        let mut tasks = Vec::with_capacity(count);

        for i in 0..count {
            let src = tmp.join(format!("payload_{i}.dat"));
            let dst = tmp.join(format!("restored_{i}.dat"));
            fs::write(&src, format!("concurrent-content-{i}")).unwrap();
            tasks.push(RestorationTask { src, dst });
        }

        let handle = AsyncRestorationQueue::spawn_restoration(tasks.clone());
        assert!(handle.join().is_ok());

        for (i, task) in tasks.iter().enumerate() {
            assert!(task.dst.is_file());
            let content = fs::read_to_string(&task.dst).unwrap();
            assert_eq!(content, format!("concurrent-content-{i}"));
        }

        let _ = fs::remove_dir_all(&tmp);
    }
}
