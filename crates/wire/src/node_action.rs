use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{HuntAction, ScanAction};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "action", content = "with", rename_all = "snake_case")]
pub enum NodeAction {
    Scan(ScanAction),
    Hunt(HuntAction),
}
