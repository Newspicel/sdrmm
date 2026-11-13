//! What a processor face draws. A processor declares its widgets in the catalog and fills them
//! from each reading, so one face serves every processor, a plugin's too.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{array::ArrayTuningMode, processor::beamformer::LaneWeight, ws::StreamKind};

/// A widget on a processor face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum ViewSpec {
    /// Bearings on a compass rose.
    Bearing,
    /// A polar beam pattern beside the weight of each lane.
    Pattern,
    /// A polarisation ellipse.
    Ellipse,
    /// The surface the processor streams, its marks drawn over it.
    Surface { stream: StreamKind },
    /// Rows of a table.
    Table,
}

/// Something an operator asks a running processor to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorAction {
    ClearTracks,
}

impl ProcessorAction {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClearTracks => "Clear",
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::ClearTracks => "Forget every track",
        }
    }

    #[must_use]
    pub fn spec(self) -> ActionSpec {
        ActionSpec {
            action: self,
            label: self.label().to_owned(),
            title: self.title().to_owned(),
        }
    }
}

/// A button a processor face shows for one of its actions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActionSpec {
    pub action: ProcessorAction,
    pub label: String,
    pub title: String,
}

/// What a processor declares about its face.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ProcessorFacets {
    /// Widgets in the order the face draws them.
    pub views: Vec<ViewSpec>,
    pub actions: Vec<ActionSpec>,
    /// How the wired array must tune its lanes, when the processor cares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<ArrayTuningMode>,
}

impl ProcessorFacets {
    #[must_use]
    pub fn new(views: &[ViewSpec], actions: &[ProcessorAction]) -> Self {
        Self {
            views: views.to_vec(),
            actions: actions.iter().map(|action| action.spec()).collect(),
            tuning: None,
        }
    }

    #[must_use]
    pub fn tuned(self, tuning: ArrayTuningMode) -> Self {
        Self {
            tuning: Some(tuning),
            ..self
        }
    }
}

/// The face's subtitle when a reading has more to say than the lane count.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Headline {
    pub text: String,
    #[serde(default)]
    pub warn: bool,
}

/// A condition a face shows as a chip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Flag {
    pub label: String,
    pub title: String,
    #[serde(default)]
    pub danger: bool,
}

impl Flag {
    #[must_use]
    pub fn warn(label: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            title: title.into(),
            danger: false,
        }
    }

    #[must_use]
    pub fn danger(label: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            danger: true,
            ..Self::warn(label, title)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NeedleWeight {
    #[default]
    Primary,
    Secondary,
    Mirror,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Needle {
    pub relative_deg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_deg: Option<f32>,
    pub weight: NeedleWeight,
}

/// Bearings relative to the array; `heading_deg` turns them to true north.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BearingView {
    /// Levels round the circle from the array's forward direction, clockwise.
    pub spectrum: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f64>,
    pub needles: Vec<Needle>,
    /// The spread of the first needle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sigma_deg: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PatternView {
    /// Levels round the circle from the array's forward direction, clockwise.
    pub pattern: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steer_deg: Option<f32>,
    pub nulls_deg: Vec<f32>,
    pub weights: Vec<LaneWeight>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EllipseView {
    pub angle_deg: f32,
    pub ellipticity_deg: f32,
    /// Its sign gives the sense of rotation.
    pub v: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Cell {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub faint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Cell {
    #[must_use]
    pub fn of(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TableRow {
    /// Ties the row to the surface mark with the same key.
    pub key: u32,
    pub cells: Vec<Cell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dim: bool,
    /// The row can be picked to pick its mark.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pick: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TableView {
    pub heads: Vec<String>,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MarkShape {
    #[default]
    Dot,
    Cross,
    Square,
    Triangle,
}

/// A point on a surface, in the surface's own axes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Mark {
    pub x: f64,
    pub y: f64,
    /// `y` from true north, for a surface with a bearing axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_y: Option<f64>,
    pub shape: MarkShape,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Where it was before, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trail: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub faint: bool,
}

/// One of several layers a surface holds, such as a baseline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Layer {
    pub label: String,
    pub text: String,
    pub title: String,
    /// Followed over time while the layer is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_deg: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SurfaceView {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f64>,
}

/// What one reading puts on its processor's face.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<Headline>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<Flag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing: Option<BearingView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<PatternView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ellipse: Option<EllipseView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableView>,
}
