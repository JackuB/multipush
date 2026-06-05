pub mod evaluator;
pub mod executor;
pub mod plan;
pub mod targeting;

pub use evaluator::evaluate;
pub use executor::{
    execute, ApplyReport, BranchProtectionAction, PrAction, PrActionKind, SettingsAction,
    SettingsActionKind,
};
pub use plan::plan_apply_actions;
pub use targeting::{filter_repos, filter_repos_basic};
