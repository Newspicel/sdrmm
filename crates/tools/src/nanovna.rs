mod device;
mod discovery;
mod shell;

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use sdrmm_wire::{
    MAX_NANOVNA_AVERAGES, MAX_NANOVNA_BANDWIDTH_HZ, MAX_NANOVNA_CAL_SLOT, MAX_NANOVNA_FREQ_HZ,
    MAX_NANOVNA_POINTS, MAX_NANOVNA_PORT_LEN, MAX_NANOVNA_POWER, MIN_NANOVNA_BANDWIDTH_HZ,
    MIN_NANOVNA_FREQ_HZ, MIN_NANOVNA_POINTS, NANOVNA_TOOL_ID, NanoVnaCalStep,
    NanoVnaCalibrateRequest, NanoVnaConfigureRequest, NanoVnaDevice, NanoVnaPower, NanoVnaRequest,
    NanoVnaResult, NanoVnaSweepRequest, ToolCategory, ToolDescriptor, ToolRequest, ToolResponse,
};
pub use shell::Connection;
use shell::Session;

use crate::{Tool, ToolError};

const BAUD_RATE: u32 = 115_200;
const READ_TIMEOUT: Duration = Duration::from_millis(100);

pub trait Backend: Send + Sync {
    fn devices(&self) -> Result<(Vec<NanoVnaDevice>, Vec<String>), String>;
    fn connect(&self, port: &str) -> Result<Box<dyn Connection>, String>;
}

struct SystemBackend;

impl Backend for SystemBackend {
    fn devices(&self) -> Result<(Vec<NanoVnaDevice>, Vec<String>), String> {
        serialport::available_ports()
            .map(discovery::partition)
            .map_err(|error| error.to_string())
    }

    fn connect(&self, port: &str) -> Result<Box<dyn Connection>, String> {
        let connection = serialport::new(port, BAUD_RATE)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|error| error.to_string())?;
        connection
            .clear(serialport::ClearBuffer::Input)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(connection))
    }
}

pub struct NanoVnaTool {
    backend: Arc<dyn Backend>,
    instrument: Mutex<()>,
}

impl Default for NanoVnaTool {
    fn default() -> Self {
        Self::with_backend(Arc::new(SystemBackend))
    }
}

impl NanoVnaTool {
    #[must_use]
    pub fn with_backend(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            instrument: Mutex::new(()),
        }
    }

    fn open(&self, port: &str) -> Result<Box<dyn Connection>, ToolError> {
        self.backend
            .connect(port)
            .map_err(|reason| ToolError::Unavailable {
                tool: NANOVNA_TOOL_ID,
                reason: format!("cannot open {port}: {reason}"),
            })
    }

    fn talk<T>(
        &self,
        port: &str,
        work: impl FnOnce(&mut Session<'_>) -> Result<T, String>,
    ) -> Result<T, ToolError> {
        let _turn = self
            .instrument
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut connection = self.open(port)?;
        let mut session = Session::new(connection.as_mut());
        work(&mut session).map_err(failed)
    }
}

impl Tool for NanoVnaTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: NANOVNA_TOOL_ID.to_owned(),
            name: "NanoVNA".to_owned(),
            summary: "Sweep S11 and S21 from a NanoVNA over USB serial".to_owned(),
            category: ToolCategory::Instrument,
            needs_hardware: true,
        }
    }

    fn run(&self, request: ToolRequest) -> Result<ToolResponse, ToolError> {
        let ToolRequest::NanoVna(request) = request else {
            return Err(ToolError::WrongTool {
                tool: NANOVNA_TOOL_ID,
                got: request.tool_id().to_owned(),
            });
        };
        match request {
            NanoVnaRequest::ListDevices => self
                .backend
                .devices()
                .map(|(devices, ignored_ports)| {
                    answer(NanoVnaResult::Devices {
                        devices,
                        ignored_ports,
                    })
                })
                .map_err(|reason| ToolError::Failed {
                    tool: NANOVNA_TOOL_ID,
                    reason: format!("listing serial ports: {reason}"),
                }),
            NanoVnaRequest::Describe(request) => {
                check_port(&request.port)?;
                self.talk(&request.port, |session| {
                    device::paused(session, |session| device::describe(session, &request.port))
                })
                .map(|report| answer(NanoVnaResult::Device(report)))
            }
            NanoVnaRequest::Sweep(request) => {
                validate_sweep(&request)?;
                self.talk(&request.port, |session| {
                    device::paused(session, |session| device::acquire(session, &request))
                })
                .map(|sweep| answer(NanoVnaResult::Sweep(sweep)))
            }
            NanoVnaRequest::Calibrate(request) => {
                validate_calibration(&request)?;
                self.talk(&request.port, |session| {
                    device::calibrate(session, &request.port, &request.step, request.range)
                })
                .map(|state| answer(NanoVnaResult::Calibration(state)))
            }
            NanoVnaRequest::Configure(request) => {
                validate_configuration(&request)?;
                self.talk(&request.port, |session| {
                    device::configure(session, &request)
                })
                .map(|report| answer(NanoVnaResult::Device(report)))
            }
        }
    }
}

fn answer(result: NanoVnaResult) -> ToolResponse {
    ToolResponse::NanoVna(Box::new(result))
}

fn failed(reason: String) -> ToolError {
    ToolError::Failed {
        tool: NANOVNA_TOOL_ID,
        reason,
    }
}

fn check_port(port: &str) -> Result<(), ToolError> {
    if port.is_empty() || port.len() > MAX_NANOVNA_PORT_LEN || port.contains('\0') {
        return Err(ToolError::Invalid(
            "port must name one serial device".to_owned(),
        ));
    }
    Ok(())
}

fn validate_sweep(request: &NanoVnaSweepRequest) -> Result<(), ToolError> {
    check_port(&request.port)?;
    range(
        "start_hz",
        request.start_hz,
        MIN_NANOVNA_FREQ_HZ,
        MAX_NANOVNA_FREQ_HZ,
    )?;
    range(
        "stop_hz",
        request.stop_hz,
        MIN_NANOVNA_FREQ_HZ,
        MAX_NANOVNA_FREQ_HZ,
    )?;
    if request.stop_hz <= request.start_hz {
        return Err(ToolError::Invalid(
            "stop_hz must be greater than start_hz".to_owned(),
        ));
    }
    if !(MIN_NANOVNA_POINTS..=MAX_NANOVNA_POINTS).contains(&request.points) {
        return Err(ToolError::Invalid(format!(
            "points must be between {MIN_NANOVNA_POINTS} and {MAX_NANOVNA_POINTS}"
        )));
    }
    if !(1..=MAX_NANOVNA_AVERAGES).contains(&request.averages) {
        return Err(ToolError::Invalid(format!(
            "averages must be between 1 and {MAX_NANOVNA_AVERAGES}"
        )));
    }
    Ok(())
}

fn validate_calibration(request: &NanoVnaCalibrateRequest) -> Result<(), ToolError> {
    check_port(&request.port)?;
    if let NanoVnaCalStep::Save { slot } | NanoVnaCalStep::Recall { slot } = request.step
        && slot > MAX_NANOVNA_CAL_SLOT
    {
        return Err(ToolError::Invalid(format!(
            "slot must be between 0 and {MAX_NANOVNA_CAL_SLOT}"
        )));
    }
    if let Some(range) = request.range {
        validate_sweep(&NanoVnaSweepRequest {
            port: request.port.clone(),
            start_hz: range.start_hz,
            stop_hz: range.stop_hz,
            points: range.points,
            averages: 1,
        })?;
    }
    Ok(())
}

fn validate_configuration(request: &NanoVnaConfigureRequest) -> Result<(), ToolError> {
    check_port(&request.port)?;
    if let Some(hz) = request.bandwidth_hz {
        range(
            "bandwidth_hz",
            u64::from(hz),
            u64::from(MIN_NANOVNA_BANDWIDTH_HZ),
            u64::from(MAX_NANOVNA_BANDWIDTH_HZ),
        )?;
    }
    if let Some(NanoVnaPower::Level(level)) = request.power
        && level > MAX_NANOVNA_POWER
    {
        return Err(ToolError::Invalid(format!(
            "power must be auto or between 0 and {MAX_NANOVNA_POWER}"
        )));
    }
    Ok(())
}

fn range(field: &str, value: u64, min: u64, max: u64) -> Result<(), ToolError> {
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(ToolError::Invalid(format!(
        "{field} must be between {min} and {max}"
    )))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        io::{self, Read, Write},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use sdrmm_wire::{NanoVnaStandard, NanoVnaSweepState};

    use super::*;

    const RECORDED_SESSION: &str =
        include_str!("../../../fixtures/nanovna/nanovna-h4-masked-scan.txt");

    const READ_BACK_SESSION: &str =
        include_str!("../../../fixtures/nanovna/nanovna-h4-session.txt");

    fn interrogation() -> &'static str {
        RECORDED_SESSION
            .split_once("scan 1000000")
            .map(|(head, _)| head)
            .expect("the recording scans")
    }

    struct FixtureConnection {
        reads: VecDeque<u8>,
        writes: Vec<u8>,
    }

    impl FixtureConnection {
        fn new(transcript: &str) -> Self {
            Self {
                reads: transcript.bytes().collect(),
                writes: Vec::new(),
            }
        }

        fn commands(&self) -> String {
            String::from_utf8(self.writes.clone()).expect("ASCII commands")
        }
    }

    impl Read for FixtureConnection {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = buffer.len().min(self.reads.len()).min(17);
            for slot in &mut buffer[..read] {
                *slot = self.reads.pop_front().unwrap_or_default();
            }
            Ok(read)
        }
    }

    impl Write for FixtureConnection {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.writes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn request() -> NanoVnaSweepRequest {
        NanoVnaSweepRequest {
            port: "/dev/cu.usbmodem4001".to_owned(),
            start_hz: 1_000_000,
            stop_hz: 2_000_000,
            points: 11,
            averages: 1,
        }
    }

    #[test]
    fn the_recorded_h4_session_produces_a_sweep_and_its_device_report() {
        let mut connection = FixtureConnection::new(RECORDED_SESSION);
        let mut session = Session::new(&mut connection);
        let sweep = device::acquire(&mut session, &request()).expect("replay the recorded sweep");

        assert_eq!(sweep.points.len(), 11);
        assert_eq!(sweep.points[0].frequency_hz, 1_000_000);
        assert_eq!(sweep.points[10].frequency_hz, 2_000_000);
        assert!((sweep.points[0].s11.re - 0.990_377_856).abs() < f64::EPSILON);
        assert!((sweep.points[0].s21.im + 0.000_003_988).abs() < f64::EPSILON);
        assert!(
            connection
                .commands()
                .ends_with("scan 1000000 2000000 11 7\rsweep 1000000 2000000 11\r"),
            "{}",
            connection.commands()
        );

        let device = &sweep.device;
        assert_eq!(device.firmware, "1.2.46");
        assert_eq!(device.board.as_deref(), Some("NanoVNA-H 4"));
        assert_eq!(device.max_points, Some(401));
        assert_eq!(device.battery_mv, Some(4183));
        assert_eq!(device.bandwidth_hz, Some(1000));
        assert_eq!(device.power, Some(255));
        assert_eq!(device.tcxo_hz, Some(26_000_000));
        assert_eq!(device.harmonic_threshold_hz, Some(300_000_100));
        assert_eq!(device.electrical_delay_s, Some(0.0));
        assert_eq!(device.s21_offset_db, Some(0.0));
        assert_eq!(
            device.sweep,
            Some(NanoVnaSweepState {
                start_hz: 1_000_000,
                stop_hz: 30_000_000,
                points: 101,
            })
        );
        assert!(device.commands.contains(&"scan".to_owned()));
        assert!(device.info.iter().any(|line| line.contains("STM32F303xC")));

        let calibration = &device.calibration;
        assert!(calibration.applied);
        assert_eq!(
            calibration.standards,
            vec![NanoVnaStandard::Load, NanoVnaStandard::Isolation]
        );
        assert_eq!(calibration.error_terms, vec!["Es", "Er", "Et"]);
    }

    #[test]
    fn firmware_that_ignores_the_output_mask_is_read_back_array_by_array() {
        let transcript = READ_BACK_SESSION.replacen(
            "scan 1000000 2000000 11",
            "scan 1000000 2000000 11 7",
            1,
        );
        let mut connection = FixtureConnection::new(&transcript);
        let mut session = Session::new(&mut connection);
        let sweep = device::acquire(&mut session, &request()).expect("read the arrays back");
        assert_eq!(sweep.points.len(), 11);
        assert!((sweep.points[0].s11.re - 0.990_368_064).abs() < f64::EPSILON);
        assert!((sweep.points[0].s21.im + 0.000_006_605).abs() < f64::EPSILON);
        assert!(
            connection
                .commands()
                .contains("frequencies\rdata 0\rdata 1\r")
        );
    }

    #[test]
    fn a_refused_scan_is_surfaced_instead_of_being_read_past() {
        let mut connection = FixtureConnection::new(&format!(
            "{}scan 1000000 2000000 11 7\r\n?\r\nch> ",
            interrogation()
        ));
        let mut session = Session::new(&mut connection);
        let error = device::acquire(&mut session, &request()).expect_err("scan must be supported");
        assert!(error.contains("refused `scan"), "{error}");
        assert!(
            !connection.commands().contains("frequencies"),
            "no data may be read after a refusal: {}",
            connection.commands()
        );
    }

    #[test]
    fn a_configuration_sets_bandwidth_and_power_then_describes_the_result() {
        let mut connection = FixtureConnection::new(&format!(
            "bandwidth 100 Hz\r\nbandwidth 39 (100Hz)\r\nch> power 2\r\nch> {}",
            interrogation()
        ));
        let mut session = Session::new(&mut connection);
        let report = device::configure(
            &mut session,
            &NanoVnaConfigureRequest {
                port: "port".to_owned(),
                bandwidth_hz: Some(100),
                power: Some(NanoVnaPower::Level(2)),
            },
        )
        .expect("configure the instrument");
        assert_eq!(report.firmware, "1.2.46");
        assert!(
            connection
                .commands()
                .starts_with("bandwidth 100 Hz\rpower 2\rversion\r")
        );
    }

    #[test]
    fn firmware_without_a_bandwidth_command_says_so() {
        let mut connection = FixtureConnection::new("bandwidth 100 Hz\r\nbandwidth?\r\nch> ");
        let mut session = Session::new(&mut connection);
        let error = device::configure(
            &mut session,
            &NanoVnaConfigureRequest {
                port: "port".to_owned(),
                bandwidth_hz: Some(100),
                power: None,
            },
        )
        .expect_err("the bandwidth command is missing");
        assert!(error.contains("refused `bandwidth 100 Hz`"), "{error}");
    }

    #[test]
    fn the_instrument_resumes_its_own_sweep_even_when_the_work_fails() {
        let mut connection = FixtureConnection::new("pause\r\nch> version\r\nch> resume\r\nch> ");
        let mut session = Session::new(&mut connection);
        let error = device::paused(&mut session, |session| device::describe(session, "port"))
            .expect_err("an empty version is an error");
        assert!(error.contains("no firmware version"), "{error}");
        assert_eq!(connection.commands(), "pause\rversion\rresume\r");
    }

    #[test]
    fn requests_to_one_instrument_take_turns() {
        let backend = Arc::new(TurnBackend::default());
        let tool = Arc::new(NanoVnaTool::with_backend(backend.clone()));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let tool = Arc::clone(&tool);
                std::thread::spawn(move || {
                    tool.run(ToolRequest::NanoVna(NanoVnaRequest::Describe(
                        sdrmm_wire::NanoVnaPortRequest {
                            port: "port".to_owned(),
                        },
                    )))
                })
            })
            .collect();
        for worker in workers {
            assert!(worker.join().expect("worker finished").is_ok());
        }
        assert_eq!(backend.peak.load(Ordering::SeqCst), 1);
    }

    #[derive(Default)]
    struct TurnBackend {
        open: Arc<AtomicUsize>,
        peak: AtomicUsize,
    }

    impl Backend for TurnBackend {
        fn devices(&self) -> Result<(Vec<NanoVnaDevice>, Vec<String>), String> {
            Ok((Vec::new(), Vec::new()))
        }

        fn connect(&self, _port: &str) -> Result<Box<dyn Connection>, String> {
            let open = self.open.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(open, Ordering::SeqCst);
            Ok(Box::new(TurnConnection {
                inner: FixtureConnection::new(&format!(
                    "pause\r\nch> {}resume\r\nch> ",
                    interrogation()
                )),
                open: Arc::clone(&self.open),
            }))
        }
    }

    struct TurnConnection {
        inner: FixtureConnection,
        open: Arc<AtomicUsize>,
    }

    impl Drop for TurnConnection {
        fn drop(&mut self) {
            self.open.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl Read for TurnConnection {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            std::thread::yield_now();
            self.inner.read(buffer)
        }
    }

    impl Write for TurnConnection {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.inner.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_calibration_step_sets_the_range_then_reports_what_the_device_did() {
        let mut connection = FixtureConnection::new(
            "sweep 1000000 30000000 101\r\nch> cal reset\r\nch> cal\r\nch> ",
        );
        let mut session = Session::new(&mut connection);
        let state = device::calibrate(
            &mut session,
            "port",
            &NanoVnaCalStep::Reset,
            Some(NanoVnaSweepState {
                start_hz: 1_000_000,
                stop_hz: 30_000_000,
                points: 101,
            }),
        )
        .expect("reset the calibration");
        assert!(state.standards.is_empty());
        assert!(!state.applied);
        assert_eq!(
            connection.commands(),
            "sweep 1000000 30000000 101\rcal reset\rcal\r"
        );
    }

    #[test]
    fn validation_rejects_unsafe_or_impossible_requests() {
        let mut invalid = request();
        invalid.port = "bad\0port".to_owned();
        assert!(
            matches!(validate_sweep(&invalid), Err(ToolError::Invalid(reason)) if reason.contains("port"))
        );
        invalid = request();
        invalid.stop_hz = invalid.start_hz;
        assert!(
            matches!(validate_sweep(&invalid), Err(ToolError::Invalid(reason)) if reason.contains("stop_hz"))
        );
        invalid = request();
        invalid.points = MAX_NANOVNA_POINTS + 1;
        assert!(
            matches!(validate_sweep(&invalid), Err(ToolError::Invalid(reason)) if reason.contains("points"))
        );
        for (bandwidth_hz, power) in [(Some(5), None), (None, Some(NanoVnaPower::Level(4)))] {
            assert!(matches!(
                validate_configuration(&NanoVnaConfigureRequest {
                    port: "port".to_owned(),
                    bandwidth_hz,
                    power,
                }),
                Err(ToolError::Invalid(_))
            ));
        }
        assert!(matches!(
            validate_calibration(&NanoVnaCalibrateRequest {
                port: "port".to_owned(),
                range: None,
                step: NanoVnaCalStep::Save {
                    slot: MAX_NANOVNA_CAL_SLOT + 1
                },
            }),
            Err(ToolError::Invalid(reason)) if reason.contains("slot")
        ));
    }
}
