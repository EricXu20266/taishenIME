//! 排序参数外置（审计报告 §4.9）。
//!
//! 影响候选顺序的参数原先全是编译期常量，改一个数就要重新编译发版。
//! 这里允许用一份 `tuning.json` 覆盖，三条硬约束：
//!
//! 1. **范围钳制**：每个字段有合法区间，越界取边界并记录日志，不静默接受；
//! 2. **缺省回退**：文件缺失 / 解析失败 / 字段缺失 → 用内置默认值，不 panic；
//! 3. **不做热更新**：进程内首次读取后固定。热更新会让"这次输入为什么变了"
//!    无法复现，收益不抵成本。
//!
//! 读取优先级（只取第一个存在且可读的）：
//! 1. 环境变量 `TAISHEN_TUNING` 指向的文件（便于测试与临时调参）；
//! 2. 可执行文件同目录 `tuning.json`（安装包随包下发）；
//! 3. `%APPDATA%/taishen-ime/tuning.json`（用户本机覆盖）。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 影响候选顺序的可调参数。默认值与改造前的编译期常量逐项一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    /// 上下文权重上限（单个后词最多前移多少位）
    pub context_weight_cap: u32,
    /// 内置精选搭配权重
    pub context_builtin_weight: u32,
    /// 外部搭配表权重
    pub context_external_weight: u32,
    /// 离线先验权重上限
    pub context_prior_max_weight: u32,
    /// 每个前词保留的学习型后词数上限
    pub learned_context_cap: usize,
    /// 降权词插入位置（前 N 位保留给非降权词）
    pub demote_position: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            context_weight_cap: 20,
            context_builtin_weight: 32,
            context_external_weight: 12,
            context_prior_max_weight: 6,
            learned_context_cap: 8,
            demote_position: 10,
        }
    }
}

/// 单个字段的取值区间与默认值。
struct FieldSpec {
    key: &'static str,
    min: u64,
    max: u64,
    default: u64,
}

const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        key: "context_weight_cap",
        min: 0,
        max: 100,
        default: 20,
    },
    FieldSpec {
        key: "context_builtin_weight",
        min: 0,
        max: 1000,
        default: 32,
    },
    FieldSpec {
        key: "context_external_weight",
        min: 0,
        max: 1000,
        default: 12,
    },
    FieldSpec {
        key: "context_prior_max_weight",
        min: 0,
        max: 1000,
        default: 6,
    },
    FieldSpec {
        key: "learned_context_cap",
        min: 1,
        max: 1000,
        default: 8,
    },
    FieldSpec {
        key: "demote_position",
        min: 1,
        max: 100,
        default: 10,
    },
];

impl Tuning {
    /// 解析 JSON：缺失字段用默认值，越界钳制，非法值回退并记录告警。
    /// 返回 (结果, 告警列表)。纯函数，便于单测。
    pub fn parse(json: &str) -> (Self, Vec<String>) {
        let mut tuning = Self::default();
        let mut warnings = Vec::new();

        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("解析失败，全部回退默认值: {e}"));
                return (tuning, warnings);
            }
        };
        let map = match value.as_object() {
            Some(m) => m,
            None => {
                warnings.push("顶层不是 JSON 对象，全部回退默认值".to_string());
                return (tuning, warnings);
            }
        };
        if let Some(v) = map.get("schema_version") {
            if v.as_u64() != Some(1) {
                warnings.push(format!("schema_version={v} 非当前支持的 1，仍按 1 解析"));
            }
        }

        for spec in FIELDS {
            let raw = match map.get(spec.key) {
                None => continue,
                Some(v) => v,
            };
            let Some(n) = raw.as_u64() else {
                warnings.push(format!(
                    "{}={} 不是非负整数，回退默认值 {}",
                    spec.key, raw, spec.default
                ));
                continue;
            };
            let clamped = n.clamp(spec.min, spec.max);
            if clamped != n {
                warnings.push(format!(
                    "{}={} 越界（允许 {}..={}），已钳制为 {}",
                    spec.key, n, spec.min, spec.max, clamped
                ));
            }
            let clamped = clamped as usize;
            match spec.key {
                "context_weight_cap" => tuning.context_weight_cap = clamped as u32,
                "context_builtin_weight" => tuning.context_builtin_weight = clamped as u32,
                "context_external_weight" => tuning.context_external_weight = clamped as u32,
                "context_prior_max_weight" => tuning.context_prior_max_weight = clamped as u32,
                "learned_context_cap" => tuning.learned_context_cap = clamped,
                "demote_position" => tuning.demote_position = clamped,
                _ => {}
            }
        }
        (tuning, warnings)
    }

    /// 从磁盘读一次。任何异常都回退默认值，绝不 panic。
    fn load_from_disk() -> Self {
        let Some(path) = Self::candidate_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let (tuning, warnings) = Self::parse(&text);
                for w in &warnings {
                    crate::log::error(&format!("[tuning] {}: {w}", path.display()));
                }
                if tuning != Self::default() {
                    crate::log::info(&format!(
                        "[tuning] 已加载 {}（context_weight_cap={} demote_position={}）",
                        path.display(),
                        tuning.context_weight_cap,
                        tuning.demote_position
                    ));
                }
                tuning
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                crate::log::error(&format!("[tuning] 读取 {} 失败: {e}", path.display()));
                Self::default()
            }
        }
    }

    fn candidate_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("TAISHEN_TUNING") {
            if !p.trim().is_empty() {
                return Some(PathBuf::from(p));
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let p = dir.join("tuning.json");
                if p.exists() {
                    return Some(p);
                }
            }
        }
        std::env::var("APPDATA")
            .ok()
            .map(|dir| PathBuf::from(dir).join("taishen-ime").join("tuning.json"))
    }
}

/// 进程内只读一次的参数表。调用方拿到的是 `&'static Tuning`。
pub fn tuning() -> &'static Tuning {
    static CACHE: OnceLock<Tuning> = OnceLock::new();
    CACHE.get_or_init(Tuning::load_from_disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_previous_constants() {
        let t = Tuning::default();
        assert_eq!(t.context_weight_cap, 20);
        assert_eq!(t.context_builtin_weight, 32);
        assert_eq!(t.context_external_weight, 12);
        assert_eq!(t.context_prior_max_weight, 6);
        assert_eq!(t.learned_context_cap, 8);
        assert_eq!(t.demote_position, 10);
    }

    #[test]
    fn parse_overrides_fields() {
        let (t, w) =
            Tuning::parse(r#"{"schema_version":1,"context_weight_cap":50,"demote_position":4}"#);
        assert_eq!(t.context_weight_cap, 50);
        assert_eq!(t.demote_position, 4);
        // 未给出的字段保持默认
        assert_eq!(t.context_builtin_weight, 32);
        assert!(w.is_empty(), "合法配置不应有告警: {w:?}");
    }

    #[test]
    fn parse_clamps_out_of_range() {
        let (t, w) = Tuning::parse(r#"{"context_weight_cap":9999,"demote_position":0}"#);
        assert_eq!(t.context_weight_cap, 100, "应钳制到上限");
        assert_eq!(t.demote_position, 1, "应钳制到下限");
        assert_eq!(w.len(), 2, "越界应逐条告警: {w:?}");
    }

    #[test]
    fn parse_missing_and_invalid_falls_back() {
        let (t, w) = Tuning::parse(r#"{"context_weight_cap":"big"}"#);
        assert_eq!(t.context_weight_cap, 20, "非整数回退默认值");
        assert_eq!(w.len(), 1);
        let (t2, w2) = Tuning::parse("not json");
        assert_eq!(t2, Tuning::default(), "整体解析失败回退默认值");
        assert_eq!(w2.len(), 1);
        let (t3, _) = Tuning::parse("{}");
        assert_eq!(t3, Tuning::default());
    }

    #[test]
    fn parse_negative_rejects() {
        let (t, w) = Tuning::parse(r#"{"learned_context_cap":-3}"#);
        assert_eq!(t.learned_context_cap, 8);
        assert_eq!(w.len(), 1, "负数应回退默认值并告警");
    }
}
