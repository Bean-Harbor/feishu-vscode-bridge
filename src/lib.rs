#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Continue,
    ContinueAll,
    Status,
    Help,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    Ok(String),
    Err(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub executed: usize,
    pub remaining: usize,
    pub stopped_at: Option<usize>,
    pub lines: Vec<String>,
}

pub fn parse_intent(text: &str) -> Intent {
    let lower = text.trim().to_lowercase();
    if matches!(lower.as_str(), "继续" | "continue" | "继续执行" | "下一步") {
        return Intent::Continue;
    }
    if matches!(
        lower.as_str(),
        "执行全部" | "全部执行" | "run all" | "执行所有步骤" | "全部继续" | "一键执行"
    ) {
        return Intent::ContinueAll;
    }
    if matches!(lower.as_str(), "状态" | "status") {
        return Intent::Status;
    }
    if lower.is_empty() || matches!(lower.as_str(), "help" | "帮助") {
        return Intent::Help;
    }
    Intent::Unknown
}

pub fn execute_continue_all<F>(pending_steps: &mut Vec<String>, mut exec: F) -> RunSummary
where
    F: FnMut(&str) -> StepResult,
{
    if pending_steps.is_empty() {
        return RunSummary {
            executed: 0,
            remaining: 0,
            stopped_at: None,
            lines: vec!["无待执行步骤。".to_string()],
        };
    }

    let mut lines = Vec::new();
    let mut executed = 0usize;
    let mut stopped_at = None;

    while !pending_steps.is_empty() {
        let step = pending_steps.remove(0);
        let line_prefix = format!("▸ 执行步骤: {}", step);
        match exec(&step) {
            StepResult::Ok(msg) => {
                lines.push(format!("{}\n{}", line_prefix, msg));
                executed += 1;
            }
            StepResult::Err(msg) => {
                lines.push(format!("{}\n❌ {}", line_prefix, msg));
                executed += 1;
                stopped_at = Some(executed);
                break;
            }
        }
    }

    let remaining = pending_steps.len();
    if remaining == 0 {
        lines.push("✅ 所有步骤已执行完毕".to_string());
    } else {
        lines.push(format!(
            "⚠️ 步骤失败，已暂停。还剩 {} 步，发送「继续」或「执行全部」恢复。",
            remaining
        ));
    }

    RunSummary {
        executed,
        remaining,
        stopped_at,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_continue_all_aliases() {
        assert_eq!(parse_intent("执行全部"), Intent::ContinueAll);
        assert_eq!(parse_intent("run all"), Intent::ContinueAll);
        assert_eq!(parse_intent("一键执行"), Intent::ContinueAll);
    }

    #[test]
    fn continue_all_stops_on_error() {
        let mut steps = vec![
            "step-1".to_string(),
            "step-2".to_string(),
            "step-3".to_string(),
        ];

        let summary = execute_continue_all(&mut steps, |s| {
            if s == "step-2" {
                StepResult::Err("failed".to_string())
            } else {
                StepResult::Ok("ok".to_string())
            }
        });

        assert_eq!(summary.executed, 2);
        assert_eq!(summary.remaining, 1);
        assert_eq!(summary.stopped_at, Some(2));
        assert_eq!(steps, vec!["step-3".to_string()]);
    }
}
