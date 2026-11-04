use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::choices::ParamChoice;
use crate::present::Field;

/// What a running decoder reports about itself between events: readouts for its node face and
/// the options it found for `Live` settings, keyed by the setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readouts: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<ParamChoice>,
}
