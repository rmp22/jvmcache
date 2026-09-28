use crate::domain::JvmCacheError;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonRequest {
    pub compiler: String,
    pub working_dir: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn write_message<W: Write>(w: &mut W, payload: &[u8]) -> Result<(), JvmCacheError> {
    let len = payload.len() as u32;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

pub fn read_message<R: Read>(r: &mut R) -> Result<Vec<u8>, JvmCacheError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 32 * 1024 * 1024 {
        return Err(JvmCacheError::Execution("Daemon message exceeds 32MB limit".to_string()));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_protocol_framing_roundtrip() {
        let req = DaemonRequest {
            compiler: "kotlinc".to_string(),
            working_dir: "/tmp".to_string(),
            args: vec!["Hello.kt".to_string(), "-d".to_string(), "out".to_string()],
        };

        let json_bytes = serde_json::to_vec(&req).unwrap();
        let mut buffer = Vec::new();
        write_message(&mut buffer, &json_bytes).unwrap();

        let mut cursor = Cursor::new(buffer);
        let read_bytes = read_message(&mut cursor).unwrap();
        let decoded: DaemonRequest = serde_json::from_slice(&read_bytes).unwrap();

        assert_eq!(req, decoded);
    }

    #[test]
    fn test_daemon_response_roundtrip() {
        let resp = DaemonResponse {
            exit_code: 0,
            stdout: "Compilation succeeded\n".to_string(),
            stderr: String::new(),
        };

        let json_bytes = serde_json::to_vec(&resp).unwrap();
        let mut buffer = Vec::new();
        write_message(&mut buffer, &json_bytes).unwrap();

        let mut cursor = Cursor::new(buffer);
        let read_bytes = read_message(&mut cursor).unwrap();
        let decoded: DaemonResponse = serde_json::from_slice(&read_bytes).unwrap();

        assert_eq!(resp, decoded);
    }
}
