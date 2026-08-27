//! 审核中断状态落盘 — 让 Rust 引擎重启后的"中断审核"可被检测。
//!
//! ## 背景
//! 审核管线（extract→chunk→embed→review）整体在内存中执行。若进程在审核
//! 进行中重启，`AppState` 内 `documents` / `active_reviews` / `review_results`
//! 全部蒸发，而磁盘上只有"审核完成"才写入的 `{doc_id}_result.json`。
//! 此时 `GET /review/:doc_id/result` 返回 404，Java 侧无法区分
//! "从未审核"与"审核中断"，只能盲等超时。
//!
//! ## 设计
//! 在 `POST /review` 被接受（accepted）时，立即向 findings 目录写入
//! `{doc_id}_review_state.json`（status=running）。管线完成时删除，
//! 管线失败时改写为 status=failed + error。
//!
//! `GET /result` 的磁盘兜底因此可以：
//! 1. 有 `_result.json` → 恢复已完成结果（原逻辑）
//! 2. 无结果文件但有状态文件(running) → 判定"引擎重启中断"，返回 failed
//! 3. 状态文件(failed) → 返回原始错误
//! 4. 都没有 → 404
//!
//! 所有函数接受显式目录参数，便于用临时目录做单元测试。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 状态文件后缀，与结果文件 `_result.json` 同目录。
pub const STATE_FILE_SUFFIX: &str = "_review_state.json";

/// 审核中断的统一错误文案（Java 侧 failTask 后前端可见）。
pub const INTERRUPTED_ERROR_MSG: &str = "审核引擎重启导致审核中断，请重新发起审核";

/// 审核状态文件内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewStateFile {
    /// "running" | "failed"
    pub status: String,
    /// RFC3339 启动时间
    pub started_at: String,
    /// RFC3339 最近更新时间
    pub updated_at: String,
    /// 失败原因（仅 failed 时有值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ReviewStateFile {
    pub fn running(now: impl Fn() -> String) -> Self {
        Self {
            status: "running".to_string(),
            started_at: now(),
            updated_at: now(),
            error: None,
        }
    }

    pub fn failed(error: impl Into<String>, now: impl Fn() -> String) -> Self {
        Self {
            status: "failed".to_string(),
            started_at: now(),
            updated_at: now(),
            error: Some(error.into()),
        }
    }

    pub fn is_running(&self) -> bool {
        self.status == "running"
    }
}

/// 状态文件路径：`{dir}/{doc_id}_review_state.json`。
pub fn state_file_path(dir: &Path, doc_id: &str) -> PathBuf {
    dir.join(format!("{}{}", doc_id, STATE_FILE_SUFFIX))
}

/// 写入 running 状态（审核已接受、管线启动前）。
pub fn write_running(dir: &Path, doc_id: &str, now: impl Fn() -> String) -> std::io::Result<PathBuf> {
    let state = ReviewStateFile::running(now);
    write(&state_file_path(dir, doc_id), &state)?;
    Ok(state_file_path(dir, doc_id))
}

/// 写入 failed 状态（管线执行失败）。
pub fn write_failed(
    dir: &Path,
    doc_id: &str,
    error: &str,
    now: impl Fn() -> String,
) -> std::io::Result<PathBuf> {
    let state = ReviewStateFile::failed(error, now);
    write(&state_file_path(dir, doc_id), &state)?;
    Ok(state_file_path(dir, doc_id))
}

/// 读取状态文件；文件不存在或解析失败时返回 None（容忍损坏）。
pub fn read(dir: &Path, doc_id: &str) -> Option<ReviewStateFile> {
    let path = state_file_path(dir, doc_id);
    let json = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<ReviewStateFile>(&json).ok()
}

/// 删除状态文件（管线成功完成时调用）。文件不存在静默成功。
pub fn remove(dir: &Path, doc_id: &str) {
    let _ = std::fs::remove_file(state_file_path(dir, doc_id));
}

fn write(path: &Path, state: &ReviewStateFile) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    // 先写临时文件再 rename，避免进程崩溃留下半个 JSON。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    fn fixed_now() -> String {
        "2026-08-27T00:00:00Z".to_string()
    }

    #[test]
    fn state_file_path_uses_suffix() {
        let dir = temp_dir();
        let path = state_file_path(dir.path(), "doc-1");
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "doc-1_review_state.json"
        );
    }

    #[test]
    fn write_running_then_read_roundtrip() {
        let dir = temp_dir();
        write_running(dir.path(), "doc-a", fixed_now).expect("write running");
        let state = read(dir.path(), "doc-a").expect("state exists");
        assert!(state.is_running());
        assert_eq!(state.status, "running");
        assert_eq!(state.started_at, fixed_now());
        assert_eq!(state.error, None);
    }

    #[test]
    fn write_failed_preserves_error() {
        let dir = temp_dir();
        write_failed(dir.path(), "doc-b", "审核引擎执行失败: boom", fixed_now)
            .expect("write failed");
        let state = read(dir.path(), "doc-b").expect("state exists");
        assert_eq!(state.status, "failed");
        assert_eq!(state.error.as_deref(), Some("审核引擎执行失败: boom"));
    }

    #[test]
    fn read_missing_file_returns_none() {
        let dir = temp_dir();
        assert!(read(dir.path(), "doc-missing").is_none());
    }

    #[test]
    fn remove_after_write_clears_state() {
        let dir = temp_dir();
        write_running(dir.path(), "doc-c", fixed_now).expect("write");
        remove(dir.path(), "doc-c");
        assert!(read(dir.path(), "doc-c").is_none());
    }

    #[test]
    fn remove_missing_file_is_silent() {
        let dir = temp_dir();
        remove(dir.path(), "doc-never-written");
    }

    #[test]
    fn read_corrupt_json_tolerates_none() {
        let dir = temp_dir();
        let path = state_file_path(dir.path(), "doc-corrupt");
        std::fs::write(&path, "{ not valid json !!!").expect("write corrupt");
        assert!(read(dir.path(), "doc-corrupt").is_none());
    }

    #[test]
    fn read_wrong_schema_tolerates_none() {
        let dir = temp_dir();
        let path = state_file_path(dir.path(), "doc-wrong");
        std::fs::write(&path, r#"{"hello": "world"}"#).expect("write wrong schema");
        assert!(read(dir.path(), "doc-wrong").is_none());
    }
}
