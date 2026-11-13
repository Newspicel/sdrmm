use crate::{
    DeviceSetState, Engine, EngineError, network_export::NetworkExportTap,
    publishing::recording::RecordingPublisher, recording::RecorderTap, runtime::DspCommand,
    sample_rate_of, time_machine::TimeMachineTap,
};

pub(crate) const SWEEPING: &str = "the radio is sweeping in firmware; stop the scan first";

/// Taps for the IQ sinks of a runtime about to be replaced. They are made while it still runs,
/// so each writer has its next feed queued before the old one ends and does not take the end of
/// the old feed for the end of the sink.
#[derive(Default)]
pub(crate) struct Handover {
    recording: Option<RecorderTap>,
    network: Option<NetworkExportTap>,
    history: Option<TimeMachineTap>,
}

impl Engine {
    pub(crate) fn prepare_handover(&self, ds: u32) -> Handover {
        self.lock()
            .device_sets
            .get(&ds)
            .map_or_else(Handover::default, DeviceSetState::prepare_handover)
    }
}

impl DeviceSetState {
    /// A firmware sweep carries no IQ, so it would starve these sinks for its whole run.
    pub(crate) fn holds_iq_sinks(&self) -> bool {
        self.recording.is_some() || self.network_export.is_some() || self.pre_roll.is_some()
    }

    fn prepare_handover(&self) -> Handover {
        Handover {
            recording: self.fresh_recording_tap(),
            network: self.fresh_network_tap(),
            history: self.fresh_history_tap(),
        }
    }

    /// Hands the IQ sinks to the runtime that just took over, so a restart leaves a marked gap in
    /// them instead of ending them. A sink the new runtime cannot feed fails where the operator
    /// sees it.
    pub(crate) fn reattach_iq_sinks(&self, handover: Handover) {
        self.reattach_recording(handover.recording);
        self.reattach_network_export(handover.network);
        self.reattach_time_machine(handover.history);
    }

    fn fresh_recording_tap(&self) -> Option<RecorderTap> {
        let recording = self.recording.as_ref()?;
        Some(recording.position.as_ref()?.resume(&recording.shared))
    }

    fn fresh_network_tap(&self) -> Option<NetworkExportTap> {
        let export = self.network_export.as_ref()?;
        export
            .refeed
            .as_ref()?
            .tap()
            .inspect_err(|error| export.shared.fail(lost(&error.to_string())))
            .ok()
    }

    fn fresh_history_tap(&self) -> Option<TimeMachineTap> {
        let history = self.pre_roll.as_ref()?;
        let shared = history.handle.shared();
        history
            .control
            .refeed(shared, history.sample_rate as f64)
            .inspect_err(|error| shared.fail(lost(&error.to_string())))
            .ok()
    }

    fn reattach_recording(&self, prepared: Option<RecorderTap>) {
        let Some(recording) = &self.recording else {
            return;
        };
        let Some(tap) = prepared
            .filter(|tap| tap.feeds(&recording.shared))
            .or_else(|| self.fresh_recording_tap())
        else {
            return;
        };
        let handed = self.check_lane(recording.stream).and_then(|()| {
            let publisher = RecordingPublisher::new(sample_rate_of(&self.settings))
                .map_err(|error| format!("start recording publisher: {error}"))?;
            self.hand_over(
                recording.stream,
                DspCommand::StartRecording { tap, publisher },
            )
        });
        if let Err(why) = handed {
            recording.shared.fail(lost(&why));
        }
    }

    fn reattach_network_export(&self, prepared: Option<NetworkExportTap>) {
        let Some(export) = &self.network_export else {
            return;
        };
        let Some(tap) = prepared
            .filter(|tap| tap.feeds(&export.shared))
            .or_else(|| self.fresh_network_tap())
        else {
            return;
        };
        let handed = self
            .check_lane(export.stream)
            .and_then(|()| self.hand_over(export.stream, DspCommand::StartNetworkExport { tap }));
        if let Err(why) = handed {
            export.shared.fail(lost(&why));
        }
    }

    fn reattach_time_machine(&self, prepared: Option<TimeMachineTap>) {
        let Some(history) = &self.pre_roll else {
            return;
        };
        let shared = history.handle.shared();
        let Some(tap) = prepared
            .filter(|tap| tap.feeds(shared))
            .or_else(|| self.fresh_history_tap())
        else {
            return;
        };
        let handed = self.check_lane(history.stream).and_then(|()| {
            self.hand_over(
                history.stream,
                DspCommand::StartTimeMachine { tap: Box::new(tap) },
            )
        });
        if let Err(why) = handed {
            shared.fail(lost(&why));
        }
    }

    fn check_lane(&self, stream: u32) -> Result<(), String> {
        self.check_physical(stream)
            .map_err(|error: EngineError| error.to_string())
    }

    fn hand_over(&self, stream: u32, command: DspCommand) -> Result<(), String> {
        self.dsp_sender(stream)
            .map_err(|error| error.to_string())?
            .send(command)
            .map_err(|_| format!("stream {stream} takes no IQ sinks"))
    }
}

fn lost(why: &str) -> String {
    format!("stopped when the radio restarted: {why}")
}
