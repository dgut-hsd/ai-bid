//! G6 知识沉淀引擎。
//!
//! 三人分工：
//! - 组员 A：`collect::collect_candidates` — 挑精华
//! - 组员 B：`extract::extract_and_dedup` — 拆实体 + 查重
//! - 组长：  `graph::Neo4jClient` + `run::run` — 建图 + 查询 + 整合

pub mod collect;
pub mod extract;
pub mod graph;
pub mod run;
pub mod types;
