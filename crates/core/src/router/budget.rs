//! Budget caps on recorded spend (idea from OmniRoute's "maybe" list; our
//! own design on top of the usage tables).
//!
//! A cap is a USD limit per day or per calendar month (UTC), for one model
//! role or overall. At 80 % the gateway logs a warning and the Usage page
//! shows it; at 100 % it either refuses the request with a clear message or
//! sends it to the cheapest priced model of the combo, as the user chose.

use serde::{Deserialize, Serialize};

/// Share of a cap at which to warn.
pub const WARN_AT: f64 = 0.8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    #[default]
    Daily,
    Monthly,
}

impl Period {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
        }
    }
}

/// One spending limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    /// A role (`chat`, `review`…) or `None` for all spend.
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub period: Period,
    pub limit_usd: f64,
}

impl Budget {
    #[must_use]
    pub fn applies_to(&self, role: &str) -> bool {
        self.role.as_deref().is_none_or(|r| r == role)
    }

    /// "Daily budget for chat" / "Monthly budget".
    #[must_use]
    pub fn label(&self) -> String {
        let period = match self.period {
            Period::Daily => "Daily",
            Period::Monthly => "Monthly",
        };
        match &self.role {
            Some(role) => format!("{period} budget for {role}"),
            None => format!("{period} budget"),
        }
    }
}

/// What to do when a cap is reached.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetAction {
    #[default]
    Block,
    Cheapest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Over,
}

/// A cap with what has been spent against it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub budget: Budget,
    pub spent: f64,
    pub level: Level,
}

impl BudgetStatus {
    #[must_use]
    pub fn new(budget: Budget, spent: f64) -> Self {
        let level = if budget.limit_usd <= 0.0 {
            Level::Ok
        } else if spent >= budget.limit_usd {
            Level::Over
        } else if spent >= budget.limit_usd * WARN_AT {
            Level::Warn
        } else {
            Level::Ok
        };
        Self {
            budget,
            spent,
            level,
        }
    }

    /// The message shown when the cap blocks a request.
    #[must_use]
    pub fn message(&self) -> String {
        let wait = match self.budget.period {
            Period::Daily => "tomorrow (UTC)",
            Period::Monthly => "next month",
        };
        format!(
            "{} reached (${:.2} of ${:.2}). Raise it in Settings → Providers → Budgets, or wait until {wait}.",
            self.budget.label(),
            self.spent,
            self.budget.limit_usd
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_messages() {
        let cap = Budget {
            role: Some("chat".into()),
            period: Period::Daily,
            limit_usd: 5.0,
        };
        assert!(cap.applies_to("chat") && !cap.applies_to("review"));
        assert_eq!(BudgetStatus::new(cap.clone(), 1.0).level, Level::Ok);
        assert_eq!(BudgetStatus::new(cap.clone(), 4.0).level, Level::Warn);
        let over = BudgetStatus::new(cap, 5.0);
        assert_eq!(over.level, Level::Over);
        assert_eq!(
            over.message(),
            "Daily budget for chat reached ($5.00 of $5.00). Raise it in Settings → Providers → Budgets, or wait until tomorrow (UTC)."
        );
        let all = Budget {
            role: None,
            period: Period::Monthly,
            limit_usd: 0.0,
        };
        assert!(all.applies_to("design"));
        assert_eq!(
            BudgetStatus::new(all, 99.0).level,
            Level::Ok,
            "0 = no limit"
        );
    }
}
