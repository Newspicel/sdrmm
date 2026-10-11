use std::io::{Read, Write};

use sdrmm_wire::{FLIP_ELEVATION_DEG, MAX_ROTATOR_AZIMUTH_DEG, RotatorAxes, SkyPoint};

use super::link::{Link, LinkError};

const MAX_REPLY: usize = 64;
const MAX_REPLIES: usize = 4;

pub(super) struct Gs232<P> {
    port: P,
    axes: RotatorAxes,
}

impl<P: Read + Write> Gs232<P> {
    pub(super) const fn new(port: P, axes: RotatorAxes) -> Self {
        Self { port, axes }
    }

    fn send(&mut self, command: &str) -> Result<(), LinkError> {
        self.port.write_all(command.as_bytes())?;
        self.port.flush()?;
        Ok(())
    }

    fn reply(&mut self) -> Result<String, LinkError> {
        let mut line = Vec::new();
        let mut byte = [0_u8];
        loop {
            self.port.read_exact(&mut byte)?;
            match byte[0] {
                b'\r' | b'\n' if line.is_empty() => {}
                b'\r' | b'\n' => return Ok(String::from_utf8_lossy(&line).into_owned()),
                other if line.len() < MAX_REPLY => line.push(other),
                _ => {
                    return Err(LinkError::Garbled(
                        String::from_utf8_lossy(&line).into_owned(),
                    ));
                }
            }
        }
    }
}

impl<P: Read + Write + Send> Link for Gs232<P> {
    fn position(&mut self) -> Result<SkyPoint, LinkError> {
        let query = match self.axes {
            RotatorAxes::AzimuthElevation => "C2\r",
            RotatorAxes::Azimuth => "C\r",
        };
        self.send(query)?;
        let mut last = String::new();
        for _ in 0..MAX_REPLIES {
            last = self.reply()?;
            if let Some(point) = parse(&last, self.axes) {
                return Ok(point);
            }
        }
        Err(LinkError::Garbled(last))
    }

    fn point(&mut self, aim: SkyPoint) -> Result<(), LinkError> {
        let command = command(aim, self.axes);
        self.send(&command)
    }

    fn stop(&mut self) -> Result<(), LinkError> {
        self.send("S\r")
    }
}

fn command(aim: SkyPoint, axes: RotatorAxes) -> String {
    let azimuth = aim.azimuth_deg.clamp(0.0, MAX_ROTATOR_AZIMUTH_DEG).round() as u16;
    let elevation = aim.elevation_deg.clamp(0.0, FLIP_ELEVATION_DEG).round() as u16;
    match axes {
        RotatorAxes::AzimuthElevation => format!("W{azimuth:03} {elevation:03}\r"),
        RotatorAxes::Azimuth => format!("M{azimuth:03}\r"),
    }
}

fn parse(reply: &str, axes: RotatorAxes) -> Option<SkyPoint> {
    let reply = reply.trim();
    let (azimuth_deg, elevation) = if let Some(azimuth) = after(reply, "AZ=") {
        (azimuth, after(reply, "EL="))
    } else {
        let mut fields = reply.strip_prefix('+')?.split('+');
        let azimuth = fields.next()?.trim().parse().ok()?;
        (azimuth, fields.next().and_then(|el| el.trim().parse().ok()))
    };
    let elevation_deg = match axes {
        RotatorAxes::Azimuth => 0.0,
        RotatorAxes::AzimuthElevation => elevation?,
    };
    Some(SkyPoint {
        azimuth_deg,
        elevation_deg,
    })
}

fn after(reply: &str, key: &str) -> Option<f64> {
    let rest = reply.get(reply.find(key)? + key.len()..)?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(rest.len());
    rest.get(..end)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const BOTH: RotatorAxes = RotatorAxes::AzimuthElevation;
    const TURNTABLE: RotatorAxes = RotatorAxes::Azimuth;

    struct Wire {
        replies: Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Wire {
        fn answering(replies: &str) -> Self {
            Self {
                replies: Cursor::new(replies.as_bytes().to_vec()),
                sent: Vec::new(),
            }
        }
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.replies.read(buf)
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

    #[test]
    fn both_controller_generations_report_a_position() {
        assert_eq!(parse("AZ=123  EL=045", BOTH), Some(sky(123.0, 45.0)));
        assert_eq!(parse("AZ=405 EL=170", BOTH), Some(sky(405.0, 170.0)));
        assert_eq!(parse("+0123+0045", BOTH), Some(sky(123.0, 45.0)));
        assert_eq!(parse("AZ=123", BOTH), None);
        assert_eq!(parse("?>", BOTH), None);
    }

    #[test]
    fn a_turntable_reports_only_its_azimuth() {
        assert_eq!(parse("AZ=123", TURNTABLE), Some(sky(123.0, 0.0)));
        assert_eq!(parse("+0123", TURNTABLE), Some(sky(123.0, 0.0)));
    }

    #[test]
    fn a_target_is_sent_as_whole_degrees() {
        assert_eq!(command(sky(359.7, 12.4), BOTH), "W360 012\r");
        assert_eq!(command(sky(-3.0, -3.0), BOTH), "W000 000\r");
        assert_eq!(command(sky(420.2, 150.0), BOTH), "W420 150\r");
        assert_eq!(command(sky(420.2, 150.0), TURNTABLE), "M420\r");
    }

    #[test]
    fn echoes_and_blank_lines_before_the_position_are_skipped() {
        let mut rotator = Gs232::new(Wire::answering("\r\nW120 030\r\nAZ=120  EL=030\r"), BOTH);
        assert_eq!(rotator.position().expect("reads"), sky(120.0, 30.0));
        assert_eq!(rotator.port.sent, b"C2\r");
    }

    #[test]
    fn a_silent_controller_is_an_error() {
        let mut rotator = Gs232::new(Wire::answering(""), BOTH);
        assert!(matches!(rotator.position(), Err(LinkError::Io(_))));
    }

    #[test]
    fn pointing_and_stopping_write_one_command_each() {
        let mut rotator = Gs232::new(Wire::answering(""), BOTH);
        rotator.point(sky(180.0, 45.0)).expect("writes");
        rotator.stop().expect("writes");
        assert_eq!(rotator.port.sent, b"W180 045\rS\r");
    }
}
