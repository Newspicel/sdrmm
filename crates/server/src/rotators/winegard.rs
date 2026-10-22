use std::io::{ErrorKind, Read, Write};

use sdrmm_wire::{MAX_WINEGARD_AZIMUTH_DEG, MAX_WINEGARD_ELEVATION_DEG, SkyPoint};

use super::link::{Link, LinkError};

const TURN: f64 = 360.0;
const CONSOLE: &str = "dcmc >";
const MOTORS: &str = "MCON>";
const UNIT: &str = "ODU>";
const PROMPTS: [&str; 3] = [CONSOLE, MOTORS, UNIT];
const NAMES: [&str; 3] = ["Azimuth", "Elevation", "Skew"];
const AZIMUTH: usize = 0;
const ELEVATION: usize = 1;
const MENU_DEPTH: usize = 4;
const MAX_TRANSCRIPT: usize = 8_192;
const TAIL: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Axis {
    steps: f64,
    per_turn: f64,
}

impl Axis {
    fn degrees(self) -> f64 {
        self.steps * TURN / self.per_turn
    }

    fn at(self, degrees: f64) -> i64 {
        (degrees * self.per_turn / TURN) as i64
    }
}

pub(super) struct Winegard<P> {
    port: P,
    mount: [Axis; 2],
}

impl<P: Read + Write> Winegard<P> {
    pub(super) fn open(port: P) -> Result<Self, LinkError> {
        let mut console = Self {
            port,
            mount: [Axis::default(); 2],
        };
        console.enter()?;
        console.refresh()?;
        Ok(console)
    }

    fn enter(&mut self) -> Result<(), LinkError> {
        let mut transcript = self.ask("")?;
        for _ in 0..MENU_DEPTH {
            if at_prompt(&transcript, CONSOLE) {
                return Ok(());
            }
            let step = if at_prompt(&transcript, UNIT) {
                "motorcon"
            } else if at_prompt(&transcript, MOTORS) {
                "run"
            } else {
                break;
            };
            transcript = self.ask(step)?;
        }
        Err(LinkError::Garbled(tail(&transcript)))
    }

    fn refresh(&mut self) -> Result<[Axis; 2], LinkError> {
        let transcript = self.ask("i")?;
        self.mount = axes(&transcript).ok_or_else(|| LinkError::Garbled(tail(&transcript)))?;
        Ok(self.mount)
    }

    fn turn(&mut self, motor: usize, steps: i64) -> Result<(), LinkError> {
        self.ask(&format!("p {motor} {steps}")).map(|_| ())
    }

    fn ask(&mut self, command: &str) -> Result<String, LinkError> {
        self.port.write_all(command.as_bytes())?;
        self.port.write_all(b"\r")?;
        self.port.flush()?;
        self.listen()
    }

    fn listen(&mut self) -> Result<String, LinkError> {
        let mut transcript = String::new();
        let mut chunk = [0_u8; 256];
        loop {
            let read = match self.port.read(&mut chunk) {
                Ok(read) => read,
                Err(error) if error.kind() == ErrorKind::TimedOut => 0,
                Err(error) if error.kind() == ErrorKind::WouldBlock => 0,
                Err(error) => return Err(error.into()),
            };
            if read == 0 || transcript.len() > MAX_TRANSCRIPT {
                return Err(LinkError::Garbled(tail(&transcript)));
            }
            transcript.push_str(&String::from_utf8_lossy(&chunk[..read]));
            if PROMPTS.iter().any(|prompt| at_prompt(&transcript, prompt)) {
                return Ok(transcript);
            }
        }
    }
}

impl<P: Read + Write + Send> Link for Winegard<P> {
    fn position(&mut self) -> Result<SkyPoint, LinkError> {
        let [azimuth, elevation] = self.refresh()?;
        Ok(SkyPoint {
            azimuth_deg: azimuth.degrees(),
            elevation_deg: elevation.degrees(),
        })
    }

    fn point(&mut self, aim: SkyPoint) -> Result<(), LinkError> {
        let azimuth = self.mount[AZIMUTH].at(aim.azimuth_deg.clamp(0.0, MAX_WINEGARD_AZIMUTH_DEG));
        let elevation =
            self.mount[ELEVATION].at(aim.elevation_deg.clamp(0.0, MAX_WINEGARD_ELEVATION_DEG));
        self.turn(AZIMUTH, azimuth)?;
        self.turn(ELEVATION, elevation)
    }

    fn stop(&mut self) -> Result<(), LinkError> {
        let [azimuth, elevation] = self.refresh()?;
        self.turn(AZIMUTH, azimuth.steps as i64)?;
        self.turn(ELEVATION, elevation.steps as i64)
    }
}

fn at_prompt(transcript: &str, prompt: &str) -> bool {
    transcript.trim_end().ends_with(prompt)
}

fn tail(transcript: &str) -> String {
    let text = transcript.trim();
    text.get(text.len().saturating_sub(TAIL)..)
        .unwrap_or(text)
        .to_owned()
}

fn axes(transcript: &str) -> Option<[Axis; 2]> {
    let mut reported = [[None; 2]; NAMES.len()];
    let mut axis = None;
    let mut fields = transcript.split('=');
    let mut head = fields.next()?;
    for tail in fields {
        if let Some(named) = named(head) {
            axis = Some(named);
        }
        if let (Some(index), Some(value)) = (axis, number(tail)) {
            match key(head) {
                "position" => reported[index][0] = Some(value),
                "steps/rev" => reported[index][1] = Some(value),
                _ => {}
            }
        }
        head = tail;
    }
    let mut mount = [Axis::default(); 2];
    for (axis, report) in mount.iter_mut().zip(reported) {
        *axis = Axis {
            steps: report[0]?,
            per_turn: report[1]?,
        };
        if axis.per_turn <= 0.0 {
            return None;
        }
    }
    Some(mount)
}

fn named(head: &str) -> Option<usize> {
    NAMES
        .iter()
        .enumerate()
        .filter_map(|(index, name)| head.rfind(name).map(|at| (at, index)))
        .max_by_key(|&(at, _)| at)
        .map(|(_, index)| index)
}

fn key(head: &str) -> &str {
    let line = head.rsplit('\n').next().unwrap_or(head);
    let after = line
        .rfind(|c: char| c.is_ascii_digit())
        .map_or(0, |at| at + 1);
    line.get(after..).unwrap_or("").trim()
}

fn number(tail: &str) -> Option<f64> {
    let text = tail.trim_start();
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.'))
        .unwrap_or(text.len());
    text.get(..end)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, io::Cursor};

    use super::*;

    const LG_TURN: f64 = 20_800.0;
    const SM_TURN: f64 = 10_750.0;
    const PROMPT: &str = "\r\ndcmc >";

    fn report(elevation_per_turn: u32, azimuth_steps: i64, elevation_steps: i64) -> String {
        format!(
            "i\r\nAzimuth:\r\n  actual velocity = 0\r\n  commanded velocity = 0\r\n  \
             target position = 0\r\n  steps/rev = 6600\r\n  polarity = 1\r\n  cmd = IDLE\r\n  \
             position = {azimuth_steps}\r\n  wrap pos = 0 command status = Cmd Complete\r\n  \
             pwm = 0\r\nElevation:\r\n  actual velocity = 0\r\n  commanded velocity = 0\r\n  \
             target position = 0\r\n  steps/rev = {elevation_per_turn}\r\n  polarity = -1\r\n  \
             cmd = IDLE\r\n  position = {elevation_steps}\r\n  command status = Cmd Complete\r\n  \
             pwm = 0\r\nSkew:\r\n  actual velocity = 0\r\n  commanded velocity = 0\r\n  \
             target position = 0\r\n  steps/rev = 2400\r\n  polarity = 1\r\n  cmd = IDLE\r\n  \
             position = 300\r\n  command status = Cmd Complete\r\n  pwm = 0\r\ndcmc >"
        )
    }

    struct Wire {
        replies: VecDeque<Cursor<Vec<u8>>>,
        sent: Vec<u8>,
    }

    impl Wire {
        fn answering(replies: &[&str]) -> Self {
            Self {
                replies: replies
                    .iter()
                    .map(|reply| Cursor::new(reply.as_bytes().to_vec()))
                    .collect(),
                sent: Vec::new(),
            }
        }

        fn typed(&self) -> String {
            String::from_utf8_lossy(&self.sent).into_owned()
        }
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            while let Some(reply) = self.replies.front_mut() {
                let read = reply.read(buf)?;
                if read > 0 {
                    return Ok(read);
                }
                self.replies.pop_front();
            }
            Ok(0)
        }
    }

    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.sent.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sky(azimuth_deg: f64, elevation_deg: f64) -> SkyPoint {
        SkyPoint {
            azimuth_deg,
            elevation_deg,
        }
    }

    fn opened(replies: &[&str]) -> Winegard<Wire> {
        let mut answers = vec![PROMPT];
        answers.extend_from_slice(replies);
        let mut rotator = Winegard::open(Wire::answering(&answers)).expect("reaches the console");
        rotator.port.sent.clear();
        rotator
    }

    #[test]
    fn the_mount_reports_its_own_step_counts() {
        let [azimuth, elevation] = axes(&report(20_800, 3_300, 5_200)).expect("reads");
        assert_eq!(azimuth.per_turn, 6_600.0);
        assert_eq!(elevation.per_turn, LG_TURN);
        assert!((azimuth.degrees() - 180.0).abs() < 0.01);
        assert!((elevation.degrees() - 90.0).abs() < 0.01);
        let [_, small] = axes(&report(10_750, 0, -1_758)).expect("reads");
        assert_eq!(small.per_turn, SM_TURN);
        assert!((small.degrees() + 58.87).abs() < 0.01);
    }

    #[test]
    fn a_report_missing_an_axis_or_its_scale_is_an_error() {
        assert!(axes("dcmc >i\r\nAzimuth:\r\n  position = 0\r\n  steps/rev = 6600\r\n").is_none());
        assert!(axes(&report(0, 0, 0)).is_none());
        assert!(axes("?\r\ndcmc >").is_none());
    }

    #[test]
    fn degrees_become_the_steps_the_mount_counts() {
        let azimuth = Axis {
            steps: 0.0,
            per_turn: 6_600.0,
        };
        for (degrees, steps) in [
            (0.0, 0),
            (3.0, 55),
            (45.0, 825),
            (180.0, 3_300),
            (360.0, 6_600),
            (420.0, 7_700),
            (445.0, 8_158),
        ] {
            assert_eq!(azimuth.at(degrees), steps, "{degrees}");
        }
        let large = Axis {
            steps: 0.0,
            per_turn: LG_TURN,
        };
        let small = Axis {
            steps: 0.0,
            per_turn: SM_TURN,
        };
        for (degrees, lg, sm) in [
            (90.0, 5_200, 2_687),
            (45.0, 2_600, 1_343),
            (15.0, 866, 447),
            (12.0, 693, 358),
            (-15.0, -866, -447),
        ] {
            assert_eq!(large.at(degrees), lg, "{degrees}");
            assert_eq!(small.at(degrees), sm, "{degrees}");
        }
    }

    #[test]
    fn the_debug_port_is_walked_down_to_the_motor_console() {
        let rotator = Winegard::open(Wire::answering(&[
            "\r\nODU>",
            "motorcon\r\nMCON>",
            "run\r\ndcmc >",
            &report(20_800, 0, 0),
        ]))
        .expect("reaches the console");
        assert_eq!(rotator.port.typed(), "\rmotorcon\rrun\ri\r");
    }

    #[test]
    fn a_console_that_never_answers_is_an_error() {
        for replies in [vec![], vec!["\r\nBOOTING\r\n"], vec![PROMPT]] {
            assert!(matches!(
                Winegard::open(Wire::answering(&replies)),
                Err(LinkError::Garbled(_))
            ));
        }
    }

    #[test]
    fn pointing_sends_one_command_per_axis() {
        let mut rotator = opened(&[&report(20_800, 0, 0), PROMPT, PROMPT]);
        rotator.point(sky(180.0, 45.0)).expect("writes");
        assert_eq!(rotator.port.typed(), "p 0 3300\rp 1 2600\r");
    }

    #[test]
    fn a_target_past_the_travel_stops_at_the_travel() {
        let mut rotator = opened(&[&report(20_800, 0, 0), PROMPT, PROMPT, PROMPT, PROMPT]);
        rotator.point(sky(540.0, 180.0)).expect("writes");
        rotator.point(sky(-10.0, -10.0)).expect("writes");
        assert_eq!(rotator.port.typed(), "p 0 8158\rp 1 5431\rp 0 0\rp 1 0\r");
    }

    #[test]
    fn stopping_holds_the_motors_where_they_stand() {
        let mut rotator = opened(&[
            &report(20_800, 0, 0),
            &report(20_800, 1_650, 866),
            PROMPT,
            PROMPT,
        ]);
        rotator.stop().expect("writes");
        assert_eq!(rotator.port.typed(), "i\rp 0 1650\rp 1 866\r");
    }

    #[test]
    fn a_position_is_read_back_in_degrees() {
        let mut rotator = opened(&[&report(10_750, 0, 0), &report(10_750, 1_650, 2_687)]);
        let position = rotator.position().expect("reads");
        assert!((position.azimuth_deg - 90.0).abs() < 0.01);
        assert!((position.elevation_deg - 90.0).abs() < 0.02);
    }

    #[test]
    fn a_silent_console_mid_conversation_is_an_error() {
        let mut rotator = opened(&[&report(20_800, 0, 0)]);
        assert!(matches!(rotator.position(), Err(LinkError::Garbled(_))));
    }
}
