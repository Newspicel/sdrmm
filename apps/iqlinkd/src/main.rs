use std::{
    io::Write as _,
    net::{TcpListener, TcpStream},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use sdrmm_iqlink::{CONTROL_PORT, Reply};

use crate::iio::Paths;

const HANDOVER: Duration = Duration::from_secs(2);
const HANDOVER_POLL: Duration = Duration::from_millis(20);

mod cpu;
mod iio;
mod loss;
#[cfg(target_os = "linux")]
mod mapped;
mod packets;
mod ring;
mod session;
mod udp;

fn main() -> ExitCode {
    let port = match port(std::env::args().skip(1)) {
        Ok(port) => port,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("iqlinkd: cannot listen on {port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("iqlinkd: listening on {port}");
    let busy = Arc::new(AtomicBool::new(false));
    for control in listener.incoming() {
        match control {
            Ok(control) => admit(control, &busy),
            Err(e) => eprintln!("iqlinkd: accept: {e}"),
        }
    }
    ExitCode::SUCCESS
}

fn port(mut args: impl Iterator<Item = String>) -> Result<u16, String> {
    let usage = || "usage: sdrmm-iqlinkd [--port N]".to_string();
    match (args.next().as_deref(), args.next(), args.next()) {
        (None, _, _) => Ok(CONTROL_PORT),
        (Some("--port"), Some(port), None) => port.parse().map_err(|_| usage()),
        _ => Err(usage()),
    }
}

fn admit(control: TcpStream, busy: &Arc<AtomicBool>) {
    let busy = busy.clone();
    thread::spawn(move || {
        if !claim(&busy, Instant::now() + HANDOVER) {
            let refusal = Reply::Refused("another session is streaming".to_string());
            if let Err(e) = (&control).write_all(refusal.line().as_bytes()) {
                eprintln!("iqlinkd: refusing a second session: {e}");
            }
            return;
        }
        let peer = control
            .peer_addr()
            .map_or_else(|_| "unknown".to_string(), |peer| peer.to_string());
        match session::serve(control, &Paths::default()) {
            Ok(()) => eprintln!("iqlinkd: session from {peer} ended"),
            Err(e) => eprintln!("iqlinkd: session from {peer}: {e}"),
        }
        busy.store(false, Ordering::Release);
    });
}

fn claim(busy: &AtomicBool, deadline: Instant) -> bool {
    loop {
        if !busy.swap(true, Ordering::AcqRel) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(HANDOVER_POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_port_defaults_and_can_be_moved() {
        assert_eq!(port(args(&[])), Ok(CONTROL_PORT));
        assert_eq!(port(args(&["--port", "4000"])), Ok(4000));
        assert!(port(args(&["--port"])).is_err());
        assert!(port(args(&["--port", "x"])).is_err());
        assert!(port(args(&["--port", "1", "2"])).is_err());
    }

    #[test]
    fn a_session_waits_for_the_previous_one_to_hand_over() {
        let busy = Arc::new(AtomicBool::new(true));
        let releaser = {
            let busy = busy.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                busy.store(false, Ordering::Release);
            })
        };
        assert!(claim(&busy, Instant::now() + Duration::from_secs(2)));
        releaser.join().expect("releaser");
        assert!(!claim(&busy, Instant::now() + Duration::from_millis(30)));
    }
}
