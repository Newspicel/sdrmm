//! A processor's settings and readings, typed by the processor that owns them and carried by
//! everything else without knowing their shape.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use sdrmm_wire::channel::ChannelStatus;
use sdrmm_wire::{ProcessorParams, ProcessorReading, ProcessorView, ServerEvent};
use serde::{Serialize, de::DeserializeOwned};

/// A processor's own settings struct, named by the type id it registers under.
pub trait Settings:
    Clone + PartialEq + fmt::Debug + Default + Serialize + DeserializeOwned + Send + Sync + 'static
{
    const TYPE_ID: &'static str;

    /// Why these settings cannot run, if they cannot.
    fn problem(&self) -> Option<&'static str> {
        None
    }
}

/// Settings a processor type keeps beside its own, such as a loaded plugin's.
pub trait Erasable: Clone + PartialEq + fmt::Debug + Serialize + Send + Sync + 'static {
    fn problem(&self) -> Option<&'static str> {
        None
    }
}

impl<S: Settings> Erasable for S {
    fn problem(&self) -> Option<&'static str> {
        Settings::problem(self)
    }
}

trait Erased: Any + fmt::Debug + Send + Sync {
    fn clone_box(&self) -> Box<dyn Erased>;
    fn same(&self, other: &dyn Erased) -> bool;
    fn problem(&self) -> Option<&'static str>;
    fn json(&self) -> Result<serde_json::Value, serde_json::Error>;
    fn as_any(&self) -> &dyn Any;
}

impl<T: Erasable> Erased for T {
    fn clone_box(&self) -> Box<dyn Erased> {
        Box::new(self.clone())
    }

    fn same(&self, other: &dyn Erased) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn problem(&self) -> Option<&'static str> {
        Erasable::problem(self)
    }

    fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Settings parsed for the processor of one type. Parsed on the control plane, so the DSP
/// thread only reads fields.
pub struct Params {
    type_id: Arc<str>,
    value: Box<dyn Erased>,
}

impl Params {
    #[must_use]
    pub fn new<S: Settings>(settings: S) -> Self {
        Self::erase(S::TYPE_ID, settings)
    }

    #[must_use]
    pub fn erase<T: Erasable>(type_id: &str, value: T) -> Self {
        Self {
            type_id: Arc::from(type_id),
            value: Box::new(value),
        }
    }

    #[must_use]
    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    /// The settings as `T`, when they are `T`.
    #[must_use]
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.value.as_any().downcast_ref()
    }

    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        self.value.problem()
    }

    /// The settings as the wire carries them.
    pub fn to_wire(&self) -> Result<ProcessorParams, String> {
        Ok(ProcessorParams {
            type_id: self.type_id.to_string(),
            settings: self.value.json().map_err(|error| error.to_string())?,
        })
    }
}

impl Clone for Params {
    fn clone(&self) -> Self {
        Self {
            type_id: Arc::clone(&self.type_id),
            value: self.value.clone_box(),
        }
    }
}

impl PartialEq for Params {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && self.value.same(other.value.as_ref())
    }
}

impl fmt::Debug for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Params")
            .field("type_id", &self.type_id)
            .field("value", &self.value)
            .finish()
    }
}

/// `settings` as the wire carries them, under their own type id.
pub fn wire_params<S: Settings>(settings: &S) -> Result<ProcessorParams, String> {
    ProcessorParams::new(S::TYPE_ID, settings)
}

/// A processor's reading: what its face shows and what its node reports.
pub trait Report: Clone + fmt::Debug + Serialize + Send + Sync + 'static {
    /// The readouts and health a face shows.
    fn status(&self) -> ChannelStatus;

    /// What it puts on its processor's face.
    fn view(&self) -> ProcessorView;
}

trait ErasedReport: Any + fmt::Debug + Send + Sync {
    fn clone_box(&self) -> Box<dyn ErasedReport>;
    fn status(&self) -> ChannelStatus;
    fn view(&self) -> ProcessorView;
    fn json(&self) -> Result<serde_json::Value, serde_json::Error>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<R: Report> ErasedReport for R {
    fn clone_box(&self) -> Box<dyn ErasedReport> {
        Box::new(self.clone())
    }

    fn status(&self) -> ChannelStatus {
        Report::status(self)
    }

    fn view(&self) -> ProcessorView {
        Report::view(self)
    }

    fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A reading of one processor type. Its slot is made on the control plane with room reserved,
/// so filling it on the DSP thread allocates nothing.
pub struct Reading {
    type_id: Arc<str>,
    value: Box<dyn ErasedReport>,
}

impl Reading {
    #[must_use]
    pub fn new<R: Report>(type_id: &str, value: R) -> Self {
        Self {
            type_id: Arc::from(type_id),
            value: Box::new(value),
        }
    }

    #[must_use]
    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    #[must_use]
    pub fn get<R: 'static>(&self) -> Option<&R> {
        self.value.as_any().downcast_ref()
    }

    pub fn get_mut<R: 'static>(&mut self) -> Option<&mut R> {
        self.value.as_any_mut().downcast_mut()
    }

    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        self.value.status()
    }

    #[must_use]
    pub fn view(&self) -> ProcessorView {
        self.value.view()
    }

    /// The reading as the wire carries it.
    pub fn to_wire(&self) -> Result<ProcessorReading, String> {
        Ok(ProcessorReading {
            type_id: self.type_id.to_string(),
            reading: self.value.json().map_err(|error| error.to_string())?,
        })
    }

    /// The update a face of `node` shows: the reading with its readouts and view.
    pub fn update(&self, node: String) -> Result<ServerEvent, String> {
        Ok(ServerEvent::ProcessorUpdate {
            node,
            reading: Box::new(self.to_wire()?),
            status: Box::new(self.status()),
            view: Box::new(self.view()),
        })
    }
}

impl Clone for Reading {
    fn clone(&self) -> Self {
        Self {
            type_id: Arc::clone(&self.type_id),
            value: self.value.clone_box(),
        }
    }
}

impl fmt::Debug for Reading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reading")
            .field("type_id", &self.type_id)
            .field("value", &self.value)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::channel::ChannelHealth;
    use serde::Deserialize;

    use super::*;

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Meter {
        blocks: u32,
    }

    impl Settings for Meter {
        const TYPE_ID: &'static str = "meter";

        fn problem(&self) -> Option<&'static str> {
            (self.blocks == 0).then_some("Blocks out of range")
        }
    }

    impl Report for Meter {
        fn status(&self) -> ChannelStatus {
            ChannelStatus {
                health: ChannelHealth::default(),
                readouts: Vec::new(),
                choices: Vec::new(),
            }
        }

        fn view(&self) -> ProcessorView {
            ProcessorView::default()
        }
    }

    #[test]
    fn params_read_back_as_their_own_type() {
        let params = Params::new(Meter { blocks: 4 });
        assert_eq!(params.type_id(), "meter");
        assert_eq!(params.get::<Meter>(), Some(&Meter { blocks: 4 }));
        assert_eq!(params.get::<u32>(), None);
        assert_eq!(params.clone(), params);
        assert_ne!(Params::new(Meter { blocks: 5 }), params);
        assert_eq!(
            Params::new(Meter::default()).problem(),
            Some("Blocks out of range")
        );
    }

    #[test]
    fn params_and_readings_go_back_to_the_wire_shape() {
        let wire = Params::new(Meter { blocks: 4 })
            .to_wire()
            .expect("serializes");
        assert_eq!(wire, wire_params(&Meter { blocks: 4 }).expect("serializes"));
        let mut reading = Reading::new("meter", Meter::default());
        if let Some(meter) = reading.get_mut::<Meter>() {
            meter.blocks = 9;
        }
        let wire = reading.to_wire().expect("serializes");
        assert_eq!(wire.reading, serde_json::json!({"blocks": 9}));
        assert_eq!(reading.clone().get::<Meter>(), Some(&Meter { blocks: 9 }));
    }
}
