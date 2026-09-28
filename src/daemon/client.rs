use crate::daemon::protocol::{read_message, write_message, DaemonRequest, DaemonResponse};
use crate::domain::JvmCacheError;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DaemonClient {
    pub socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn is_alive(&self) -> bool {
        if !self.socket_path.exists() {
            return false;
        }
        match UnixStream::connect(&self.socket_path) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                true
            }
            Err(_) => false,
        }
    }

    pub fn send_request(&self, request: &DaemonRequest) -> Result<DaemonResponse, JvmCacheError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(Duration::from_secs(300)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;

        let json_bytes = serde_json::to_vec(request)?;
        write_message(&mut stream, &json_bytes)?;

        let resp_bytes = read_message(&mut stream)?;
        let response: DaemonResponse = serde_json::from_slice(&resp_bytes)?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_client_is_alive_false_on_missing_socket() {
        let client = DaemonClient::new(Path::new("/tmp/non_existent_jvmcache_test.sock"));
        assert!(!client.is_alive());
    }
}
