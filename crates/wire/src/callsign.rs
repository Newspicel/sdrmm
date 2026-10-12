use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Callsign {
    pub call: String,
    pub country: String,
    pub prefix: String,
    pub continent: String,
    pub cq_zone: u8,
    pub itu_zone: u8,
}
