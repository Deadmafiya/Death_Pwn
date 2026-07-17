//! Goal-completion context threaded through the engine's multi-step loop.

use crate::schema::Mode;

/// One executed step in a goal-completion run, with a one-line outcome summary.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    pub command: String,
    pub summary: String,
}

/// Mutable context for a goal-completion session, threaded through the loop.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalContext {
    pub goal_summary: String,
    pub mode: Mode,
    pub steps_taken: u32,
    pub history: Vec<StepRecord>,
}

impl GoalContext {
    /// Start a fresh context for the given goal and mode.
    pub fn new(goal_summary: String, mode: Mode) -> Self {
        Self {
            goal_summary,
            mode,
            steps_taken: 0,
            history: Vec::new(),
        }
    }

    /// Record one executed step and advance the step counter.
    pub fn record_step(&mut self, record: StepRecord) {
        self.history.push(record);
        self.steps_taken += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;

    #[test]
    fn record_step_appends_history_and_counts() {
        let mut ctx = GoalContext::new("get a shell".to_string(), Mode::GoalCompletion);
        assert_eq!(ctx.steps_taken, 0);
        assert!(ctx.history.is_empty());

        ctx.record_step(StepRecord {
            command: "nmap -sV 10.0.0.5".to_string(),
            summary: "found ssh".to_string(),
        });
        ctx.record_step(StepRecord {
            command: "hydra -l root ssh://10.0.0.5".to_string(),
            summary: "no creds".to_string(),
        });

        assert_eq!(ctx.steps_taken, 2);
        assert_eq!(ctx.history.len(), 2);
        assert_eq!(ctx.history[0].command, "nmap -sV 10.0.0.5");
        assert_eq!(ctx.history[1].summary, "no creds");
        assert_eq!(ctx.mode, Mode::GoalCompletion);
    }
}
